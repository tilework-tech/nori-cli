# Remote Control and the Session Host

Nori serves its sessions to other processes over four transports. Pick the
one that matches who owns the session and who is allowed to connect.

| § | Transport | Started by | Bind and auth | Paths |
| --- | --- | --- | --- | --- |
| 1 | Remote control (WebSocket ACP) | `nori --remote <PORT\|IP:PORT>` or `/remote-control` in the interactive TUI | Exact addresses, always including `127.0.0.1`; never a wildcard. No authentication, plaintext `ws://`. | `/acp`; also `/sessions` when `[features] multi_session` is on |
| 3 | Background host (WebSocket) | `nori host` | `127.0.0.1` only, port 7420 by default. Bearer token required on every upgrade, including loopback. | `/sessions` (session wire, §2). With `[host] managed = true`, `/sessions` is the managed control socket instead and `/managed` is added |
| 4 | Host on stdio | `nori host --stdio --launch FILE [--inherit-env]` | stdin/stdout; the host it runs internally uses an allocated loopback port and its own token | one managed launch as plain ACP |
| — | Headless stdio | `nori exec --acp` | stdin/stdout | see [headless](headless.md) |

Rules that hold across all of them:

- Every remote operation goes through the session harness, never straight to
  the downstream agent connection. Hooks, transcripts, goals, permission
  policy, and the prompt queue apply to remote input exactly as to local
  input.
- Nori adds no TLS and does not configure Tailscale Serve/Funnel, VPNs, or
  tunnels. Tailscale is used only to look up an address for
  `/remote-control on tailnet`. To reach a background host from another
  machine, provide the tunnel yourself and pass `--host URL --token-file PATH`.
- Nori Sessions may front these transports with Handroll; that topology lives
  in the Handroll and Sessions repositories, not here.

## 1. Remote control (`--remote`, `/remote-control`)

