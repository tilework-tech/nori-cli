# Turns, the queue, and delegation

This spec states how a session runs turns, queues and steers messages,
pursues goals, compacts its context, delegates work, and forks or rewinds.
It refines the sections "Turns, the queue, and steering" and "Delegation" of
the nori-monorepo product spec (`docs/specs/nori-cli/product-spec.md`) with
the exact CLI behaviour. Terms follow [GLOSSARY.md](../GLOSSARY.md) and the
nori-monorepo `GLOSSARY.md`.

The session runtime owns all of this behaviour. Every surface (the TUI,
`nori exec`, `nori host`, the managed host) gets the same rules. Wire shapes
for `_nori/*` methods are in nori-monorepo `docs/acp-extensions.md`. The
broker's own queue and steering rules are in nori-sessions
`docs/specs/conversation-lifecycle.md`.

## Turns

- A session runs one request at a time: a prompt turn or a load. Its phase
  is idle, prompt (with a cancelling flag), or loading.
- A turn starts when Nori sends `session/prompt` and ends with its response.
- A turn **ends normally** when the stop reason is `end_turn`.
- A turn **ends early** on any other stop reason (`cancelled`, `max_tokens`,
  `max_turn_requests`, `refusal`). A turn **fails** when `session/prompt`
  returns an error.
- **Cancel** sends `session/cancel`. Nori answers every pending permission
  request of the turn as cancelled and marks its unfinished tool calls as
  failed. A second cancel does nothing. In the TUI, Esc cancels the running
  turn.
- A permission request that arrives when no turn runs is answered as
  cancelled.
- On `nori host`, a `session/prompt` sent while a turn runs gets `-32015`.
  Clients send messages for a running turn as follow-ups (below).

## The prompt queue

### What enters the queue

- A prompt submitted while the session is busy waits in the prompt queue.
- Nori's own prompts wait in the same queue, hidden from clients: the
  compaction prompt and goal continuations.
- Clients see only the user's prompts, oldest first. Each entry has a stable
  ID, its text, and whether it can be edited (plain text only).
