# Agent Turn State — Design Spec

## The ACP model

The ACP protocol defines a single primitive for conversation flow: the **prompt turn**.

```
Client ──session/prompt──▶ Agent
                             │
                             ├─ session/update (streaming content, tool calls, …)
                             ├─ session/update
                             ├─ …
                             │
Client ◀──prompt response──  Agent  (stopReason: end_turn | cancelled | …)
```

A prompt turn begins when the client sends `session/prompt`. It ends when the agent responds to that request. Between those two points, the agent may stream any number of `session/update` notifications. The client may send `session/cancel` at any point during the turn; the agent must eventually respond to the original `session/prompt` with `stopReason: "cancelled"`.

There is exactly **one turn in flight at a time** per session. A new `session/prompt` may only be sent after the previous prompt response has been received.

## The state we need

From the TUI's perspective, the agent is always in one of three states:

```
         submit            prompt response
  Idle ────────▶ Working ────────────────▶ Idle
                   │                         ▲
                   │  session/cancel          │
                   ▼                         │
               Cancelling ──────────────────┘
                          prompt response
```

### `Idle`

No turn is in flight. The user may compose and submit a message.

- The input area is editable.
- No spinner. No "esc to interrupt" hint.

### `Working`

A turn is in flight. The agent is streaming content.

- The input area shows a read-only queue (or is hidden).
- A spinner and "esc to interrupt" hint are visible.
- `session/update` notifications are rendered as they arrive.

### `Cancelling`

The user pressed Escape. A `session/cancel` notification has been sent, but the prompt response has not yet arrived.

- The spinner changes to indicate cancellation is in progress.
- The input area remains locked — **no new turn may begin until the current prompt response arrives**.
- `session/update` notifications may still arrive (the spec permits this) and should be rendered.

## Transitions

| From | Event | To | Side effects |
|------|-------|----|--------------|
| Idle | User submits message | Working | Send `session/prompt`. Show spinner. |
| Working | `session/update` | Working | Render the update (message chunk, tool call, etc.). |
| Working | Prompt response (`end_turn`) | Idle | Hide spinner. Show the completed turn. Unlock input. |
| Working | Prompt response (`cancelled`) | Idle | Hide spinner. Show interruption notice. Unlock input. |
| Working | User presses Escape | Cancelling | Send `session/cancel`. Change spinner to cancelling indicator. |
| Cancelling | `session/update` | Cancelling | Render the update. |
| Cancelling | Prompt response (`cancelled`) | Idle | Hide spinner. Show interruption notice. Unlock input. |
| Cancelling | Prompt response (`end_turn`) | Idle | Treat as a completed turn (agent finished before cancel took effect). |

## Invariants

1. **One turn at a time.** A new `session/prompt` is never sent while a previous prompt response is pending. This is not a performance optimization — it is a protocol requirement.

2. **The prompt response is the single source of truth for turn completion.** Neither `session/cancel` nor `TurnLifecycle::Aborted` ends a turn. Only the prompt response does. Until it arrives, the turn is still in flight.

3. **Cancelling is not Idle.** Pressing Escape does not immediately free the input. The TUI remains in a "waiting for cancellation acknowledgment" state until the prompt response arrives. This prevents the user from submitting a new message before the agent has confirmed the cancellation.

4. **No client-side event fabrication.** The TUI does not synthesize `TurnLifecycle::Completed` or any other turn-boundary event. Turn boundaries come exclusively from the prompt response.

## What this means for the input area

When the user presses Escape:

- The TUI enters `Cancelling`.
- If the user types a message during `Cancelling`, it is buffered.
- When the prompt response arrives (transitioning to `Idle`), the buffered message is submitted as the next turn.

This eliminates the race entirely. There is no window where a new `session/prompt` can collide with a pending cancellation, because the new prompt is only sent after the old one's response has been received.

## Message queueing

The user may type and press Enter at any time, regardless of the current state. Messages submitted while not `Idle` are **queued**, not dropped.

```
Queue: [ ]                              state = Idle

User submits "fix the bug"
  → send session/prompt immediately
Queue: [ ]                              state = Working

User submits "also update the tests"
Queue: [ "also update the tests" ]      state = Working (message queued)

User presses Escape
  → send session/cancel
Queue: [ "also update the tests" ]      state = Cancelling (message preserved)

Prompt response arrives (cancelled)
  → pop queue, send session/prompt "also update the tests"
Queue: [ ]                              state = Working
```

Rules:

1. **Idle → submit:** Send `session/prompt` immediately. Queue stays empty.
2. **Working → submit:** Append to queue. Do not send anything to the agent.
3. **Cancelling → submit:** Append to queue. Do not send anything to the agent.
4. **Any → Idle (prompt response arrives):** If the queue is non-empty, pop the front message and submit it (transition back to Working). If empty, stay Idle.

The queue is a simple FIFO of user messages. It has no protocol significance — the agent never knows about it. From the agent's perspective, each turn is a single `session/prompt` → response cycle.