Remote control exposes the session running in an interactive `nori` TUI as an
ACP agent over the WebSocket profile of the
[ACP Streamable HTTP & WebSocket Transport RFD](https://agentclientprotocol.com/rfds/streamable-http-websocket-transport).
A client such as Zed or another Nori CLI drives the same harness the TUI is
showing. Prompts, tool calls, and results from either side render in both.

### Security

`/acp` is unauthenticated and unencrypted. It is off until you enable it,
always includes loopback while on, and never binds a wildcard. Any
non-loopback address needs an explicit per-run opt-in. When `multi_session` is
on, the same listeners also serve `/sessions` with the same lack of
authentication; `/remote-control status` lists only the `/acp` URLs, so
remember that `/sessions` is reachable at the same address.

### Enabling

`/remote-control` is handled by the TUI: it never reaches the agent and works
with no active session. Usage: `/remote-control [on [tailnet|IP:PORT]|off|status]`.

| Command | Effect |
| --- | --- |
| `/remote-control` or `/remote-control on` | Bind `127.0.0.1` on an allocated port. |
| `/remote-control on tailnet` | Run `tailscale status --json` (3 s timeout), require `BackendState` `Running` and an IPv4 address, then bind loopback and that address on one shared allocated port. |
| `/remote-control on IP:PORT` | Bind loopback and the exact address on `PORT`. A non-loopback address first shows a red confirmation that applies to this run only. |
| `/remote-control off` | Disconnect the controller and stop every listener. The harness keeps running. |
| `/remote-control status` | Report scope, the `ws://IP:PORT/acp` URLs, and controller state. |

Enable and status results are history entries listing every bound `/acp` URL.
In local-only scope (including explicit loopback such as `[::1]:PORT`), status
and enable add a hint when Tailscale is running but do not list the tailnet
address until it is bound. Re-enabling the current target only reports status.

`nori --remote <PORT|IP:PORT>` starts the same manager at launch. A bare port
binds loopback only. A non-loopback `IP:PORT` fails unless
`--remote-allow-nonloopback` is also passed (that flag requires `--remote`);
startup shows no confirmation prompt. Wildcard addresses are rejected even
with the flag.

Binding is atomic: all sockets of a surface bind before any serves. A
replacement surface binds before the old one stops, unless it reuses an exact
nonzero address of the old surface; then the old server stops first, and if
the new bind fails the old addresses are rebound and the original error is
returned. Only a failed restore leaves remote control off, reporting both
errors. App exit turns remote control off.

### WebSocket contract (`/acp`)

- `GET /acp` with a WebSocket upgrade opens a connection; other requests on
  `/acp` get `426 Upgrade Required`. Streamable HTTP/SSE is not served.
- Each upgrade response carries a fresh UUID `Acp-Connection-Id`.
- One text frame holds one UTF-8 JSON-RPC message. Binary frames are ignored.
  Ping/pong is liveness only.
- The first valid message must be `initialize` with valid params.
  Unparsable frames before it get `-32700` and the gate keeps waiting; any
  other first message, or malformed `initialize` params, closes the socket
  with code `1002`.
- The `initialize` response is the first server message; event forwarding
  starts after it.
- Outbound frames pass through a 256-frame queue. A frame that cannot be
  delivered within 30 s closes the connection.

`initialize` advertises `loadSession`, session `list`/`resume`/`close`
capabilities, agent info `nori`, and a Nori marker:

```json
{ "_meta": { "nori": { "remoteControl": { "version": 1, "activeSessionId": "3f6c1a2e-0000-4000-8000-000000000001" } } } }
```

`activeSessionId` is present once a session has started. A Nori client that
sees version 1, a non-empty ID, and `loadSession` skips its session picker and
calls `session/load` on that ID. Other clients discover the session through
`session/list`.

| Method | Behavior |
| --- | --- |
| `session/list` | Returns the one hosted session, or none. |
| `session/new` | Does not create a session. Attaches to the hosted one, responds with its ID, then replays history. `-32002` if none. |
| `session/load` | Flushes the transcript, replays history as `session/update` notifications, then responds. |
| `session/resume` | Checks that the ID is the hosted session. No replay. |
| `session/prompt` | Submits through the harness. The response is that turn's outcome under the client's own request ID. |
| `session/cancel` | Cancels only a turn this remote client started. |
| `session/close` | Closes the hosted harness session. Terminal. |

Unknown session IDs return `-32002`. The `session/load` and `session/resume`
semantics follow ACP v1:
[session setup](https://agentclientprotocol.com/protocol/v1/session-setup).

### Session identity and forwarding

The outward session ID is the Nori conversation ID. Compact and restore swap
the downstream agent session without changing it. A fork creates a new
conversation and closes the remote connection; the client reconnects and
rediscovers the session.

The remote agent forwards; it does not translate:

- `session/update` passes through with only the session ID rewritten.
- `session/request_permission` reaches the remote client only when the
  current turn is remote-owned. Other agent-to-client request types are
  answered `method not found`.
- A failed remote prompt returns `-32000`. Session end fails every pending
  remote prompt with `-32000` and closes the connection. No other Nori event
  crosses `/acp`.
- The harness brackets each turn with `SessionInfoUpdate` carrying
  `_meta.nori.status` `working` then `idle`, so an observer can track activity
  without seeing the initiator's prompt response.

Prompt display is canonical: when a prompt becomes active, the harness emits
one `user_message_chunk` sequence with the original blocks and one message ID,
ahead of agent output. Neither the TUI nor the remote client inserts its own
copy. The harness tags each prompt with `nori.dev/promptEchoId`
(`PROMPT_ECHO_ID_META_KEY`) if the caller did not, and suppresses a
downstream echo only when the marker and content both match the active
prompt.

### Controller, busy rule, and permissions

- One remote controller at a time across all listeners. A newer connection on
  any address replaces the current one (last connect wins). The replaced
  socket closes and its unanswered forwarded requests are answered
  `Cancelled`.
- The TUI and remote controller share one harness. Local prompts use the
  normal harness queue. A remote `session/prompt` is never queued: it is
  rejected with `-32015` when the session is not idle, when the queue holds
  entries and is not paused, or while a steer is unsettled. When the
  queue is paused (after any turn ending other than `end_turn`, if it holds a
  user prompt), the remote prompt
  runs at once and the queue stays paused. See
  [turn lifecycle](turn-lifecycle.md) for the queue and pause rules.
- For a remote-owned turn, both the TUI and the remote client may answer the
  same permission request. The first answer wins; the second gets an error
  that no request is pending.
- The app attaches the remote host at `SessionStarted`, whether or not a
  listener is on, so enabling later exposes the running session at once. On
  an agent switch the host keeps following the old session until the new one
  starts; a failed switch is invisible remotely. A committed switch
  disconnects the controller.
- With `multi_session`, switching sessions in the TUI stages the on-screen
  session as the default for the next `/acp` connection. A connected
  controller stays pinned to the session it attached to. An unstarted or
  ended on-screen session clears the default.

### Reconnect

Socket EOF or network loss detaches the controller only. It does not close
the session, stop the agent, cancel the active prompt, or exit the TUI.
Unanswered forwarded requests are cancelled so they cannot block the agent.

Reconnect is a fresh connection: new `Acp-Connection-Id`, new `initialize`,
then `session/load` to recover.

- Replayed: the conversation history from the transcript.
- Lost: frames sent while disconnected (there are no sequence numbers),
  in-flight requests (never retried), and permission requests that were
  cancelled on disconnect.
- A `session/load` during a streaming turn can interleave live updates with
  the replay.

### Current limits of `/acp`

- `initialize` echoes the client's requested protocol version.
- `session/list`, `session/new`, `session/load`, and `session/resume` ignore
  `cwd`, pagination cursors, and MCP server parameters.
- Live updates flow right after `initialize`, before `session/load` or
  `session/resume`.
- A permission request emitted before a remote prompt's turn is registered is
  shown only in the TUI.
- `session/close` can close the socket before its response is delivered.
- Delegation events are not forwarded; a delivered delegation result appears
  as an ordinary user prompt in the stream.

## 2. Managed session wire (`/sessions`)

`/sessions` carries the session manager over JSON-RPC envelopes. It is served
by the TUI's remote-control listeners only with `[features] multi_session`
(unauthenticated, see §1) and by every `nori host` that does not set
`[host] managed = true` (bearer token, see §3).
The same flag switches `nori exec --acp` to the managed stdio facade, which
advertises `agentCapabilities._meta["nori/multiSession"]` version 1; see
[headless](headless.md).

Each text frame is a JSON-RPC 2.0 request with an ID and object params.
`initialize` comes first and returns `result.capabilities.nori_sessions` with
`version: 1`, `envelopes`, and `reconnect`. The background host adds
`session_cwd: true` and `follow_up: 1`.

| Method | Params | Result |
| --- | --- | --- |
| `_nori/session/list` | `{}` | `selected_session_id`, `sessions`, `replay_cursors` |
| `_nori/session/subscribe` | optional `session_id` (or `"*"`), optional `cursors` map | `{ "subscribed": true }`, then envelopes |
| `_nori/session/prompt` | optional `session_id`, ACP `prompt` blocks, optional `meta` | `session_id`, admitted `request_id` |
| `_nori/session/follow_up` | `session_id`, `expected_turn`, `message_id`, `intent` (`steer` or `queue`), `prompt` | delivery receipt. Host controller only; `-32601` on the TUI listener. |
| `_nori/session/respond` | optional `session_id`, permission `request_id`, ACP `response` | `session_id`, `answered: true` |
| `_nori/session/delegate` | optional parent `session_id`, `agent`, `task` | delegation record |
| `_nori/session/delegation/resolve` | optional parent `session_id`, `request_id`, `approved` | updated record |

- An explicit `session_id` always wins. Omitting it uses the selected session
  (in the TUI, the one on screen; in exec, the first started). A subscription
  pins its target when made. Only subscribe accepts `"*"` (all current and
  future sessions).
- Events arrive as `_nori/session/event` notifications whose params are the
  full `SessionEnvelope`: `conversation_id`, `agent`, `seq`, `event`.
- A prompt result acknowledges admission, not completion; watch the event
  stream for the turn's outcome. `_nori/session/prompt` follows the same busy
  rule as `/acp` and returns `-32015`.
- `_nori/session/follow_up` sends a message into a running turn: `steer`
  asks the agent to take it now (ACP extension `_session/steering`, only for
  verified adapter versions), `queue` holds it for the next turn. Receipts and
  fallbacks are described in the
  [host docs](../nori-rs/cli/src/host/docs.md#mid-turn-follow-ups). Clients
  must check the `follow_up` capability before calling it.
- A permission answer addresses the conversation and request IDs. A repeated
  answer is rejected with `-32600`. `RequestResolved` tells every subscriber
  that the request was answered.
- Store and forward event payloads as raw JSON. ACP response types are
  untagged; a generic deserialize/serialize round trip can drop fields.

List rows report identity, agent, status, `family_status`, cwd, lineage,
pending request IDs, `source`, `depth`, `delegations`, `latest_seq`,
`first_available_seq`, age, and title, plus display fields older hosts omit:
`reply_preview`, `waiting_request`, `model`, `idle_seconds`, `context_used`,
`context_size`.

### Paused queues

A paused queue cannot be resumed or discarded from `nori attach` or any remote
client; only the local TUI that owns the harness can resume it. Queue
visibility depends on the surface:

- `/sessions` subscribers, including `nori host` watchers, receive
  `QueueChanged` snapshots with each entry's text and the paused reason
  (for example `"paused":"interrupted"`). They cannot edit, remove, resume,
  or discard entries.
- `/acp` drops `QueueChanged`.
- `nori attach` receives the snapshots but does not render them.

### Reconnect and replay

- The manager journals at most 4096 envelopes in total across sessions, in
  memory, for the current process lifetime only.
- Each session's `seq` starts at 1. A cursor is the last envelope the client
  processed; subscribe replays strictly greater sequences, then switches to
  live delivery with no gap or overlap. Omitted cursors mean zero.
- Persist cursors after processing each envelope. To reconnect: initialize,
  then subscribe with those cursors. A new subscription replaces the
  connection's previous one.
- Replay never retries mutations. Do not resubmit a prompt whose
  acknowledgement was lost; check the stream first.
- Negative, future, unknown, mismatched, or expired cursors fail with
  `-32003`. Missing or unknown command targets fail with `-32002`. To skip
  lost history on purpose, list again and subscribe from `replay_cursors` or a
  row's `latest_seq`.
- Disconnect detaches managed clients without stopping sessions, cancelling
  turns, or answering permissions; pending permissions stay answerable after
  reconnect or from the TUI.
- A slow consumer is dropped when its live queue fills. WebSocket closes the
  connection after draining buffered events. Stdio instead sends
  `_nori/session/subscription_ended` with `{"reason":"replay_required"}` and
  keeps the connection; resubscribe with your cursors.

On the TUI listeners, `/acp` and `/sessions` share one controller slot
(last connect wins). The background host uses controllers and watchers
instead (§3).

### Delegation as seen by remote clients

Delegation is behind `multi_session`. A controller starts it with
`_nori/session/delegate`; the local `/delegate <agent> <task>` command and the
agent's `delegate` MCP tool use the same manager operation.

- A delegation record holds `request_id`, `parent_id`, `child_id`,
  `parent_agent`, `agent`, `task`, `depth`, `status`, optional `result`, and
  `origin` (`agent` for the MCP tool, `user` otherwise).
- Statuses: `awaiting_approval`, `running`, `declined`, `completed`,
  `failed`, `cancelled`.
- A same-kind child starts immediately, including when the agent asked for
  it. A different-kind child waits in `awaiting_approval` until the
  controller calls `_nori/session/delegation/resolve` with `approved`.
- Children may delegate once more: depth is at most two levels below the top
  session. Startup must fit the shared session cap.
- `DelegationChanged` envelopes go to both the parent's and the child's
  streams. A parent-only subscription does not include the child's own
  envelopes; subscribe to `"*"` or to the child.
- A child's permission request belongs to the child. The parent gets a
  notice naming both sessions and the next-waiting key.
- The child's result is queued into the parent as an attributed user prompt
  (`[Nori delegation result] from <agent> · <task> · <outcome>`, then
  `Child session: <id>`), even when the parent is busy. The manager keeps it
  until the parent's prompt response, and resends it after a parent crash and
  `_nori/session/resume`.
- Closing a parent cancels its descendants and pending approvals. Delegation
  state lives in the current manager; a host restart does not restore it.

## 3. Background host (`nori host`)

`nori host` runs sessions without a terminal. It starts a detached Unix
process with an empty session manager, binds `127.0.0.1` only, and keeps
running after every terminal detaches. `nori host` itself is not gated;
`nori attach` requires `[features] multi_session = true`.

| Command | Behavior |
| --- | --- |
| `nori host [--port N]` | Start the host (default port 7420; `0` allocates one), or report the running one. |
| `nori host ls` | List sessions, children under their parent. A TTY gets the two-row session layout; piped output is one plain `ID AGENT STATUS AGE CWD TITLE` row per session with full values. |
| `nori host new [--cwd DIR] [--agent A] [--prompt TEXT] [--json]` | Start a session and print its ID, or `{session_id, cwd}` with `--json`. Add `--host URL --token-file PATH` (both required together) for another host. Refuses a host that does not advertise `session_cwd`. |
| `nori host stop` | Request shutdown, then wait up to 30 s for cleanup. Prints `No host running.` when none is. |
| `nori attach [SESSION_ID]` | Open a terminal on the host: the session list, or the named session. |
| `nori attach --host URL --token-file PATH` | Attach to an explicit endpoint with a private token file. |
| `nori host --stdio --launch FILE [--inherit-env]` | Serve one managed launch as ACP on stdin/stdout (§4). |

`nori attach` accepts any unique start of a session ID. A full ID in canonical
form skips lookup; anything else is matched against `_nori/session/list`
(3 s timeout). Several matches print the candidates; none points at
`nori host ls | cat`. `nori host stop` and `nori host ls` use only the local
host.

### State and authentication

- Discovery state, token, log, and lock live under `$NORI_HOME/host`
  (directory `0700`, files `0600`, owned by the user, regular files, no
  symlinks). An OS lock, not the saved PID, decides whether a host is
  running. Hosting is refused on platforms without these Unix checks.
- Each start generates a new token. Local commands read it automatically;
  startup prints the token path, never the value.
- Every WebSocket upgrade, including on loopback, needs
  `Authorization: Bearer <token>`. A missing or wrong token gets HTTP 401
  before the upgrade. A valid token without an upgrade gets 426.
- Authenticated upgrades return `x-nori-host-instance`, a UUID per host
  process. Clients compare it on reconnect so old cursors never target a new
  manager.

```bash
curl -i -H "Authorization: Bearer example-token" \
  -H "Connection: Upgrade" -H "Upgrade: websocket" \
  -H "Sec-WebSocket-Version: 13" -H "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==" \
  http://127.0.0.1:7420/sessions
```

### Endpoints

- `/sessions`: the managed session wire (§2) with the controller/watcher
  rules below. With `[host] managed = true` it becomes the managed control
  socket instead: `initialize` advertises
  `agentCapabilities._meta["nori.dev/managedHost"]`, and only
  `_nori/session/ensure`, `_nori/session/status`, and `_nori/host/*` are
  accepted. `nori host ls`, `nori host new`, and `nori attach` do not work
  against such a host.
- `/managed`: only with `[host] managed = true`. Standard ACP for machine
  adapters, scoped by binding and generation. See the
  [managed launch contract](../nori-rs/cli/src/host/docs.md#managed-launch-control-v1)
  and the
  [managed machine connection](../nori-rs/acp-host/docs.md#managed-machine-connection).

### Socket rules

- Each host socket processes one request at a time. Events for that socket
  wait while one of its requests is in flight; other sockets are not
  affected.
- A binary frame closes any `/sessions` socket, on the host or the TUI
  listener (`/acp` ignores it).
- A first request other than `initialize` gets `-32001`, then the socket
  closes.
- Host lifecycle errors (`_nori/session/close`, `/cancel`, `/resume` on an
  unknown or wrong-state session) return `-32000`, not `-32002`.

### Controllers and watchers

The first interactive `initialize` claims control if the slot is empty. Later
clients are watchers. `initialize` with `{"observe":true}` never claims
control, so `nori host ls`, `nori host new`, and `nori host stop` do not take
it. `initialize` returns `result.control` with `role` and
`controller_connected`.

| Method | Who | Meaning |
| --- | --- | --- |
| `_nori/control/status` | any | This connection's role and whether a controller is connected. |
| `_nori/control/take` | any | Move control to the caller; the old controller stays connected as a watcher. |
| `_nori/session/new` | any | Start a session (optional absolute `cwd`, `agent`, `prompt`); it becomes the selected session. Replies `{session_id, cwd}`. |
| `_nori/session/close`, `_nori/session/cancel` | controller | Close or cancel the given `session_id`. |
| `_nori/session/resume` | controller | Restart a failed session (see below). |
| `_nori/host/stop` | any | Shut the host down. |

Watchers may list, subscribe, start sessions, stop the host, and take control.
On a host with `[host] managed = true`, watchers may also call
`_nori/session/ensure` and `_nori/session/status`. Prompts, follow-ups,
permission answers, delegation, close, cancel, and resume from a watcher fail
with `-32016`. `_nori/control/changed` tells affected terminals their new
role. A request already admitted may finish after control moves. When the
controller disconnects, no watcher is promoted automatically.

## 4. Host on stdio (`nori host --stdio`)

`nori host --stdio --launch FILE [--inherit-env]` lets a machine bridge that
already pipes an adapter's stdio run the host in its place. It needs
`[host] managed = true` and `[host.dedicated_roots]`; startup fails without
them. The host runs in the foreground on an allocated loopback port, admits
the launch described in `FILE` once, and relays plain ACP over stdin/stdout
until stdin closes, then stops. `--inherit-env` adds this process's
environment to the launch; variables in the launch file win. Details:
[managed launch on stdio](../nori-rs/cli/src/host/docs.md#managed-launch-on-stdio).

## 5. Resume and load: four different operations

The word "resume" names two unrelated operations. Do not confuse them.

| Operation | Transport | What it does | History replay |
| --- | --- | --- | --- |
| `session/new` | `/acp` | Attaches to the one hosted session; does not create one. | Yes, after the response |
| `session/load` | `/acp` | Attaches and recovers after reconnect. | Yes, before the response |
| `session/resume` (ACP) | `/acp` | Checks that the ID is the hosted session. | No |
| `_nori/session/resume` | `/sessions` | Restarts a **failed** session's agent process. Requires status `failed`; reloads configuration from the session's cwd. | No; subscribe with cursors |

`_nori/session/resume` keeps the Nori conversation ID, list row, and
sequence counter, so existing subscriptions and saved cursors still apply.
Only the downstream agent's own session ID may change. In `nori attach`,
Ctrl+R in the session list and `/resume` both call it.

## 6. Attached terminal (`nori attach`)

`nori attach` is a terminal frontend for the host. It owns terminal state and
replay cursors, never agent processes. See the
[attach frontend docs](../nori-rs/tui/src/attach/docs.md) for rendering.

### Control

The status line shows controlling, watching (read-only), reconnecting, or
disconnected. Alt+T takes control. Watchers can browse and read; anything
that changes a session needs control.

### Keys and commands

| Input | Effect |
| --- | --- |
| Enter (turn running) | Steer: `_nori/session/follow_up` with `intent: "steer"`. Every draft except the attach commands below is steered, including `!` and other slash text. |
| Tab (turn running) | Queue: `_nori/session/follow_up` with `intent: "queue"`. Tab first completes a `/new` folder. Slash and `!` drafts are not queued; a bare `exit` or `quit` is. |
| Enter (idle) | `_nori/session/prompt`. |
| Ctrl+C | Empty composer: send `/cancel`. Otherwise clear the draft. |
| Ctrl+D, `/quit`, `/exit`, `/detach` | Detach. The host and its sessions keep running. |
| `/close`, `/cancel`, `/resume` | Close, cancel, or restart the selected session. |
| `/new [DIR] [AGENT]` | Start a session in the attaching directory or `DIR`. |
| `/delegate <agent> <task>` | Delegate from the selected session. |
| `/sessions`, Left on an empty composer | Open the session list. |
| `hotkeys.open_session_list` (Alt+S by default) | Toggle the session list. |
| Ctrl+R (list open) | Restart the highlighted session if it failed; otherwise `Only failed sessions can be resumed.` |
| `hotkeys.next_waiting_session` (Alt+Down) | Jump to the next session waiting on you. |
| `hotkeys.parent_session` (Alt+Up) | From a delegated child, open its parent. |
| Alt+T | Take control. |

Follow-up delivery notices match the local TUI. Unlike the local TUI, attach
puts the draft back in an empty composer after a `rejected` receipt or an
error response. Attach has no queue list: it
cannot show, edit, remove, resume, or discard queued prompts, and a queue
paused by an interrupt (for example after `/cancel`) stays paused until the
TUI that owns the harness resumes it.

While a permission or delegation approval shows for the on-screen session,
the composer is hidden and only request keys and PageUp/PageDown act. Esc
rejects (or declines); Ctrl+C cancels (or declines). Option keys are ignored
for 400 ms after the request appears, so a key typed just before cannot
answer it. Each session keeps its own unsent draft.

### Reconnect

- Socket loss or a 10 s receive-liveness timeout starts recovery. The
  terminal keeps its rendered history and draft, retries every 750 ms,
  checks `x-nori-host-instance`, initializes, and resubscribes the selected
  session with its last applied cursor.
- Requests queued while disconnected are discarded, never retransmitted.
- A changed host instance stops recovery and reports that the host restarted
  and its sessions were lost.
- On 401 or 403, a locally discovered attachment rereads the token file once
  and retries. An attachment with `--host` never rereads local credentials.
- Replay expiry or a sequence gap shows an error; the terminal never jumps
  silently to live output. Detach with Ctrl+D and rerun `nori attach`.

## 7. Code map

```text
nori-rs/
├── acp-host/src/remote/
│   ├── server.rs          # listeners, bind policy, /acp upgrade, host router
│   ├── connection.rs      # /acp initialize gate, ACP handlers, forwarding
│   ├── wire.rs            # frame adapters, bounded output
│   ├── hosted_agent.rs    # HostedAgent trait (implemented by the harness)
│   ├── managed.rs         # ManagedAgent trait for /sessions
│   ├── managed_wire.rs    # /sessions on the TUI listeners
│   ├── host_wire.rs       # bearer auth, /sessions and /managed on the host
│   ├── host_control.rs    # controller/watcher admission
│   ├── standard_wire.rs   # /managed standard ACP
│   └── client.rs          # managed WebSocket client (attach, host CLI)
├── harness/src/
│   ├── remote_agent.rs        # HostedAgent over the harness (/acp)
│   ├── managed_remote.rs      # ManagedRemoteHost (/sessions methods)
│   ├── session_event_fanout.rs
│   ├── follow_up.rs           # steering guard and receipt mapping
│   ├── follow_up_rpc.rs       # _nori/session/follow_up params
│   └── session_manager/       # sessions, journal, delegation
├── cli/src/host/          # nori host, nori attach, stdio relay, token state
├── tui/src/
│   ├── cli.rs             # --remote, --remote-allow-nonloopback
│   ├── remote_control.rs  # /remote-control manager and lifecycle
│   ├── chatwidget/remote_control.rs  # non-loopback confirmation
│   └── attach/            # nori attach frontend
└── exec/src/managed.rs    # nori exec --acp managed facade
```
