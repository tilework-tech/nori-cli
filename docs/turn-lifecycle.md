# ACP Turn State and Session Update Model

Terms follow [Turns and Ownership](glossary.md#turns-and-ownership) and
[Tools, Execution, and Permissions](glossary.md#tools-execution-and-permissions).
Existing `proactive_*` and `observer_*` identifiers name
[unowned presentation](glossary.md#turn-ownership) and
[agent-owned turn](glossary.md#turn-ownership) state only.

## ACP Ground Truth

Nori follows ACP v1 as written:

- A [prompt turn](glossary.md#turn-ownership) begins with one
  [prompt request](glossary.md#protocol) and ends only when that request's
  [prompt response](glossary.md#protocol) arrives with a `stopReason`.
- `session/cancel` does not end the turn. The agent may keep sending
  [session updates](glossary.md#protocol) until it answers the original
  `session/prompt` with `cancelled`; the client must accept them.
- `session/load` replays history through `session/update` and finishes only
  when the load response arrives.
- ACP chunk updates carry no reliable message identity, so message assembly
  stays local to the [active local request](glossary.md#protocol).
- ACP v1 has no agent-initiated turn. Nori's
  [agent-turn metadata](glossary.md#nori-extension) is an extension.

## Ownership

- The **session harness** owns ACP request lifecycle. The TUI, remote
  controllers, and observers derive their view from the harness's public event
  stream; none of them may complete, cancel, or open a request on their own
  authority.
- A turn is client-owned only on the connection that sent its prompt request.
  Ownership never implies cancellation or permission rights on another
  connection.
- Never invent a request ID, and never attribute an
  [unowned update](glossary.md#turn-ownership) to an earlier or later local
  request.

## Runtime State

Each ACP session has one `SessionRuntime`
(`nori-rs/harness/src/normalized/session_runtime.rs`):

| Field | Meaning |
| --- | --- |
| `phase` | `Idle`, `Loading { request_id }`, or `Prompt { request_id, cancelling }`. The single source of truth for whether a local request is active. |
| `active` | `ActiveRequestState`: open message buffers, the request's tool call IDs, pending permission request IDs, and the originating `QueuedPrompt`. Exists only while `phase` is not `Idle`. |
| `persisted` | Transcript, plan, tool snapshots, commands, mode, config options, session info, and usage. Survives request boundaries. |
| `queue` | Client-local FIFO of prompts not yet sent. Invisible to the agent. |
| `observer_turn_active` | Set by `working`/`idle` agent-turn metadata while no local request is active. Never claims the request slot. |
| `orphan_update_warning_emitted` | Rate-limits the unowned-update warning to once per burst. |

The public projection is `nori_protocol::SessionPhase`: `Idle`,
`Loading { request_id }`, `Prompting { request_id }`, and
`Cancelling { request_id }`, published as `NoriEvent::SessionPhaseChanged`.
The `request_id` is the ACP wire request ID.

## Serialized Reducer

All traffic that affects `SessionRuntime` must flow through one ordered
reducer, `reduce()` in `nori-rs/harness/src/backend/session_reducer.rs`,
driven by a single runtime task that consumes one channel:

- `session/update` notifications, `session/request_permission` requests,
  prompt responses and failures, load responses, local prompt submits, and
  cancels are all reduced in arrival order, one at a time.
- The **ACP host** publishes every notification, request, and response onto
  one ordered connection inbox. Do not split public ACP traffic into racing
  channels.
- The relay `select!` is `biased` toward the connection inbox over prompt
  results. A prompt task publishes its raw response to the inbox before it
  reports the stop reason, so every update the agent sent before the response
  is reduced before the turn completes.
- Reducer output is side effects (`SendPrompt`, `SendCancel`,
  `ResolvePermissionCancelled`, `RejectPromptBusy`) that the driver executes
  after reduction.

## Routing Rules

### Session metadata

`available_commands_update`, `current_mode_update`, `config_option_update`,
`session_info_update`, and `usage_update` patch `persisted` in any phase and
are never turn boundaries.

### Request-scoped content

`user_message_chunk`, `agent_message_chunk`, `agent_thought_chunk`, `plan`,
`tool_call`, and `tool_call_update`:

- With `active` set, text chunks append to the open buffer of their kind.
  A chunk of a different kind first flushes the other open buffers into the
  transcript, which preserves interleaved order. Non-text chunks are not
  assembled.
- Without `active`, the update is unowned. The harness emits
  `Received update with no active local request` once per burst (reset when a
  local prompt or load starts, or on `idle` agent-turn metadata), still
  normalizes and publishes it, and does not assemble it into the transcript.
  The warning is suppressed while an agent-owned turn is active.
- Open buffers never survive request completion and are never assembled
  across `Idle`.
- Tool snapshots are keyed by `toolCallId`. An unknown
  `tool_call_update` is normalized from a default `ToolCall`. A snapshot's
  `owner_request_id` is the active request at the time of the update, or
  `None` when unowned; it is never inferred from timing.

### Agent-owned turns

`SessionInfoUpdate` carrying `_meta.nori.status`:

- exact `working` establishes an agent-owned turn when no local prompt or load
  is active; exact `idle` ends it. Both are ignored while a local request is
  active.
- other values are ordinary session metadata.
- The TUI hides a [status-only frame](glossary.md#nori-extension); a frame that
  also carries `title` or `updated_at` keeps those fields visible.
- Agent ownership grants no queue, cancel, loop, or permission controls. The
  TUI must not send `Cancel` for an agent-owned turn.

### Permission requests

- A `session/request_permission` is valid only in the `Prompt` phase. Its ID
  is recorded in `active.pending_permission_requests`.
- Outside a prompt, including during bootstrap and `session/load`, the harness
  answers with the `cancelled` [permission outcome](glossary.md#permission-and-policy);
  after setup it also publishes a warning.
- Under approval policy `Never`, the harness selects the first allow option
  without surfacing the request.

## Lifecycle

### Submitting a prompt

- From `Idle`, the reducer creates `active`, enters `Prompt`, and emits
  `SendPrompt`. Otherwise a local prompt is appended to `queue` and
  `QueueChanged` is published.
- Each queued or submitted prompt is sent as exactly one `session/prompt`.
- The harness assigns a provisional ID, then rewrites `phase` and `active` to
  the transport-assigned wire ID (`PromptStarted`) before publishing
  `SessionPhaseChanged(Prompting)`. Consumers correlate on the wire ID only.

### Turn brackets for observers

Remote control and Nori cloud observers do not receive the initiator's prompt
response, so the harness brackets every local turn with agent-turn metadata.
On its public stream it must publish, in order:

1. `SessionPhaseChanged(Prompting { request_id })`
2. a status-only `working` frame
3. the original prompt content as `user_message_chunk` updates with
   `messageId` set to the prompt event ID and `_meta["nori.dev/promptEchoId"]`
4. agent updates and the raw prompt response
5. `SessionPhaseChanged(Idle)`, then a status-only `idle` frame

Transport events wait on the prompt phase gate until step 3 is published, so
no agent output precedes the bracket. `idle` is published for every terminal
outcome, including cancellation and failure.

When a downstream agent (for example the session broker) echoes the active
prompt back as a `user_message_chunk` whose `promptEchoId` matches and whose
content is part of the sent prompt, the harness suppresses that echo so the
prompt renders once. A user message without a matching echo ID is never
suppressed, even during an active prompt.

### Cancelling a prompt

From `Prompt { cancelling: false }` the reducer:

- sets `cancelling = true` and publishes `SessionPhaseChanged(Cancelling)`
- marks unfinished tool snapshots owned by the request as failed (ACP has no
  `cancelled` tool call status)
- resolves every pending permission request with the `cancelled` outcome
- sends `session/cancel` and keeps `active` intact

A second cancel is a no-op. Cancel never transitions to `Idle`; the prompt
stays active until its response arrives.

If no response arrives, the harness publishes a "slow to cancel" notice after
3 s and, after 10 s, aborts the prompt task and completes the turn as
`Cancelled`. Child exit fails the active prompt as fatal. These are the only
completions not driven by a prompt response.

### Completing a prompt

The prompt response is the only locally owned turn boundary. On it the reducer
finalizes open buffers into the transcript, clears `active`, enters `Idle`,
and emits `PromptCompleted`. A prompt that fails on the transport completes as
`Cancelled` with a failure disposition.

Cancellation boundary rules:

- One prompt is one `session/prompt` request. The response to that request,
  whatever its content, is its terminal response.
- A successful empty `EndTurn` is a valid terminal response. Never swallow it,
  absorb it into a previous cancelled turn, or resend the prompt.
- Do not add client-side cancel-tail retry or delay logic. Investigate any
  remaining off-by-one stop from the correlated wire request and response.
- The TUI completes its owned turn only on the prompt response, or the
  correlated `RequestFailed`, whose request ID equals the ID from
  `SessionPhaseChanged`. Responses for other request IDs never complete it.

### Queue drain

- Only an `EndTurn` response drains the queue: the reducer dequeues exactly
  one prompt and starts it in the same reduction step, so no consumer observes
  an idle gap in which it must decide to drain.
- Any other stop reason leaves the queue intact. The next drain happens after
  a later `EndTurn`; a prompt submitted while `Idle` starts immediately.
- Queued prompts are never restored into the composer or merged into another
  message.
- Loads never drain the queue.
- Goal continuation prompts are submitted only after `EndTurn`, only when the
  runtime is `Idle` with an empty queue, and are hidden from the queue view.
- Summarize-and-swap compaction defers `PromptCompleted` until the session
  swap finishes.

### Remote controller prompts

Remote prompts use `PromptSubmitIfIdle` and must not queue. A remote prompt
received while `phase` is not `Idle` or the queue is non-empty is rejected as
busy (ACP error `-32015`). A remote cancel uses `CancelSubmitFor` and cancels
only when its request ID matches the active prompt, so it never cancels a turn
started by the local TUI. See [remote-control.md](remote-control.md).

### Load

From `Idle`, `session/load` enters `Loading` with its wire request ID. Replay
content and metadata are reduced like live updates, the load response
finalizes buffers and returns to `Idle`, and `LoadCompleted` is emitted.
[Replay](glossary.md#history-operations) is bracketed by
`ReplayStarted`/`ReplayFinished`; the TUI does not classify replayed updates
as unowned activity.

## TUI Projection

- The TUI normalizes raw public ACP traffic for display and uses
  `SessionPhaseChanged` for local lifecycle. It holds no second request FSM.
- `proactive_turn_active` groups unowned output. Unowned content starts it,
  `working` starts it, `idle` completes it, and a local `Prompting` or
  `Loading` start separates statusless unowned output without completing it.
- Follow-only attachments (`_meta.nori.attachmentMode = "follow"` on
  initialize) must never submit prompts.

## Drift Guards

Reject changes that:

- make TUI presentation state an authority for ACP request lifecycle
- complete a client-owned turn from anything other than its prompt response,
  except the forced-cancel timeout and child exit
- resend a prompt or retry around an empty `EndTurn` after cancel
- let open buffers survive request completion or infer tool ownership from
  timing
- treat cancel as immediate idle
- queue remote prompts, or let a remote cancel affect a local turn
- reduce inbound ACP traffic for a session out of order

Tests that enforce these rules:

| Rule | Tests |
| --- | --- |
| Phase, cancel, queue drain, ownership, warnings, assembly | `nori-rs/harness/src/backend/session_reducer/tests.rs` |
| Ordered inbox; prompt after cancel gets its own response | `nori-rs/acp-host/src/connection/acp_connection_tests.rs` (`test_event_receiver_preserves_update_then_approval_order`, `test_sequential_prompt_after_cancel_receives_response`) |
| Prompt echo order and suppression | `nori-rs/harness/tests/session_event_boundary.rs` |
| Observer `working`/`idle` brackets; remote busy and cancel scoping | `nori-rs/harness/tests/remote_host.rs`, `nori-rs/acp-host/tests/remote_ws.rs` |
| TUI correlation and agent-owned turn presentation | `nori-rs/tui/src/chatwidget/tests/part10.rs` |
| End-to-end cancel and queued prompt during cancelling | `nori-rs/tui-pty-e2e/tests/streaming.rs` |
