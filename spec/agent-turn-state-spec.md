# ACP Turn State and Session Update Model

## Goal

Define a minimal, ACP-faithful model for turn state and `session/update`
handling in the Nori TUI and ACP backend.

The design must:

- follow the ACP protocol as written
- avoid accidental complexity in the TUI
- remove duplicated turn-state bookkeeping between backend and TUI
- stay small enough that the implementation is likely to be a net negative diff

## ACP Ground Truth

ACP gives the client two distinct ownership boundaries:

1. request-owned flows
   - `session/prompt`
   - `session/load`
2. session-owned flows
   - `session/update`
   - `session/request_permission`

The protocol rules that matter most here are:

- a prompt turn begins when the client sends `session/prompt`
- streamed prompt-turn content arrives via `session/update`
- the prompt turn ends only when the response to that same `session/prompt`
  arrives with a `stopReason`
- `session/cancel` does not end the prompt; the prompt remains active until its
  response arrives
- `session/load` replays conversation state via `session/update` and finishes
  only when the `session/load` response arrives
- ACP explicitly calls out ordering hazards between `session/update`
  notifications and request responses
- ACP currently has no stable message identity for chunk updates, which means
  clients must keep message assembly local to the active request and avoid
  cross-turn heuristics

This design follows those boundaries exactly.

## Proposed Runtime Model

Per ACP session, the backend owns exactly one runtime object:

```rust
struct SessionRuntime {
    phase: SessionPhase,
    persisted: PersistedSessionState,
    active: Option<ActiveRequestState>,
    queue: VecDeque<QueuedPrompt>,
}
```

Everything else should be derived from this.

### `SessionPhase`

`SessionPhase` is the single source of truth for whether the session is idle,
loading history, or processing a prompt.

```rust
enum SessionPhase {
    Idle,
    Loading {
        request_id: JsonRpcId,
    },
    Prompt {
        request_id: JsonRpcId,
        cancelling: bool,
    },
}
```

Properties:

- `Idle` means no ACP request currently owns streamed content.
- `Loading` means `session/load` owns replay content until its response arrives.
- `Prompt` means `session/prompt` owns prompt-turn content until its response
  arrives.
- `cancelling` means `session/cancel` has been sent for the active prompt, but
  the prompt still remains in flight until its response arrives.

There is no second TUI-owned turn FSM beyond this.

### `PersistedSessionState`

`PersistedSessionState` is the long-lived session state that survives across
request boundaries.

```rust
struct PersistedSessionState {
    transcript: Transcript,
    plan: Option<PlanSnapshot>,
    tool_calls: HashMap<ToolCallId, ToolSnapshot>,
    available_commands: AvailableCommands,
    current_mode: Option<ModeSnapshot>,
    config_options: ConfigOptions,
    session_info: Option<SessionInfoSnapshot>,
    usage: Option<UsageSnapshot>,
}
```

This is session state, not turn state.

### `ActiveRequestState`

`ActiveRequestState` is the concrete in-flight request state. It is the ACP
analogue of pi-mono's active run.

```rust
enum ActiveRequestKind {
    Loading,
    Prompt,
}

struct ActiveRequestState {
    request_id: JsonRpcId,
    kind: ActiveRequestKind,
    open_user_message: Option<OpenMessage>,
    open_agent_message: Option<OpenMessage>,
    open_thought_message: Option<OpenMessage>,
    tool_call_ids: IndexSet<ToolCallId>,
    pending_permission_requests: HashSet<JsonRpcId>,
}
```

This is where all in-flight, request-local state lives. If a piece of state does
not survive the request boundary, it belongs here.

### `OpenMessage`

Because ACP chunk updates currently do not reliably identify individual
messages, each active request owns at most one open message buffer per stream
kind.

```rust
struct OpenMessage {
    message_id: Option<String>,
    chunks: Vec<ContentBlock>,
}
```

The buffer exists only inside the active request. It must never survive into
`Idle`.

### `ToolSnapshot`

Tool snapshots persist across turns, but each one records which request created
it.

```rust
struct ToolSnapshot {
    owner_request_id: JsonRpcId,
    status: ToolStatus,
    title: String,
    kind: ToolKind,
    content: Vec<ToolCallContent>,
    locations: Vec<ToolCallLocation>,
    raw_input: Option<JsonValue>,
    raw_output: Option<JsonValue>,
}
```

`owner_request_id` is required so that cancellation, stale updates, and
request-local rendering stay precise without heuristics.

### `OutgoingQueue`