- A child session's result enters the parent's queue as a prompt
  (see [Delegation](#delegation)).

### Sending

- The queue sends one prompt per turn, in order.
- After a turn that ends normally, the next queued prompt starts.
- While a steer is still unsettled, the queue waits for it before it sends
  the next prompt.

### Pause

| Event | Queue result |
|---|---|
| The user cancels the turn | Pauses with reason **interrupted** |
| The turn fails | Pauses with reason **turn failed** |
| The turn ends early (any stop reason except `end_turn`) | Pauses with reason **turn failed** |
| Any of the above, but the queue holds no user prompt | Does not pause |

While the queue is paused:

- It keeps every entry in order and sends nothing on its own.
- A new prompt sent while the session is idle runs at once. The queue stays
  paused after that turn.
- **Resume the queue** sends the next queued prompt if the session is idle.
  If a turn is running, the queue sends after that turn ends normally. While
  a session loads, resume does nothing and the pause stays.
- After a resume, the queue sends one prompt per completed turn again. A
  later cancel or failure pauses what is left.
- Reclaiming the last user prompt clears the pause.
- **Discard** drops every queued user prompt, including ones that cannot be
  edited, and clears the pause. None of them is sent.

In the TUI, Enter on an empty composer resumes a paused queue.

### The queue on the host

The queue lives on the host, so a terminal that reconnects, or a second
terminal on the same host, finds it as it was.

- Every client sees the queue in `queue_changed` session events and on each
  `_nori/session/list` row (`queue`: entries and pause reason). The row keeps
  the latest snapshot after its event has left the replay window.
- Only the controller changes the queue, with `_nori/session/queue/resume`,
  `_nori/session/queue/take` and `_nori/session/queue/discard`. Watchers get
  `-32016`. The host advertises them as `capabilities.nori_sessions.queue`.
- `take` returns a prompt's text to exactly one caller. Each call carries a
  `requestKey` the caller chose. The same key gets the same answer again, so
  a terminal whose reply was lost to a disconnect asks again with it after
  reconnecting. Any other key for that entry, or a take of a prompt that
  already started, returns `{"text": null}`. The host remembers the latest
  256 answers.
- Only prompts a terminal's own user typed are editable: those from plain
  `nori`'s own pipe, and `_nori/session/follow_up` calls from a client that
  declared `capabilities.nori_sessions.composer` at `initialize`, as the
  shared TUI does. Any controller terminal may take them back. A follow-up
  from a client that does not declare it, such as the Sessions broker or an
  older client, can be resumed or discarded, not taken into a composer.
- The plain TUI and `nori attach` use these methods for any session on a
  host. `nori attach` on a host without `queue` sends no queue calls.

### Reclaim

- Reclaim removes one queued prompt by its ID, before it is sent. Identical
  texts never collide.
- A prompt that has already started cannot be reclaimed. The client is told
  that it started; it is never sent twice.
- A reclaimed prompt gives back the one-shot context it took when it was
  queued (hook context and a pending compaction summary), so the next prompt
  still carries it.

TUI keys for the queue list above the composer:

| Key | Action |
|---|---|
| Up (empty composer) | Focus the list on its newest editable prompt |
| j / k, Down / Up | Move between editable prompts. Down from the newest returns to the composer |
| Enter or e | Reclaim the prompt into the composer to edit it |
| Delete or x | Reclaim the prompt and drop it |
| Esc | Leave the list |
| Any other key | Leave the list; the key goes to the composer |

## Follow-ups: steer or queue

A follow-up is a message sent against a named running turn, with intent
`steer` or `queue`.

### How a client sends one

| Client | During a turn it started | Otherwise |
|---|---|---|
| TUI and `nori attach` | Enter steers; Tab queues | A plain prompt (queued if the session is busy) |
| `nori cloud` | A plain prompt; the broker decides | A plain prompt |
| Host client | `_nori/session/follow_up` | ACP `session/prompt` |
| Broker on a managed host | `_nori/session/follow_up`, sender `machine-client` | `session/prompt` |

Tab does not queue a command: text that starts with `/` or `!`, or the words
`exit` and `quit`.

### Rules

- A follow-up names its message ID (1 to 256 bytes), its sender (1 to 256
  bytes), the expected turn (the JSON-RPC ID of the running
  `session/prompt`), its intent, and a prompt that is not empty.
- The host sets the sender: `host-controller` for host clients,
  `machine-client` for the broker. Only the controller can send follow-ups.
- Nori picks the receipt status:

| Situation | Status | Result |
|---|---|---|
| The expected turn already ended normally and the session is idle | `idle_queued` | Sent as the next prompt |
| Intent `queue`, the expected turn is running or cancelling | `queued` | Added to the queue |
| Intent `steer`, the expected turn is running, steering available, agent accepts | `injected` | Joins the running turn |
| Intent `steer`, steering not available | `unsupported_queued` | Added to the queue |
| Intent `steer`, agent reports no running turn | `idle_queued` | Sent as the next prompt |
| Intent `steer`, another steer is still unsettled | `rejected` | Dropped |
| Intent `steer`, agent rejects it (`failed`, `rejected`, or error `-32600`, `-32602`, `-32000`, `-32002`) | `rejected` | Dropped |
| Intent `steer`, agent returns `-32601` | `unsupported_queued` | Added to the queue |
| Intent `steer`, no answer within 5 seconds, or an unknown answer | `unknown` | Delivery is not known |
| The expected turn is not the running turn | `rejected` | Dropped |

- **Receipts are idempotent.** A repeated message ID with the same content
  gets the first receipt again and is not delivered twice. A repeated message
  ID with different content is an error.
- A live session keeps every receipt. At 16,384 receipts it refuses new
  message IDs.

### Steering

Steering is available only when all of these hold:

- the agent speaks ACP protocol version 1;
- its `initialize` result has `_meta.steering.supported: true`;
- its `agentInfo` names a known adapter at or above that adapter's minimum
  steering version.

| Adapter (`agentInfo.name`) | Minimum version |
|---|---|
| `@agentclientprotocol/claude-agent-acp` | 0.102.0 |
| `@agentclientprotocol/codex-acp` | 2.7.0 |

Nori steers with the `_session/steering` request and
`_meta.steering.idleBehavior: "promptRequired"` (nori-monorepo
`docs/acp-extensions.md`, upstream extensions).

### TUI notices

| Status | Notice |
|---|---|
| `injected` | Delivered to the active turn. |
| `queued` | Queued for the next turn. |
| `unsupported_queued` | Steering unavailable. Added to the next-turn queue. |
| `idle_queued` | The turn ended before steering. Queued for the next turn. |
| `rejected` | Follow-up rejected for the requested turn. (with the ID and text) |
| `unknown` | Delivery unknown. Check the conversation before sending again. (with the ID and text) |

## Goals

### Commands

| Command | Effect |
|---|---|
| `/goal <objective>` | Set an active goal. Asks first when a goal already exists. |
| `/goal` | Show the current goal |
| `/goal pause` | Suspend the goal |
| `/goal resume` | Make the goal active again |
| `/goal edit` | Edit the objective |
| `/goal clear` | Remove the goal |

Goal statuses: `active`, `paused`, `blocked`, `usage_limited`,
`budget_limited`, `complete`. A goal tracks the tokens and active time used
since it was set. Suspended time does not count.

### Who drives the goal

| Agent advertises | Driver |
|---|---|
| The goal extension (`_meta.goal`, version 1, in `initialize`) | The agent's own goal loop. Nori sends `_session/goal` (`set`, `pause`, `resume`, `clear`) and mirrors the snapshots the agent publishes. |
| HTTP MCP support, no goal extension | Nori. The agent reports progress with `get_goal` and `update_goal` on the `nori-client` MCP server. |
| Neither | `/goal` is unavailable for the session. |

- When the goal extension rejects `set` and the `nori-client` server is
  available, Nori drives the goal itself.
- A goal set with a status other than `active` always uses the `nori-client`
  server.

### Continuation (Nori drives)

- Nori puts the goal context and the `nori-client` instructions in front of
  every prompt.

### The goal on a host

Each session on a host has its own goal, kept by the host. Every terminal on
the host shows the goal of the session on screen and never another
session's.

- Any client reads a session's goal with `_nori/session/goal/get`
  {sessionId}, follows changes in `goal_changed` session events, and finds
  the latest goal on each `_nori/session/list` row (`goal`). The row keeps it
  after its event has left the replay window.
- Only the controller changes it, with `_nori/session/goal/set` {sessionId,
  op}, where `op` is `{"set": {"objective": …}}`, `"pause"`, `"resume"` or
  `"clear"`. Watchers get `-32016`. Both reply `{goal}`, which is null when
  the session has no goal. An unknown session gets `-32002`.
- The host advertises these as `capabilities.nori_sessions.goal`.
- The plain TUI and `nori attach` use these methods for any session on a
  host. A widget reads its session's goal when the session comes on screen,
  so `/goal edit` and the replace prompt use that session's goal. `nori
  attach` offers `/goal` only on a host that advertises `goal`; a watcher can
  show the goal, but changing it is refused with the draft kept.
- After a turn ends normally, if the goal is active, the session is idle, and
  the queue is empty, Nori queues a hidden goal continuation.
- A user turn can always start one continuation. A continuation can follow a
  continuation only after the agent has connected to the `nori-client`
  server, so a goal the agent cannot complete never loops.
- A turn that does not end normally starts no continuation.
- Compaction turns start no continuation.

## Compaction

`/compact` reduces the agent's context. Nori compacts only when asked.

| Agent advertises a `compact` command | Behaviour |
|---|---|
| Yes | **Native compaction.** Nori sends `/compact` as an ordinary turn. No new ACP session, no summary. |
| No | **Summarize-and-swap.** Nori sends a summarization prompt. When the turn ends, Nori keeps the summary, starts a new ACP session for the same conversation, and puts the summary in front of the next prompt. Nori then warns that many compactions can lower accuracy. |

- Both kinds wait in the prompt queue as hidden entries.
- A queue that holds only hidden entries does not pause.
- A failed compaction reports "Compact failed" and is not retried.

## Delegation

### Starting a child session

- The parent's agent calls the `delegate` tool on the `nori-client` MCP
  server. The tool exists only for agents with HTTP MCP support.
- The user runs `/delegate <agent> <task>` in the TUI.
- A host client sends `_nori/session/delegate`.

Nori admits a delegation only when all of these hold:

| Check | Error when it fails |
|---|---|
| The task is not empty | delegation task must not be empty |
| The agent ID (or alias) has a launch configuration | the agent's launch error |
| The parent session is live and not closed | parent session ... is not running / is closed |
| The child's depth is at most 2 | delegation depth limit is 2 levels below the top session |
| The session cap has a free slot | N sessions are already running; close one before delegating |

### Approval

- A child that uses the same agent as its parent starts at once (`running`).
- A child that uses a different agent waits (`awaiting_approval`). The TUI
  asks "Yes, start it" or "No, decline". `/delegate` with no arguments shows
  a pending request again. Host clients answer with
  `_nori/session/delegation/resolve`.
- A declined request returns "The user declined this delegation." to the
  parent.

### What a child gets

- The parent's folder and permission mode.
- The parent's configuration, without its initial context.
- Model: the parent's default model when the agent is the same. Otherwise
  the model bound to the agent's token provider, if one is set, or the
  agent's own default.
- Its own conversation ID, and the parent as its parent.

### States

| Status | Meaning |
|---|---|
| `awaiting_approval` | Waits for the user |
| `running` | The child is working |
| `declined` | The user declined |
| `completed` | The child finished its task |
| `failed` | The child's process or request failed. The result is the first line of the error. |
| `cancelled` | The child's last turn stopped with `cancelled`, or the user closed the parent |

### Finishing and the result

- A running child finishes when its submitted turns have ended, none of its
  own children are active, and no result waits to be delivered into it.
- The result is the agent text of the child's last turn.
- Nori queues the result into the parent as a prompt. The prompt names the
  agent, the task, the status, and the child's conversation ID. A
  user-started delegation adds a line saying the user started it.
- The finished child is closed. A completed child has session status done;
  its history stays readable.
- No result is delivered to a closed parent.

### Closing

- Closing a session closes its whole family below it.
- Every active delegation in that family becomes `cancelled` with "Session
  closed by the user".

## Fork, rewind, and undo

| Action | TUI entry | Result |
|---|---|---|
| Fork | `/fork`, first item | ACP `session/fork` at the head. A new conversation with the full history. The old session is closed. |
| Rewind | Esc twice on an empty composer, or pick a message in `/fork` | A new conversation from a summary of the history before the chosen message |
| Undo | `/undo` | Restores files from a Git snapshot. The conversation does not change. |

Fork:

- The `/fork` picker offers fork only when the agent advertises
  `session/fork`.
- Fork needs an idle session.
- The new conversation's transcript starts with the old transcript. The old
  conversation stays stored and can be resumed.

Rewind:

- Nori builds a plain-text summary of the user and agent messages before the
  chosen message and starts a new conversation with it as context.
- The chosen message goes back into the composer.
- The earlier session keeps running, parked. The new conversation joins its
  family.
- Rewind needs a free slot in the session cap.
- Rewind does not change files; use `/undo` for that.

## Where `main` differs

- Steering checks an exact version allowlist (`claude-agent-acp` 0.102.0,
  0.108.1, 0.109.0; `codex-acp` 2.7.0, 2.8.0), not a minimum version.
- `/goal pause` and the goal status `paused` use "pause", which is reserved
  for the prompt queue.
- The `/fork` picker is titled "Fork conversation", its fork item reads
  "Branch from current point", and `/fork` is described as "rewind
  conversation to a previous message".

### Context use on a host

Each session on a host reports its own context use. Every terminal on the
host shows the use of the session on screen.

- Any client, watchers included, reads a session's report with
  `_nori/session/context` {sessionId, skipInstructions?}. The reply is
  `{timeline, instructions, usage}`:
  - `timeline` holds the usage the agent reported at each prompt boundary.
  - `instructions` lists the active instruction files in the session's
    folder on the host, with token counts; `~` is the host's home.
    `skipInstructions` leaves it empty.
  - `usage` is the latest `{used, size}`, kept after its update has left
    the replay window, as on the `_nori/session/list` row
    (`context_used`, `context_size`).
- An unknown session gets `-32002`. The host advertises the method as
  `capabilities.nori_sessions.context`.
- The plain TUI and `nori attach` use it for any session on a host. `/context`
  prints the host's report. When a session comes on screen, the footer takes
  its usage from the host unless the agent has already reported a newer one.
