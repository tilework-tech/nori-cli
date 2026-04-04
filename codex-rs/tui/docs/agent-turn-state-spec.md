# ACP Turn State and Session Update Model

## Goal

Define a minimal, ACP-faithful model for turn state and `session/update`
handling in the Nori TUI and ACP backend.

The design must:

- follow the ACP protocol as written
- avoid accidental complexity in the TUI
- remove duplicated turn-state bookkeeping between backend and TUI
- stay small enough that the implementation is likely to be a net negative diff

## Ground Truth

ACP gives the client two distinct flows to manage:

1. request-owned flows
   - `session/prompt`
   - `session/load`
2. session-owned updates
   - `session/update`
   - `session/request_permission`

The only reliable owner of unattributed streamed content is the request that is
currently in flight.

That means:

- prompt-turn content belongs to the active `session/prompt` request
- replay content belongs to the active `session/load` request
- session metadata is session state, not turn state
- the client must not invent inter-turn attribution rules for bare content

If ACP does not identify which request owns some streamed content, the client
should not guess.

## Architecture Reference

### Overview

Per ACP session, the runtime keeps exactly three state buckets:

1. `SessionPhase`
2. `SessionStore`
3. `OutgoingQueue`

Everything else should be derived from those.

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
- `Loading` means `session/load` owns streamed replay content until its response arrives.
- `Prompt` means `session/prompt` owns streamed prompt-turn content until its response arrives.
- `cancelling` means `session/cancel` has been sent for the active prompt, but the prompt still
  remains in flight until its response arrives.

There is no separate TUI-owned turn FSM beyond this.

### `SessionStore`

`SessionStore` holds the latest ACP session state that must survive across turns:

- `tool_calls: HashMap<ToolCallId, ToolSnapshot>`
- `plan: Option<PlanSnapshot>`
- `available_commands`
- `current_mode`
- `config_options`
- `session_info`
- `usage`
- transcript/history output

This is long-lived session state, not turn state.

### `OutgoingQueue`

`OutgoingQueue` is a client-local FIFO of user prompts that have not yet been
sent to ACP.

It has no protocol meaning. The agent never sees it.

It exists for exactly one reason: ACP allows only one prompt in flight per
session, but the user may keep typing while a request is active.

## Ownership Rules

### Request-owned content

The following updates are request-owned content:

- `user_message_chunk`
- `agent_message_chunk`
- `agent_thought_chunk`
- `plan`
- `tool_call`

Handling rule:

- accept them only while `SessionPhase` is `Loading` or `Prompt`
- route them to the active request owner
- if they arrive in `Idle`, handle them as out-of-phase updates (see
  "Out-of-phase updates" under Design Choice 3)

This is intentionally strict about attribution. Content that arrives outside of
a request is never attributed to a turn. It is rendered between turns with a
warning, not silently absorbed into adjacent turns.

### Session-owned metadata

The following updates are session metadata:

- `available_commands_update`
- `current_mode_update`
- `config_option_update`
- `session_info_update`
- `usage_update`

Handling rule:

- accept them in any phase
- patch `SessionStore`
- never treat them as turn boundaries

### Attributed tool updates

`tool_call_update` is special because it carries a stable `toolCallId`.

Handling rule:

- if the `toolCallId` is known, patch the existing tool snapshot in any phase
- if the `toolCallId` is unknown, emit a user-visible warning and ignore the update

This keeps tool updates simple and avoids creating orphan client objects from
partial information.

## Design Choices

### 1. The backend owns ACP request state

The ACP backend should own `SessionPhase`.

The TUI should consume derived state such as:

- whether input is currently locked
- whether an interrupt is available
- whether the active prompt is cancelling

The TUI should not separately store or mutate ACP connection state.

All state regarding the ACP connection and the running ACP agent must derive
from backend-owned session state.

### 2. The JSON-RPC response is the only request boundary

For prompt turns:

- `session/prompt` begins ownership
- the prompt response ends ownership

For replay loads:

- `session/load` begins ownership
- the load response ends ownership

`session/cancel` does not end a prompt.

This is the core guardrail against drifting from ACP again.

### 3. No synthetic turn-boundary events for ACP correctness

The implementation may expose derived lifecycle notifications if that is the
cleanest way to keep the TUI event-driven.

If such notifications exist, they must be projections of backend-owned
`SessionPhase`. They must not become an independent source of truth.

If a piece of logic needs to know whether a prompt is still active, it should
ask the backend-owned `SessionPhase`, not infer it from translated lifecycle
events.

Recommended push shape:

```rust
enum AcpSessionPhaseView {
    Idle {
        /// Present when transitioning from `Prompt` or `Loading`.
        /// Absent on initial idle (session created but no request yet).
        completed: Option<TurnCompletion>,
    },
    Loading,
    Prompt,
    Cancelling,
}

struct TurnCompletion {
    stop_reason: StopReason,
    /// The backend evaluates drain eligibility atomically with the phase
    /// transition. The TUI does not re-derive this.
    drained: Option<DrainedPrompt>,
}

struct DrainedPrompt {
    /// The prompt that was dequeued and sent. The TUI should render it as the
    /// next user message immediately; the session is already back in `Prompt`.
    prompt: Vec<ContentBlock>,
}
```

The backend emits `AcpSessionPhaseView` whenever `SessionPhase` changes.

Rules:

- `Idle { completed: None }` maps from `SessionPhase::Idle` when no request
  preceded it (initial state, or after explicit reset).
- `Idle { completed: Some(..) }` maps from `SessionPhase::Idle` when a prompt
  or load response just arrived. The `TurnCompletion` carries everything the
  TUI needs to render the outcome and decide next steps.
- `Loading` maps from `SessionPhase::Loading { .. }`
- `Prompt` maps from `SessionPhase::Prompt { cancelling: false, .. }`
- `Cancelling` maps from `SessionPhase::Prompt { cancelling: true, .. }`

When `completed.drained` is `Some`, the backend has already dequeued one prompt
and sent it as a new `session/prompt`. A second `PhaseView(Prompt)` event
follows immediately. The TUI receives both in order and never needs to
independently decide whether to drain.

This gives the TUI a concise push-based rendering signal without introducing a
second authority for ACP state. The TUI gets everything it needs from one
event — stop reason, drain outcome, and phase — and never re-derives drain
policy.

#### Out-of-phase updates

Multiple ACP agents send well-formed content updates (tool calls, agent
thought, agent message chunks) outside of an active `session/prompt` request.
This behavior is not well-defined in the ACP spec, but it is a reality of the
systems we integrate with. Silently dropping these updates would hide
information the user cares about and remove the visibility needed to eventually
harden the spec around this behavior.

Handling rule:

- The backend tags the update as out-of-phase (a boolean flag, not an envelope
  wrapper — the update itself is unchanged).
- If the update is well-formed (parses as a valid `SessionUpdate` variant), the
  backend forwards it to the TUI with the out-of-phase tag.
- The TUI renders a short warning banner before the content, then renders the
  content normally in the history. This content is not attributed to any turn —
  it appears between turns as standalone output.
- If the update is malformed or unrecognizable, the backend logs a warning and
  drops it.

This avoids the complexity of an envelope struct wrapping every event. The
common path (in-phase updates) carries no extra data. Only the exceptional
path gets a flag.

The goal is observability: learn when and how often agents do this, what
content they send, and whether patterns emerge that warrant first-class
protocol support. Until then, the TUI shows it honestly with a warning rather
than pretending it didn't happen.

### 4. No inter-turn attribution heuristics

The client must not:

- stash bare content updates while idle and prepend them later
- append bare content updates to a previous turn based on timing guesses
- treat wire order alone as proof of message ownership

Those policies add accidental complexity and are not grounded in ACP.

### 5. The queue is outbound-only

Queued user prompts are unsent local drafts.

They are not:

- part of the current turn
- restored into the composer on cancel
- merged into a synthetic user message

They remain a FIFO until the backend returns to `Idle` and explicitly drains one.

## Final Behavior

### Submitting a prompt

If `SessionPhase == Idle`:

- send `session/prompt`
- enter `Prompt { cancelling: false }`

If `SessionPhase != Idle`:

- append the user prompt to `OutgoingQueue`
- do not send anything to ACP yet

### Cancelling a prompt

If `SessionPhase == Prompt { cancelling: false }`:

- send `session/cancel`
- mark non-finished tool calls for the active prompt as cancelled in the UI
- resolve pending permission requests with `cancelled`
- stay in `Prompt`, but set `cancelling: true`

If already cancelling, do nothing.

The session does not become idle until the prompt response arrives.

### Prompt response handling

When the response to the active `session/prompt` arrives, the backend executes
one atomic step:

1. Evaluate `stopReason`.
2. If `stopReason == end_turn` and `OutgoingQueue` is non-empty:
   - dequeue exactly one prompt
   - send it as a new `session/prompt`
   - transition to `Prompt { cancelling: false }` with the new request id
   - emit `PhaseView(Idle { completed: Some(TurnCompletion { stop_reason, drained: Some(..) }) })`
   - immediately emit `PhaseView(Prompt)`