`OutgoingQueue` is a client-local FIFO of user prompts that have not yet been
sent to ACP.

It has no protocol meaning. The agent never sees it.

It exists for exactly one reason: ACP allows only one prompt in flight per
session, but the user may keep typing while a request is active.

Queued prompts are unsent local drafts. They are not:

- part of the active request
- restored into the composer on cancel
- merged into a synthetic user message

## Serialized Reducer

Per session, all ACP traffic must flow through one ordered reducer:

```rust
fn reduce(session: &mut SessionRuntime, event: InboundAcpEvent) -> Vec<UiEvent>
```

Requirements:

- `session/update` notifications are reduced serially
- `session/request_permission` requests are reduced serially
- `session/prompt` responses are reduced serially
- `session/load` responses are reduced serially
- transport or protocol errors that affect the session are reduced serially
- no later inbound message for that session is processed until the current one
  is fully handled

This is not an optimization. It is a correctness requirement. ACP explicitly
calls out that unordered handling can allow a request response to overtake a
prior `session/update`, which makes turn completion ambiguous in practice.

The backend reduces state first, then emits derived UI events. The TUI consumes
those projections. It does not race the backend for authority.

## Routing Rules

The reducer answers one question for every inbound message:

- does it patch `persisted`
- does it patch `active`
- does it finish `active`

### 1. Session metadata updates

The following updates patch `persisted` in any phase:

- `available_commands_update`
- `current_mode_update`
- `config_option_update`
- `session_info_update`
- `usage_update`

Rules:

- accept them in `Idle`, `Loading`, or `Prompt`
- patch `PersistedSessionState`
- never treat them as turn boundaries

### 2. Request-owned content updates

The following updates require an active request:

- `user_message_chunk`
- `agent_message_chunk`
- `agent_thought_chunk`
- `plan`
- `tool_call`

Rules:

- if `active.is_some()`, patch `ActiveRequestState` or create request-owned state
- if `active.is_none()`, handle the update as out-of-phase content

More specifically:

- `user_message_chunk` appends only to `active.open_user_message`
- `agent_message_chunk` appends only to `active.open_agent_message`
- `agent_thought_chunk` appends only to `active.open_thought_message`
- `plan` patches `persisted.plan`
- `tool_call` creates or replaces `persisted.tool_calls[toolCallId]`, sets
  `owner_request_id = active.request_id`, and adds the id to
  `active.tool_call_ids`

The client never invents a turn owner when ACP did not provide one.

### 3. Out-of-phase request content

The NDJSON event stream can contain well-formed request-shaped content outside
an active `session/prompt` or `session/load`. The backend therefore has to
handle that path anyway, even if only to log and drop it.

The observable behavior should be:

- if a request-owned content update arrives with `active.is_none()`, emit
  `UiEvent::Warning` once per burst — only the first such update since the
  last active request emits the warning, subsequent updates in the same
  idle window do not. The flag resets when a new prompt or load begins.
- forward the well-formed content to the TUI as standalone between-turn output
  (every update, regardless of whether the warning fired)
- do not attribute that content to a prior or future request
- do not reopen `active`
- if the update is malformed or unrecognizable, log a warning and drop it

This keeps the protocol handling honest without adding attribution heuristics to
the core reducer, and prevents post-cancel update bursts from spamming the
history with identical warning cells.

### 4. Attributed tool updates

`tool_call_update` is special because it carries a stable `toolCallId`.

Rules:

- if `persisted.tool_calls` already contains the id, patch that snapshot
- if the id is unknown, emit `UiEvent::Warning` and ignore the update

Tool snapshots are persisted session state. Their ownership is explicit via
`owner_request_id`, not inferred from timing.

### 5. Permission requests

`session/request_permission` is neither plain session metadata nor plain turn
content. It is a request scoped to the active prompt.

Rules:

- require `phase == SessionPhase::Prompt { .. }`
- record the permission request id in `active.pending_permission_requests`
- emit `UiEvent::PermissionRequested`
- if no prompt is active, emit `UiEvent::Warning` and reject or fail the request
  per transport policy

Pending permission requests must live inside the active prompt so cancellation
can resolve them deterministically.

## Message Assembly

ACP today leaves same-type chunk boundaries ambiguous. The client therefore uses
one minimal assembly rule:

- assemble chunks only inside the current `ActiveRequestState`
- keep at most one open message per stream kind when no `messageId` is present
- if ACP starts providing `messageId`, append only to the matching open message
- never carry an open message across request completion
- never assemble across `Idle`

This is the smallest acceptable rule until ACP message ids are standardized.

