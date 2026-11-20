# Turn Lifecycle

How Nori starts, routes, and ends ACP prompt turns, and how follow-ups and
the prompt queue behave around them. Terms follow the [glossary](glossary.md).
Remote transports are covered in [remote control](remote-control.md).

## ACP Ground Truth

Nori follows ACP v1 (https://agentclientprotocol.com/protocol/v1/prompt-turn):

- A prompt turn starts with one `session/prompt` request and ends only when
  that request's response arrives with a `stopReason`.
- `session/cancel` is a notification. It does not end the turn: the agent may
  keep sending `session/update` until it answers the original prompt with
  `cancelled`, and the client must accept those updates.
- `session/load` replays history through `session/update` and finishes when
  the load response arrives.
- Chunk updates carry no reliable message identity, so message assembly stays
  local to the active request.
- ACP v1 has no agent-initiated turn and no steering or queue mechanism.
  Nori's agent-turn metadata, steering, and queue are extensions.

## Ownership

- The **session harness** owns ACP request lifecycle. The TUI, remote
  controllers, and observers derive their view from its public event stream.
  None of them may open, complete, or cancel a request on their own authority.
- A turn is client-owned only on the connection that sent its prompt. That
  ownership never grants cancel or permission rights on another connection.
- Never invent a request ID, and never attribute an unowned update to an
  earlier or later local request.

## Runtime State

Each ACP session has one `SessionRuntime`
(`nori-rs/harness/src/normalized/session_runtime.rs`):

| Field | Meaning |
| --- | --- |
| `phase` | `Idle`, `Loading { request_id }`, or `Prompt { request_id, cancelling }`. The only source of truth for whether a local request is active. |
| `active` | Open message buffers, the request's tool call IDs, pending permission request IDs, and the originating queued prompt. Present only while `phase` is not `Idle`. |
| `persisted` | Transcript, plan, tool snapshots, commands, mode, config options, session info, and usage. Survives request boundaries. |
| `queue` | Harness-owned FIFO of prompts not yet sent. The agent never sees it. |
| `queue_pause` | Set when the queue is paused after an interrupt or error. |
| `steer_fence` | `Open`, or held while a steer's reply is unsettled. |
| `observer_turn_active` | Set by `working`/`idle` agent-turn metadata while no local request is active. |
| `orphan_update_warning_emitted` | Limits the unowned-update warning to once per burst. |

The public projection is `nori_protocol::SessionPhase` (`Idle`,
`Loading { request_id }`, `Prompting { request_id }`,
`Cancelling { request_id }`), published as `SessionPhaseChanged`. The
`request_id` is the ACP wire request ID.

## Serialized Reducer

All traffic that affects `SessionRuntime` flows through `reduce()` in
`nori-rs/harness/src/backend/session_reducer.rs`, driven by one runtime task
that reads one channel. The one exception is resume bootstrap: the
`session/load` collector reduces the replay before the runtime task starts.
The channel carries:

- `session/update` notifications, `session/request_permission` requests,
  prompt and load responses, transport failures, local submits, cancels,
  follow-ups, and queue actions are reduced one at a time in arrival order.
- The ACP host publishes every notification, request, and response onto one
  ordered connection inbox. Do not split public ACP traffic into racing
  channels.
- The relay `select!` is `biased` toward that inbox. The prompt task publishes
  the raw response before it reports the stop reason, so every update sent
  before the response is reduced first. On `end_turn` the prompt task also
  waits for in-flight permission answers.
- Reducer output is side effects (`SendPrompt`, `SendCancel`,
  `ResolvePermissionCancelled`, `RejectPromptBusy`) that the driver runs after
  reduction.

Child exit is the one terminal path that does not pass through the reducer
(see [How a turn ends](#how-a-turn-ends)).

## `session/update` Routing

### Session metadata

`available_commands_update`, `current_mode_update`, `config_option_update`,
`session_info_update`, and `usage_update` patch `persisted` in any phase.
They are never turn boundaries.

### Request-scoped content

`user_message_chunk`, `agent_message_chunk`, `agent_thought_chunk`, `plan`,
`tool_call`, and `tool_call_update`:

- With `active` set, a text chunk appends to the open buffer of its kind. A
  chunk of another kind first flushes the other open buffers into the
  transcript, which keeps interleaved order. Non-text chunks are not
  assembled.
- Open buffers are finalized into the transcript when the request completes.
  They never survive completion and are never assembled across `Idle`.
- Without `active`, the update is **unowned**. The harness still normalizes
  and publishes it, does not assemble it, and emits the notice
  `Received update with no active local request` once per burst. The burst
  resets when a local prompt or load starts or on `idle` agent-turn metadata.
  The notice is suppressed during an agent-owned turn.

### Tool ownership

Tool snapshots are keyed by `toolCallId`. Each update stamps the snapshot's
`owner_request_id` with the local request active when that update arrived, or
`None` when no request is active. Ownership is never inferred from timing
across requests: a tool updated after its request ended becomes ownerless.

### Agent-owned turns

A `session_info_update` with `_meta.nori.status`:

- exact `working` starts an agent-owned turn when no local prompt or load is
  active; exact `idle` ends it. Both are ignored while a local request is
  active.
- any other value is ordinary session metadata.
- The TUI hides a status-only frame. A frame that also carries `title` or
  `updated_at` keeps those fields visible.
- Agent ownership grants no queue, cancel, loop, or permission controls. The
  TUI does not send cancel for an agent-owned turn.

### Permission requests

- `session/request_permission` is valid only in the `Prompt` phase, including
  while cancelling. Its ID is recorded in `active`.
- Outside a prompt, the harness answers with the `cancelled` outcome and
  publishes a warning. During bootstrap and `session/load` it answers
  `cancelled` without a warning.
- Under approval policy `Never`, the harness picks the first allow option, or
  answers `cancelled` when there is none.

## Starting a Turn

- `PromptSubmit` starts a prompt from `Idle` when the steer fence is open.
  Otherwise the prompt is appended to `queue` and `QueueChanged` is published.
- `PromptSubmitIfIdle` starts a prompt only when the session is `Idle`, the
  queue is empty or paused, and no steer is unsettled. Otherwise it is
  rejected as busy. A prompt admitted over a paused queue runs ahead of the
  held entries and leaves the queue paused.
- Each prompt is sent as exactly one `session/prompt`. The harness uses a
  provisional ID until the transport assigns the wire ID, then publishes
  `SessionPhaseChanged(Prompting)`. Consumers correlate on the wire ID only.

### Public turn brackets

Remote and cloud observers do not receive the initiator's prompt response, so
the harness brackets every local turn on its public stream, in order:

1. `SessionPhaseChanged(Prompting { request_id })`
2. a status-only `working` frame
3. for user prompts, the caller's prompt content as `user_message_chunk`
   updates with `messageId` set to the echo ID and
   `_meta["nori.dev/promptEchoId"]`
4. agent updates and the raw prompt response
5. `SessionPhaseChanged(Idle)`, then a status-only `idle` frame

Transport events wait on the prompt phase gate until step 3 is published, so
no agent output precedes the bracket. Between a completed turn and a drained
queued turn, consumers see `Idle` and the `idle` frame before the next
`Prompting`.

When a downstream agent echoes the active prompt back as a
`user_message_chunk` whose `promptEchoId` matches and whose content is part of
the sent prompt, the harness drops that echo so the prompt renders once. A
user message without a matching echo ID is never dropped.

## How a Turn Ends

The prompt response is the normal boundary. The table lists every terminal
path and what clients observe.

| Cause | Harness | Public stream | Queue |
| --- | --- | --- | --- |
| Response with `end_turn` | Finalize, clear `active`, `Idle` | Raw ACP `Response`, `Idle`, `idle` frame | Drains one prompt |
| Response with another stop reason (including `cancelled`) | Same | Same | Pauses |
| Transport or agent error | Completes as `Cancelled` with a failure | `RequestFailed` (retryable or fatal), `Idle`, `idle` frame | Pauses |
| Forced cancel after 10 s | Aborts the prompt task, completes as `Cancelled` | Notice at 3 s, then `Idle` and `idle` frame. No `Response` and no `RequestFailed` | Pauses |
| Agent process exit | Prompt task aborted; reducer does not complete | `RequestFailed` (fatal) for the active wire ID, then `SessionEnded(ConnectionLost)`. No `Idle` and no `idle` frame | Not reduced |

Current behavior to account for:

- A forced cancel publishes no prompt outcome. A client that completes its
  turn only on `Response` or `RequestFailed` (the TUI, the remote host) does
  not see that turn finish.
- Child exit publishes no `Idle`. Treat `SessionEnded` as the end of the
  session. The message is `Agent process exited unexpectedly (exit code N).`,
  or the same text without the code when none is available, plus the stderr
  tail.
- `PromptCompleted` and `LoadCompleted` are harness-private. Never depend on
  them outside the harness.
- Summarize-and-swap compaction delays the private completion and the `idle`
  frame until the session swap finishes.

Boundary rules:

- One prompt is one `session/prompt`. Its response, whatever its content, is
  its terminal response.
- A successful empty `EndTurn` is a valid terminal response. Never absorb it
  into a previous cancelled turn or resend the prompt.
- Do not add client-side cancel-tail retry or delay logic.
- The TUI completes its owned turn only on the raw `Response` or the
  correlated `RequestFailed` whose request ID matches `SessionPhaseChanged`.
  A raw error `Response` pairs with its `RequestFailed`; responses for other
  IDs never complete the turn.

## Cancellation

From `Prompt { cancelling: false }` the reducer:

- sets `cancelling` and publishes `SessionPhaseChanged(Cancelling)`
- marks unfinished tools owned by the request as failed in harness state (ACP
  v1 has no `cancelled` tool status); no tool event is published for this
- answers every pending permission request with the `cancelled` outcome
- sends `session/cancel`, keeps `active`, and starts the cancel watchdog

A second cancel is a no-op. Cancel never moves the session to `Idle` by
itself. If the agent does not answer, the harness publishes
`Agent is slow to cancel. Will force-cancel in 7s…` after 3 s and forces the
turn to end after 10 s.

A remote cancel names its request ID and cancels only when that ID matches
the active prompt, so it never cancels a turn started by the local TUI.

## Error Categories

`AcpErrorCategory` in `nori-rs/acp-host/src/error_category.rs` classifies a
failed user prompt. The category picks the user message and the
`RequestFailed` kind.

| Category | Trigger | Kind |
| --- | --- | --- |
| `ApiServerError` | Provider 5xx | retryable |
| `QuotaExceeded` | Rate limit or usage quota | retryable |
| `AgentUnreachable` | ACP `-32010` | retryable |
| `AgentBusy` | ACP `-32011`: the agent is still finishing a turn from an earlier connection | retryable |
| `Authentication` | Auth required or failed | fatal |
| `ExecutableNotFound` | Agent binary missing | fatal |
| `Initialization` | Agent failed to start | fatal |
| `PromptTooLong` | Context window exceeded | fatal |
| `SessionNotFound` | ACP `-32002` | fatal |
| `SessionNotResumable` | ACP `-32012` | fatal |
| `NoActiveSession` | ACP `-32014` | fatal |
| `SessionAlreadyActive` | ACP `-32015` | fatal |
| `Unknown` | Anything else | fatal |

- Only user prompts are categorized. A failed compaction (`Compact failed: …`)
  or goal continuation (`Goal continuation failed: …`) is always fatal.
- An agent-supplied `error.data.detail` is appended in parentheses.
- In the TUI, a fatal failure stops `/loop`; a retryable failure leaves it
  armed so the next iteration retries. A failure shows its own error cell
  instead of the generic "Conversation interrupted" notice.
- Child exit is always fatal.

## Remote Prompts

Remote prompts (`session/prompt` from a remote-control client, or
`_nori/session/prompt` from a host controller) use `PromptSubmitIfIdle` and
never queue. They are rejected as busy with `-32015` when the session is not
`Idle`, when the queue holds entries and is not paused, or while a steer is
unsettled. A paused queue does not make the session busy: a remote prompt
runs ahead of it and leaves it paused. The controller's
`_nori/session/follow_up` is the deliberate way to queue remote input (see
below).

## Follow-ups

A follow-up is input sent while a turn is running, either **steered** into
that turn or **queued** for the next one. No config flag gates follow-ups;
steering depends only on the agent capability check below.

### Keys

In direct `nori` and the `nori attach` controller, with a composed draft and a
running turn:

| Key | Effect |
| --- | --- |
| Enter | Steer into the running turn. |
| Tab | Queue for the next turn. |

- In direct `nori`, slash commands, `!` shell commands, and bare
  `exit`/`quit` drafts keep their normal behavior and are never sent as
  follow-ups; Tab falls back to its ordinary meaning for them.
- In `nori attach`, only the attach commands (`/quit`, `/exit`, `/detach`,
  `/sessions`, `/new`, `/delegate`, `/close`, `/cancel`, `/resume`) are
  handled locally. Enter sends any other draft, including `!` text and other
  slash text, as a steer follow-up. Tab does not queue slash or `!` drafts,
  but it does queue a bare `exit` or `quit`.
- In direct `nori`, the running turn must be in `Prompting`. While the turn is
  cancelling, or in cloud mode, Enter submits an ordinary prompt (queued if
  busy) and Tab does not queue. A follow-only attachment keeps the draft and
  submits nothing.
- In `nori attach`, Tab first tries `/new` path completion.
- When idle, Enter submits immediately. Direct `nori` uses
  `PromptSubmitIfIdle` and shows `Could not submit …` on rejection;
  `nori attach` sends `_nori/session/prompt` and shows the host's error.

### Wire path

1. The client sends the follow-up. Direct `nori` calls the in-process harness
   with sender `local`. An attached controller sends
   `_nori/session/follow_up` to the host with `session_id`, `expected_turn`,
   `message_id`, `intent` (`steer` or `queue`), and `prompt`.
2. For a steer, the harness sends the ACP extension request
   `_session/steering` to the agent with
   `_meta.steering.idleBehavior = "promptRequired"`.
3. The caller gets a delivery receipt. Queued and fallback input enters the
   ordinary harness queue. Injected input is published as a user message with
   the echo ID; the original prompt keeps ownership of output, completion,
   usage, and cancellation.

The host request format and transports are in the
[host follow-up contract](../nori-rs/cli/src/host/docs.md#mid-turn-follow-ups).

### Steering capability

The harness steers only when the live initialize result has
`protocolVersion == 1`, `_meta.steering.supported == true`, and an exact
`agentInfo` match:

| `agentInfo.name` | Versions |
| --- | --- |
| `@agentclientprotocol/claude-agent-acp` | 0.102.0, 0.108.1, 0.109.0 |
| `@agentclientprotocol/codex-acp` | 2.7.0 |

Any other agent or version gets `unsupported_queued`: the input is queued for
the next turn instead. Default Claude and Codex launches resolve `@latest`
(`nori-rs/acp-host/src/registry.rs`), so an adapter release can change the
result without a Nori release.

### Receipts

| Status | Meaning |
| --- | --- |
| `injected` | The agent accepted the input into the running turn. |
| `queued` | Explicit `queue` intent admitted to the next turn. |
| `unsupported_queued` | Steering unavailable (guard mismatch or agent `-32601`); queued instead. |
| `idle_queued` | The session is `Idle` and the expected turn already ended with `end_turn`, or the agent answered outcome `promptRequired` with reason `noRunningTurn`; queued as the next prompt. |
| `rejected` | Refused: wrong turn, steer while cancelling, another steer unsettled, the expected turn ended by cancel or error, or an agent refusal. |
| `unknown` | Delivery not confirmed. Keep the message ID for reconciliation. |

- A retry with the same `message_id` and an identical submission returns the
  stored receipt. Any difference (content, intent, expected turn, sender, or
  origin) under the same ID is an error.
- The TUI reports each outcome as a notice. Direct `nori` does not restore
  the draft on `rejected` or `unknown`. `nori attach` puts the draft back in
  an empty composer on a `rejected` receipt or an error response, but not on
  `unknown`.

### Steer timeout

The harness waits 5 s for the agent to answer `_session/steering`. On
timeout the receipt is `unknown` and the steer fence is raised until the late
reply settles. While the fence is up, `PromptSubmitIfIdle` is rejected as
busy, an idle `PromptSubmit` is queued, and queue draining waits for the
reply. Other `unknown` causes (a transport error or an unmapped result) do
not raise the fence.

## Prompt Queue

The harness queue holds prompts that have not been sent. Entries are never
merged into another message or restored into the composer automatically.

### Drain and pause

- A response with `end_turn` dequeues and starts exactly one prompt, unless
  the queue is paused. If a steer is unsettled, the drain waits for it.
- Any other ending pauses the queue when it holds at least one user prompt.
  Entries stay in order and nothing is sent. The reason is `interrupted` for
  `cancelled` or a failure while cancelling, and `turn_failed` otherwise. It
  travels as `QueueSnapshot.paused`.
- A queue holding only hidden work (a queued `/compact` or goal continuation)
  does not pause.
- A paused queue sends nothing on its own, including a drain deferred behind
  a steer.
- Loads never drain the queue, and a resume during a load is ignored.

### Resume

- Enter on an empty composer resumes a paused queue. An idle session starts
  the next entry at once (after any unsettled steer); a busy session drains
  after its running turn ends with `end_turn`.
- After a resume the queue drains one prompt per completed turn again. A
  later interrupt or error pauses what is left.
- Taking back the last held user prompt clears the pause.
- Resume and discard are possible only from the local TUI that owns the
  harness. No other client can edit, resume, or discard the harness queue.
  `/sessions` subscribers receive `QueueChanged` snapshots, `/acp` drops
  them, and `nori attach` does not render them.

### Recall, edit, and delete

Only text-only prompts typed in the local composer are editable. Delegation
results, remote and host follow-ups, and prompts with images stay queued and
cannot be taken back.

| Situation | Key | Effect |
| --- | --- | --- |
| Empty composer, editable entries | Up | Select the newest editable entry. |
| Entry selected | k / Up | Move to the older entry; stops at the oldest. |
| Entry selected | j / Down | Move to the newer entry; past the newest, focus returns to the composer. |
| Entry selected | Enter / e | Take the entry back into the composer to edit. |
| Entry selected | Delete / x | Remove the entry; the list stays open on a neighbor. |
| Entry selected | Esc | Leave the list. |
| Entry selected | Any other key, or a key with modifiers | Leave the list and pass the key to the composer. |

- Take-back is a harness reclaim by the entry's event ID on the reducer path,
  so an entry is removed exactly once or has already started and stays
  delivered. The TUI reports an entry that already started instead of sending
  it twice.
- If the selected entry starts or leaves the queue, the selection clears.
  Enter, e, Delete, and x then do nothing (Enter does not resume); press j or
  k to select again.
- A taken-back draft is resubmitted with whichever key you press: Enter
  steers, Tab queues.

### Local queue vs `/managed` queue

| | Local (TUI, harness, `nori host` `/sessions`) | `/managed` relay (`[host] managed = true`) |
| --- | --- | --- |
| Owner | Session harness reducer | Per-session FIFO in `nori-rs/harness/src/managed_acp/follow_ups.rs` |
| Pause after interrupt or error | Yes, visible in `QueueSnapshot.paused` | No pause state |
| After a non-`end_turn` ending | Held until explicit resume | Held; drains after the next turn that ends with `end_turn` |
| Drain condition | `end_turn`, not paused, steer settled | `end_turn` and no pending permissions |
| New `session/prompt` while items are held | Admitted ahead of a paused queue | Refused with `-32015` (`session has pending follow-up delivery`) while a steer is in flight, or while items are queued and either ready to drain or waiting on a permission. Items held after a non-`end_turn` ending do not block it |
| Edit or remove entries | Local TUI only | Not supported |
| Started-item notifications | Ordinary session events | `_nori/session/follow_up/submitted`, `/started`, `/finished` |

## Delegated Child Results

Delegation requires `[features] multi_session = true` (default false). When a
child session finishes, its result returns to the parent as an ordinary
queued prompt:

- The prompt starts with `[Nori delegation result] from <agent>` and names the
  child session.
- It is submitted with `PromptSubmit`, so it starts at once when the parent is
  idle and waits in the parent queue when the parent is busy.
- It is not editable and waits behind a paused queue like any other entry.
- The harness retains the delivery until the parent's correlated prompt
  response arrives.

See [remote control](remote-control.md) for delegation controls.

## Load

`session/load` runs during resume bootstrap from `Idle`. The harness publishes
`SessionPhaseChanged(Loading)`, the setup events including the load response,
then `Idle`. Replayed updates follow as a `ReplayStarted`/`ReplayFinished`
batch, and the TUI does not treat them as unowned activity.

A `session/load` refused with `-32011` (`AgentBusy`) means the original turn
is still running on the agent. The harness ends the resume with the
agent's error and does not fall back to transcript replay, since a
new session would fork the conversation. Any other load failure falls back to
replay when a transcript is available. Recover by relaunching the CLI after
the turn ends; see [transcripts](transcripts.md#busy-agent-on-load).

## TUI Projection

- The TUI renders raw public ACP traffic and uses `SessionPhaseChanged` for
  local lifecycle. It holds no second request state machine.
- `proactive_turn_active` groups unowned output: unowned content or `working`
  starts it, `idle` completes it, and a local `Prompting` or `Loading` start
  separates statusless unowned output without completing it.
- A follow-only attachment never submits prompts or follow-ups.

## Drift Guards

Reject changes that:

- make TUI presentation state an authority for ACP request lifecycle
- complete a client-owned turn from anything but its prompt response or
  correlated `RequestFailed`, other than the forced-cancel timeout and child
  exit
- resend a prompt or retry around an empty `EndTurn` after cancel
- let open buffers survive request completion, or infer tool ownership from
  timing
- treat cancel as immediate idle
- start queued prompts after a turn that did not end with `end_turn` without
  an explicit resume
- queue a remote `session/prompt`, or let a remote cancel affect a local turn
- reduce inbound ACP traffic for a session out of order

| Rule | Tests |
| --- | --- |
| Phase, cancel, drain, ownership, warnings, assembly | `nori-rs/harness/src/backend/session_reducer/tests.rs` |
| Ordered inbox; prompt after cancel gets its own response | `nori-rs/acp-host/src/connection/acp_connection_tests.rs` |
| Prompt echo order and suppression; child exit | `nori-rs/harness/tests/session_event_boundary.rs` |
| Observer brackets; remote busy and cancel scoping | `nori-rs/harness/tests/remote_host.rs`, `nori-rs/acp-host/tests/remote_ws.rs` |
| Follow-ups, steering guard, steer timeout | `nori-rs/harness/tests/follow_up.rs`, `nori-rs/harness/src/managed_acp/follow_up_tests.rs` |
| Queue pause, resume, and reclaim | `nori-rs/harness/tests/queue_pause.rs`, `nori-rs/harness/tests/queue_reclaim.rs` |
| Delegated results in a busy parent | `nori-rs/harness/tests/delegation.rs` |
| TUI correlation and agent-owned turns | `nori-rs/tui/src/chatwidget/tests/part10.rs` |
| End to end: cancel, queue keys, pause, attach | `nori-rs/tui-pty-e2e/tests/streaming.rs`, `nori-rs/tui-pty-e2e/tests/queue_recall.rs`, `nori-rs/tui-pty-e2e/tests/queue_pause.rs`, `nori-rs/tui-pty-e2e/tests/host_attach.rs` |