3. Otherwise:
   - transition to `Idle`
   - emit `PhaseView(Idle { completed: Some(TurnCompletion { stop_reason, drained: None }) })`

The drain decision is evaluated inside the phase transition, not after it. The
TUI never observes an `Idle` gap where it must independently decide whether to
drain. This is the ACP equivalent of pi-mono's outer loop, where follow-up
drain and loop continuation are adjacent lines in the same function.

Queue drain eligibility is opinionated and simple:

- auto-drain after `end_turn`
- do not auto-drain after `cancelled`
- do not auto-drain after `refusal`
- do not auto-drain after `max_tokens`
- do not auto-drain after `max_turn_requests`
- do not auto-drain after transport or protocol errors

Those non-success cases leave the queue visible and let the user decide what to
do next.

### Loading a session

If `SessionPhase == Idle`:

- send `session/load`
- enter `Loading`

While loading:

- accept replay content updates and route them to transcript/history
- accept metadata updates and patch `SessionStore`
- do not drain queued prompts

When the load response arrives:

- transition to `Idle`
- emit `PhaseView(Idle { completed: Some(TurnCompletion { stop_reason: end_turn, drained: None }) })`
- do not auto-send queued prompts

Default design: `session/load` is an idle-only restore operation. Finishing a
load only restores session state. It does not implicitly start a new turn.
The `drained` field is always `None` after a load — loads never trigger queue
drain.

### Metadata updates

Metadata updates are accepted in any phase.

They must not:

- unlock input
- complete a prompt
- start a prompt
- drain the queue

### Tool call updates

`tool_call` creates the snapshot.

`tool_call_update` patches the snapshot by `toolCallId`.

Tool snapshots are session state with request-local rendering.

That means:

- the snapshot survives after the request completes
- a later attributed update may patch it
- unattributed content may not attach itself to it by heuristic

## Open Design Questions

### Type-level enforcement of phase transitions

`SessionPhase` as specified is a plain enum. Nothing prevents code from writing
`SessionPhase::Idle` directly, bypassing drain evaluation, event emission, or
cleanup. This is the same class of bug the spec exists to prevent — duplicated
bookkeeping — except at the implementation level instead of the architecture
level.

The next step is to define `SessionPhase` as an opaque struct with methods that
enforce the valid transition graph:

```
Idle  →  Prompt    (begin_prompt)
Idle  →  Loading   (begin_load)
Prompt → Cancelling (begin_cancel)
Prompt → Idle      (complete_prompt — evaluates drain atomically)
Cancelling → Idle  (complete_prompt — drain is never eligible)
Loading → Idle     (complete_load — drain is never eligible)
```

Each method would return the `AcpSessionPhaseView` to emit and, for
`complete_prompt`, the `TurnCompletion` including any drained prompt. Invalid
transitions would be compile-time or runtime errors rather than silent
corruption.

This is discussed in a follow-up section and is not yet specified here.

### Hook orchestration as middleware

The current implementation inlines 12 hook invocations (6 lifecycle points ×
sync + async variants) directly inside the prompt handler. This mixes turn-state
management with cross-cutting concerns (transcript recording, ghost snapshots,
idle timers, summary tasks) in a single 564-line function.

The next step is to define a middleware layer that the phase machine calls at
well-defined lifecycle points, analogous to pi-mono's `beforeToolCall` /
`afterToolCall` config hooks. The phase machine handles transitions; middleware
handles side effects.

This is discussed in a follow-up section and is not yet specified here.

## Non-Goals

This document does not define:

- a full implementation plan
- speculative handling for future ACP message-id proposals
- heuristics for background-task narration after prompt completion
- UI polish details such as exact spinner wording or footer copy

If a behavior requires guessing ownership, it is out of scope for this design.

## Drift Guards

When changing ACP turn handling in the future, the design should be rejected if
it introduces any of these smells:

- the TUI and backend both track whether a prompt is active
- turn completion is inferred from anything other than the request response
- idle bare content is buffered for later attribution
- cancel is treated as immediate idle
- queued prompts are merged back into the composer instead of remaining a FIFO

The simplest ACP-faithful design is the correct default.

If a future feature needs more state, it must justify why `SessionPhase`,
`SessionStore`, and `OutgoingQueue` are insufficient.

## Footguns Removed

- `session/cancel` no longer implies idle
- queued prompts are no longer restored into the composer
- the TUI no longer acts as an independent authority on whether an ACP prompt is active
- bare idle content updates are no longer heuristically attached to a previous or future turn