If the user submits multiple messages during a single turn, they accumulate. When the turn ends, the first queued message is submitted as the next turn. The rest remain queued. This matches user intent: each message becomes its own turn, in order.

## Inter-turn session updates

The ACP spec scopes `session/update` notifications to the prompt turn lifecycle: they are sent between the `session/prompt` request and the prompt response. However, agents may also send `session/update` notifications **outside** a turn — for example, `available_commands_update`, `usage_update`, `session_info_update`, or `config_option_update`. The Claude Code agent specifically sends `available_commands_update` and `usage_update` during session creation, before any prompt is sent.

Additionally, some agents run background tasks (e.g. terminal commands with `run_in_background: true`) whose results may arrive after the turn that launched them has already completed.

### Update classification

Updates split into three categories by their relationship to turn state:

**Metadata updates** — session-level, accepted in any state, never affect turn state:

- `usage_update` — update the status bar
- `available_commands_update` — update the command palette
- `session_info_update` — update the session title
- `config_option_update` — update configuration
- `current_mode_update` — update the mode indicator

**Attributed content updates** — carry a `toolCallId` that links back to a previously-rendered widget:

- `tool_call_update` with a `toolCallId` the client has already seen

**Unattributed content updates** — no stable identifier linking them to prior output:

- `agent_message_chunk`
- `agent_thought_chunk`
- `tool_call` (initial creation of a new tool call)
- `plan`
- `user_message_chunk`

### Handling by state

| Category | Idle | Working / Cancelling |
|---|---|---|
| Metadata | Accept. Update session-level UI. | Accept. Update session-level UI. |
| Attributed (`tool_call_update` for known ID) | **Accept. Update the existing widget in place.** | Accept. Update the existing widget in place. |
| Unattributed content | **Stash.** Hold until the next turn begins, then prepend. | Render in the current turn's output area. |

The key insight for attributed updates: a `tool_call_update` carries a `toolCallId` that the client already rendered during a previous turn. The update is completing or progressing something the user can already see. This does not require an active turn — the client simply updates the existing tool call widget (e.g. from `in_progress` to `completed`, appending output). No turn state changes. No spinner. The widget was already there.

For unattributed content (bare `agent_message_chunk`s with no tool call context): the stdio stream provides a total ordering. Every message arriving after a prompt response but before the next `session/prompt` is, by definition, inter-turn. The client knows the boundary — it is the prompt response itself. No timestamps or sequence numbers needed.

This means unattributed inter-turn content can be **appended to the previous turn's output area**. It arrived on the wire after the prompt response, but it is narration from work that turn initiated (e.g. a background task completing). The user sees it appear below the completed turn, which is exactly where it belongs. No turn boundary is fabricated; the content is simply late-arriving output from the turn that started the work.

During Working or Cancelling, a new turn is active and the content renders there as normal. The distinction only matters for Idle, where the previous turn's output area is the correct anchor.

## Permission requests during cancellation

The ACP spec requires:

> The Client **MUST** respond to all pending `session/request_permission` requests with the `cancelled` outcome.

When the TUI transitions to `Cancelling`:

1. All pending `session/request_permission` dialogs are immediately dismissed.
2. The TUI responds to each with `{ outcome: "cancelled" }`.
3. No new permission dialogs are shown while in `Cancelling` state.

If a `session/request_permission` arrives during `Cancelling` (the agent sent it before receiving the cancel), the TUI responds immediately with `cancelled` without showing a dialog.

## Tool call cleanup on cancel

The ACP spec requires:

> The Client **SHOULD** preemptively mark all non-finished tool calls pertaining to the current turn as `cancelled` as soon as it sends the `session/cancel` notification.

When the TUI transitions to `Cancelling`, all rendered tool calls that are still `pending` or `in_progress` should be visually marked as cancelled. The agent may still send `tool_call_update` notifications with final status before the prompt response; these should be rendered normally (updating the visual state from cancelled to completed if the tool finished before the cancel took effect).

## Stop reasons

The prompt response `stopReason` determines the transition out of Working / Cancelling:

| stopReason | Meaning | TUI behavior |
|---|---|---|
| `end_turn` | Agent finished normally. | Show completed turn. Transition to Idle. |
| `cancelled` | Agent confirmed the cancellation. | Show interruption notice. Transition to Idle. |
| `max_tokens` | Context window exhausted. | Show warning. Transition to Idle. |
| `max_turn_requests` | Agent hit its request limit. | Show warning. Transition to Idle. |
| `refusal` | Agent refuses to continue. | Show refusal notice. Transition to Idle. |

All stop reasons transition to Idle. The only difference is the message shown to the user. The queue-drain behavior (auto-submit next queued message) applies equally to all stop reasons.

## Error responses

If the agent returns a JSON-RPC error instead of a prompt response, the turn is still over. The TUI should:

1. Display the error to the user.
2. Transition to Idle.
3. Drain the queue as normal.

An error response is not a valid ACP stop reason, but it is a definitive end to the JSON-RPC request-response cycle. The turn is complete.