When a request completes, the reducer finalizes any open messages from `active`
into `persisted.transcript`, in request-local order, before clearing `active`.

## Lifecycle

### Submitting a prompt

If `phase == Idle`:

- send `session/prompt`
- create `active = Some(ActiveRequestState { kind: Prompt, .. })`
- set `phase = Prompt { request_id, cancelling: false }`

If `phase != Idle`:

- append the user prompt to `queue`
- do not send anything to ACP yet

### Cancelling a prompt

If `phase == Prompt { cancelling: false, .. }`:

- send `session/cancel`
- set `cancelling = true`
- mark every non-finished tool snapshot with
  `owner_request_id == active.request_id` as cancelled in the UI
- resolve every id in `active.pending_permission_requests` with the ACP
  `cancelled` outcome
- keep `active` intact

If already cancelling, do nothing.

`session/cancel` does not transition the session to `Idle`. The prompt remains
active until the response to the original `session/prompt` arrives.

### Prompt response handling

The response to `session/prompt` is the only prompt-turn boundary.

When the response to the active prompt arrives, the reducer performs one ordered
completion step:

1. finalize open messages from `active` into `persisted.transcript`
2. clear `active`
3. set `phase = Idle`
4. emit `UiEvent::PromptFinished`
5. evaluate queue drain policy

If queue drain is eligible:

- dequeue exactly one prompt
- send a new `session/prompt`
- create a fresh `active`
- set `phase = Prompt { cancelling: false, .. }`
- emit the resulting `UiEvent::QueueChanged` and `UiEvent::PhaseChanged`

The reducer owns this whole sequence. The TUI never observes an ambiguous gap
where it must decide for itself whether the queue should drain.

### Load handling

If `phase == Idle`:

- send `session/load`
- create `active = Some(ActiveRequestState { kind: Loading, .. })`
- set `phase = Loading { request_id }`

While loading:

- accept replay content updates into `active`
- accept metadata updates into `persisted`
- do not drain queued prompts

When the load response arrives, the reducer performs one ordered completion
step:

1. finalize replayed open messages from `active` into `persisted.transcript`
2. clear `active`
3. set `phase = Idle`
4. emit `UiEvent::LoadFinished`

Loads never auto-drain the outbound queue.

## Queue Drain Policy

Queue draining is policy, not protocol truth.

The backend should distinguish two drain outcomes:

```rust
enum QueueDrainOutcome {
    SendNextPrompt,
    RestoreForEditing,
    LeaveQueued,
}
```

Default policy:

- `end_turn` should drain by dequeuing the next prompt and sending it as the
  next `session/prompt`
- other stop reasons may still drain by restoring the next queued prompt for
  editing, but only if doing so would not overwrite an in-progress edit
- otherwise, leave queued prompts in `OutgoingQueue`

This keeps ACP ownership simple while still supporting the intended UX split
between "continue immediately" and "surface the next queued draft for editing."

## UI Event Surface

The backend may project reduced state into UI events, but those events are not a
second source of truth.

A flat event model is sufficient:

```rust
enum UiEvent {
    PhaseChanged(SessionPhaseView),
    TranscriptPatched,
    ToolPatched(ToolCallId),
    PlanPatched,
    SessionMetadataPatched,
    PermissionRequested(PermissionRequestView),
    PromptFinished(PromptFinishedView),
    LoadFinished,
    QueueChanged,
    Warning(WarningView),
}
```

Recommended `SessionPhaseView`:

```rust
enum SessionPhaseView {
    Idle,
    Loading,
    Prompt,
    Cancelling,
}
```

The TUI reads these events as projections of backend-owned state. If a piece of
logic needs to know whether a prompt is active, it should ask the backend-owned
session runtime, not infer it from timing or event order.

## Non-Goals

This document does not define:

- a full implementation plan
- speculative handling beyond the current ACP message-id direction
- heuristics for attaching idle bare content to a previous or future turn
- UI polish details such as exact spinner wording or footer copy

If a behavior requires guessing request ownership, it is out of scope for this
design.

## Drift Guards

Future changes should be rejected if they introduce any of these smells:

- the TUI and backend both track whether a prompt is active
- turn completion is inferred from anything other than the request response
- open message buffers survive across request completion
- tool ownership is inferred from timing instead of stored explicitly
- cancel is treated as immediate idle
- queued prompts are merged back into the composer instead of remaining a FIFO
- request-scoped permission state lives outside the active request
- a session allows unordered reduction of inbound ACP messages

The simplest ACP-faithful design is the correct default.
