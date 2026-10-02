# ACP Turn State and Session Update Model

Canonical terms live in [Turns and Ownership](glossary.md#turns-and-ownership).
Existing `proactive_*` identifiers name presentation state only.

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

## Ownership Vocabulary

- A **client-owned turn** is initiated by this connection with
  `session/prompt`; its lifecycle is correlated to that request and ends with
  its response.
- An **agent-owned turn** is established on this connection by explicit Nori
  agent-turn metadata. ACP v1 has no corresponding primitive.
- An **unowned update** is an individual `session/update` received while this
  client has no active local request. It is not a protocol error or evidence
  of a turn.
- **Unowned presentation** groups unowned updates without creating a turn or
  request.

- while a local request is active, request-scoped updates belong to that
  request
- without a local request, request-scoped updates are unowned activity; warn
  once per unowned burst, but accept, preserve, and render them
- never invent a request ID or assign unowned activity to an earlier or later
  local request

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

There is no second request-lifecycle FSM beyond this. The TUI may retain
presentation-only state for grouping unowned output, but that state does not
own ACP requests or change this phase.

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

Tool snapshots persist across turns. They record a local owner when a request
created them, while unowned updates remain ownerless.

```rust
struct ToolSnapshot {
    owner_request_id: Option<JsonRpcId>,
    status: ToolStatus,
    title: String,
    kind: ToolKind,
    content: Vec<ToolCallContent>,
    locations: Vec<ToolCallLocation>,
    raw_input: Option<JsonValue>,
    raw_output: Option<JsonValue>,
}
```

`owner_request_id` is `Some(active.request_id)` for locally owned tool activity
and `None` for unowned activity. The client never invents ownership from
timing.

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

The harness reducer maintains private phase, persisted state, and local request
ownership while separately forwarding raw ACP traffic. The TUI builds its own
private display projections from that public traffic; neither projection
changes ACP ownership or semantics.

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

### 2. Request-scoped content updates

The following updates ordinarily belong to an active request:

- `user_message_chunk`
- `agent_message_chunk`
- `agent_thought_chunk`
- `plan`
- `tool_call`

Rules:

- if `active.is_some()`, patch `ActiveRequestState` or create request-owned state
- if `active.is_none()`, handle the update as unowned activity

More specifically:

- `user_message_chunk` appends only to `active.open_user_message`
- `agent_message_chunk` appends only to `active.open_agent_message`
- `agent_thought_chunk` appends only to `active.open_thought_message`
- `plan` patches `persisted.plan`
- `tool_call` creates or replaces `persisted.tool_calls[toolCallId]`; when
  active, it records that request as owner and adds the id to
  `active.tool_call_ids`, otherwise ownership remains `None`

The client never invents a turn owner when ACP did not provide one.

### 3. Unowned updates and agent-owned turns

Well-formed user, agent, thought, plan, or tool content received with
`active.is_none()` is a valid unowned update. The
harness normalizes and projects it after emitting `Received update with no
active local request` for the first non-metadata update in the unowned burst.
Later updates do not repeat the warning until a local prompt or load starts.
The warning does not reopen `active`, fabricate a request ID or completion
signal, or attribute the update to an earlier or later local request. An idle
user chunk therefore does not create a synthetic prompt phase.

The harness forwards schema-native ACP metadata and does not use it to complete
a prompt. Any unowned user, agent, thought, plan, or tool update starts or
confirms unowned presentation. Without a lifecycle hint, content still renders,
and a later local prompt or load start separates it without fabricating
completion.

Nori's brokered Sessions product may send status-only `SessionInfoUpdate`
notifications with `_meta.nori.status`:

- exact `working` establishes or confirms an agent-owned turn and starts its
  presentation when no local prompt or load is active
- exact `idle` ends that turn and completes its presentation under the same
  condition

The TUI hides a known status-only frame. If the same frame also carries
`title` or `updated_at`, those fields remain visible. Unknown status values are
ordinary session metadata. Agent ownership implies no queue, cancellation,
loop, permission, or request semantics.

### 4. Attributed tool updates

`tool_call_update` is special because it carries a stable `toolCallId`.

Rules:

- if `persisted.tool_calls` already contains the id, patch that snapshot
- if the id is unknown, normalize it from a default `ToolCall`, apply the
  update, and persist the resulting snapshot

Tool snapshots are persisted session state. Their optional ownership is
explicit via `owner_request_id`, not inferred from timing.

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

The response to `session/prompt` is the only locally owned prompt-turn
boundary. Presentation of unowned updates or an agent-owned turn does not
create an active backend prompt or publish a separate Nori completion event.

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

The harness may project reduced state into private client events for its own
behavior, but those events are not a public protocol or a second source of
truth.

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

The TUI instead normalizes raw public ACP traffic for display and observes Nori
phase events for locally owned lifecycle. Its unowned presentation bit may
group unowned output, but it cannot complete a prompt or mutate harness-owned
request state.

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

- TUI presentation state becomes an authority for ACP request lifecycle
- owned-turn completion is inferred from anything other than the request
  response
- open message buffers survive across request completion
- tool ownership is inferred from timing instead of stored explicitly
- cancel is treated as immediate idle
- queued prompts are merged back into the composer instead of remaining a FIFO
- request-scoped permission state lives outside the active request
- a session allows unordered reduction of inbound ACP messages

The simplest ACP-faithful design is the correct default.

## Appendix: ACP Cancel Turn Boundary Investigation

**Status: Historical investigation, superseded as current protocol guidance.**
The evidence artifacts were removed (see git history). The ACP-canonical
boundary now treats each Harness prompt as exactly one `session/prompt` request
and accepts a successful empty `EndTurn` as that request's terminal response. Nori does not
absorb the response or resend the prompt. Any remaining user-visible issue in
this scenario must be investigated from the correlated wire request/response,
not repaired with client-side cancel-tail retry logic.

### Summary

Nori releases the ACP session back to the UI too early after a cancelled prompt turn.
In the reproduced Claude ACP session, Nori emits `SessionPhaseChanged(Idle)` and
`PromptCompleted(stop_reason=Cancelled)` immediately after the cancelled prompt
response, then accepts the user's next prompt as a fresh turn. That follow-up
prompt is then immediately completed with `stopReason=end_turn` and no content.

This appears to be a Nori turn-boundary handling bug, not an ACP adapter bug.
The reproduced wire stream is permissible ACP behavior and matches the shape
that other ACP clients are expected to tolerate.

This document records the investigation only. It does not propose a fix.

### Impact

- User presses `Ctrl-C` during an ACP agent turn.
- Nori shows the interrupted state and returns the prompt.
- The next user prompt is accepted immediately.
- That prompt is then completed immediately with an empty `end_turn`.
- The UI appears to consume a stale stop/completion signal instead of treating
  the post-cancel tail as part of the cancelled turn's completion lifecycle.

### Reproduction

#### Environment

- Branch: `debug-acp-cancel-ordering-trace`
- Binary: `codex-rs/target/debug/nori`
- Agent: `claude-debug-acp`
- Logging:
  - `RUST_LOG=acp_event_flow=debug,sacp::jsonrpc::handlers=debug,nori_tui=info`
- Runtime home:
  - `.tmp/claude-debug-repro-home-2`

#### Steps

1. Start `nori --agent claude-debug-acp --skip-trust-directory`.
2. Submit:
   - `I'm testing something. Just run a foreground sleep 30 task, then say 'all done!'`
3. Wait until the tool call shows `sleep 30`.
4. Press `Ctrl-C`.
5. Submit:
   - `what have you finished`

#### Observed Result

- The cancelled turn ends.
- Nori returns to idle and accepts the follow-up prompt.
- The follow-up prompt immediately receives `stopReason=end_turn` with zero usage.
- The TUI jumps to a fresh prompt with no assistant answer.

### Raw Wire Evidence

The reproduced wire sequence for the main session is:

1. Client sends `session/cancel`.
2. Agent sends `session/update` with `usage_update`.
3. Agent responds to the original prompt request with `stopReason=cancelled`.
4. Client sends the next `session/prompt`.
5. Agent immediately responds with `stopReason=end_turn` and zero usage.

From the captured wire session log:

- line 22: `session/cancel`
- line 23: post-cancel `usage_update`
- line 24: cancelled prompt response
- line 25: next `session/prompt`
- line 26: immediate empty `end_turn`

This ordering is also visible in the original log copy:

- [codex-rs/debug-acp-claude.log](/home/clifford/Documents/source/nori/cli/.worktrees/debug-acp-cancel-ordering-trace/codex-rs/debug-acp-claude.log:22)

### ACP Specification References

The ACP reference explicitly allows final `session/update` traffic after cancel,
as long as those updates are sent before the cancelled prompt response:

- `acp-cancellation-spec.txt`
- Source reference:
  - `/home/clifford/Documents/source/nori/docs/references/acp-llms-full.txt:3048`
  - `/home/clifford/Documents/source/nori/docs/references/acp-llms-full.txt:3078`

Key points from the reference:

- The client may cancel with `session/cancel`.
- The agent must eventually respond to the original `session/prompt` with
  `stopReason=cancelled`.
- The agent may still send `session/update` notifications after receiving
  `session/cancel`, but before responding to `session/prompt`.
- The client should still accept those updates.

There is also a specific note in the `session/update` section that clients
should continue accepting updates after cancel:

- `acp-session-update-note.txt`
- Source reference:
  - `/home/clifford/Documents/source/nori/docs/references/acp-llms-full.txt:3895`

Nothing in the reproduced wire stream violates these requirements.

### Investigation Log

#### 1. Initial hypothesis check

The first investigation pass considered whether Nori was locally reordering
`session/update` notifications and prompt results while merging:

- transport notifications from `event_rx`
- prompt results from `prompt_result_rx`

Instrumentation was added at:

- transport ingress:
  - [codex-rs/acp/src/connection/sacp_connection.rs](/home/clifford/Documents/source/nori/cli/.worktrees/debug-acp-cancel-ordering-trace/codex-rs/acp/src/connection/sacp_connection.rs:168)
- relay merge point:
  - [codex-rs/acp/src/backend/spawn_and_relay.rs](/home/clifford/Documents/source/nori/cli/.worktrees/debug-acp-cancel-ordering-trace/codex-rs/acp/src/backend/spawn_and_relay.rs:258)
- reducer:
  - [codex-rs/acp/src/backend/session_reducer.rs](/home/clifford/Documents/source/nori/cli/.worktrees/debug-acp-cancel-ordering-trace/codex-rs/acp/src/backend/session_reducer.rs:44)
- runtime driver:
  - [codex-rs/acp/src/backend/session_runtime_driver.rs](/home/clifford/Documents/source/nori/cli/.worktrees/debug-acp-cancel-ordering-trace/codex-rs/acp/src/backend/session_runtime_driver.rs:127)

That tracing established that Nori did see the post-cancel `usage_update` before
the cancelled prompt response in the reproduced run.

#### 2. Added turn-boundary tracing

To determine exactly when Nori released control back to the UI, two more
tracing points were added:

- prompt admission into the ACP backend:
  - [codex-rs/acp/src/backend/user_input.rs](/home/clifford/Documents/source/nori/cli/.worktrees/debug-acp-cancel-ordering-trace/codex-rs/acp/src/backend/user_input.rs:164)
- emitted client events, especially:
  - `SessionPhaseChanged`
  - `PromptCompleted`
  - [codex-rs/acp/src/backend/session_runtime_driver.rs](/home/clifford/Documents/source/nori/cli/.worktrees/debug-acp-cancel-ordering-trace/codex-rs/acp/src/backend/session_runtime_driver.rs:304)

These extra traces are what closed the loop.

#### 3. Reproduced with boundary tracing enabled

The decisive sequence in the captured Nori ACP trace is:

1. Nori marks the active prompt as cancelling.
   - line 54
2. Nori forwards `SessionPhaseChanged(Cancelling)`.
   - line 56
3. Nori sends `session/cancel`.
   - line 57
4. Nori receives the cancelled prompt response.
   - lines 64-66
5. Nori finalizes that prompt response.
   - line 68
6. Nori forwards `SessionPhaseChanged(Idle)`.
   - line 70
7. Nori forwards `PromptCompleted(stop_reason=Cancelled)`.
   - line 71
8. Only after that, Nori accepts the user's follow-up prompt as a new turn.
   - line 72
9. That new turn immediately receives `stopReason=EndTurn`.
   - lines 78-85

This sequence is the key investigation result.

### What The Trace Proves

The new trace proves all of the following in the reproduced session:

#### A. Nori can receive and parse both stop reasons off the wire

Nori successfully parses:

- the cancelled response for the interrupted turn
- the subsequent `end_turn` response that arrives after the next `session/prompt`

Evidence:

- `wire-session.log` lines 24 and 26
- `nori-acp-trace.log` lines 64-66 and 78-80

So this is not a failure to deserialize or understand the wire payloads.

#### B. Nori releases the cancelled turn before the next logical stop boundary is fully resolved

Nori explicitly emits:

- `SessionPhaseChanged(Idle)` at line 70
- `PromptCompleted(Cancelled)` at line 71

and then admits the follow-up prompt at line 72.

That is the precise point where control returns to the UI and a new user turn
becomes possible.

#### C. The follow-up prompt is treated as a brand new turn

The follow-up prompt is not queued behind additional cancel-tail processing.
It is accepted from idle:

- `phase_before_submit="idle"`
- `active_request_id_before_submit="<none>"`

Evidence:

- `nori-acp-trace.log` line 72

#### D. The empty `end_turn` disrupts the following prompt turn, not the cancelled one

The prompt request for `what have you finished` is started with a new request id:

- line 73

That request then immediately gets `EndTurn`:

- lines 78-85

This means the disruptive `end_turn` is observed as the response to the new
prompt turn after Nori has already returned to idle.

### Comparison With SACP Ordering APIs

Nori's current prompt path uses `block_task()`:

- [codex-rs/acp/src/connection/sacp_connection.rs](/home/clifford/Documents/source/nori/cli/.worktrees/debug-acp-cancel-ordering-trace/codex-rs/acp/src/connection/sacp_connection.rs:527)

SACP documents that `block_task()` acknowledges the response immediately:

- `sacp-ordering-excerpt.txt`
- source:
  - `/home/clifford/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/sacp-10.1.0/src/jsonrpc.rs:2747`

By contrast, `on_receiving_result()` keeps ordering until the callback completes:

- `sacp-ordering-excerpt.txt`
- source:
  - `/home/clifford/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/sacp-10.1.0/src/jsonrpc.rs:2915`

The session-oriented helper in SACP uses `on_receiving_result()` for prompt
completion and then keeps reading updates until a stop reason is drained:

- `sacp-session-excerpt.txt`
- source:
  - `/home/clifford/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/sacp-10.1.0/src/session.rs:554`
  - `/home/clifford/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/sacp-10.1.0/src/session.rs:588`

This comparison does not by itself prove the root cause, but it is relevant
because Nori's current prompt handling takes the "ack immediately and process
later" path rather than a session-scoped "keep consuming until the turn is
actually over" path.

### Comparison With Toad

The example ACP client `toad` is also useful as a reference point.

Its conversation layer waits for `agent.send_prompt(prompt)` and only then
calls `agent_turn_over(stop_reason)`:

- `toad-conversation-excerpt.py`
- source:
  - `/home/clifford/Documents/source/nori/cli/.worktrees/plan-session-update-support/toad/src/toad/widgets/conversation.py:822`

Its ACP agent layer waits on the ACP prompt request and returns the stop reason:

- `toad-agent-excerpt.py`
- source:
  - `/home/clifford/Documents/source/nori/cli/.worktrees/plan-session-update-support/toad/src/toad/acp/agent.py:739`

Again, this report is not claiming a fix from Toad's implementation. The
comparison is included because the same adapter is known to work in other ACP
clients, so Nori's turn-boundary handling is the thing under investigation.

### Conclusion

The evidence gathered here supports the following bug statement:

> Nori completes and releases a cancelled ACP turn too early. It forwards
> `PromptCompleted(Cancelled)` and `SessionPhaseChanged(Idle)` immediately after
> the cancelled prompt response, then admits the next user prompt as a fresh
> turn. In the reproduced session, that fresh turn is immediately completed by
> an empty `end_turn`, producing the visible off-by-one stop-boundary bug.

This report intentionally stops at attribution and evidence. It does not
recommend or document a fix.
