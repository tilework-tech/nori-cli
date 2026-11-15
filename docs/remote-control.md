# Remote ACP Transport

**Status:** Implemented ACP transport plus feature-gated managed-session transport

**Date:** 2026-08-23 (revised 2026-11-15)

**Upstream:** [ACP Streamable HTTP & WebSocket Transport RFD](https://agentclientprotocol.com/rfds/streamable-http-websocket-transport)

## 1. Decision

Nori CLI will optionally expose its running harness as an ACP Agent over the
WebSocket profile of the upstream remote transport RFD.

The server belongs in `nori-acp-host`, which already owns the ACP SDK,
subprocess connection, and wire lifecycle. The interactive application owns
one stable harness host and the complete listener lifecycle. Remote access is
disabled unless startup configuration or a runtime command explicitly enables
it. Every enabled surface binds exact addresses and includes loopback; no
wildcard listener or separate session-host crate is introduced.

Without `[features] multi_session`, `nori exec --acp` remains a bounded,
terminal-independent stdio facade and `/acp` exposes the interactive harness.
With the flag, `/sessions` and the existing exec facade share the managed
session commands described in section 10. This adds transport primitives only;
it does not introduce host/attach commands or a persistent host lifecycle.
Sections 3–6 describe the legacy `/acp` projection unless stated otherwise.

## 2. Topology

```text
Zed
  └─ stdio ─► local Handroll facade
                 └─ WebSocket ─► microVM Nori CLI
                                      └─ stdio ACP ─► Codex or another agent

terminal user ─► microVM Handroll PTY attach ─► the same Nori CLI process
```

The local Handroll process lists and flattens sessions across Nori hosts. The
microVM Handroll process, when present, owns only the persistent PTY and
terminal detach/attach. WebSocket ACP traffic terminates directly in Nori.

## 3. Code ownership

`nori-acp-host` gains an optional remote-server module. It owns:

- the `/acp` WebSocket endpoint;
- connection identity and initialization state;
- WebSocket framing, ping/pong, bounded output, and disconnect cleanup;
- the outward ACP Agent implementation.

The TUI owns the runtime policy above that server: it selects exact listener
addresses, retains one stable `HarnessRemoteHost`, reports reachable endpoints,
and starts or stops all listeners without changing the harness lifetime. The
host remains attached while listeners are disabled, so enabling remote control
can expose the already-running session immediately.

`nori-acp-host` must not depend on `nori-harness`. It defines a small
`HostedAgent` interface using `nori-protocol` types; `nori-harness` implements
that interface through `HarnessHandle`. The existing dependency direction
therefore remains `nori-harness` → `nori-acp-host`.

The remote Agent must call the harness interface rather than calling the
downstream `AcpConnection` directly. Direct calls would bypass Nori-owned
hooks, transcripts, goals, permissions, prompt state, and session switching.

The existing raw `ConnectionEvent` channel remains private and
single-consumer. The harness replaces its single frontend event receiver with
an ordered, subscribable `SessionEvent` fan-out. The TUI and remote Agent are
separate consumers of that post-harness stream. A slow remote consumer must
never block the harness or TUI; its connection is closed if its bounded queue
overflows.

The remote Agent forwards the post-harness ACP stream rather than translating
it. `session/update` notifications pass through unmodified except for the
outward session ID. Responses are correlated at the boundary: the transport
tracks the harness request it issued and answers under the remote client's
own request ID, as `nori exec --acp` does today. Delegated agent-to-client
requests such as `session/request_permission` go to the remote controller
after harness policy. `SessionEnded` and `RequestFailed` surface as JSON-RPC
errors on the affected request or close the connection; no other `NoriEvent`
is forwarded in this version.

The post-harness stream includes one canonical user-message sequence when each
queued prompt becomes active. It contains the caller's original ACP blocks,
uses one message id across the sequence, and precedes agent output. Because the
TUI and remote Agent subscribe to this same fan-out, both render the prompt
without transport-specific insertion.

Nori composition preserves correlation end to end. The public
`nori_protocol::PROMPT_ECHO_ID_META_KEY` constant names
`nori.dev/promptEchoId`; the WebSocket handler passes the remote
`PromptRequest.meta` through `HostedAgent` and `HarnessHandle`, and the harness
retains that metadata while queued before forwarding it on the downstream
`session/prompt`. The harness supplies a marker when the caller did not. Each
canonical user chunk carries only that marker, and remote event forwarding
changes only its session id. A downstream echo is suppressed only when its
marker identifies the active prompt and its content matches active wire
content. Load/replay, unowned, and unmarked activity is forwarded; ownership is
never guessed from content alone.

## 4. WebSocket contract

The first implementation is WebSocket-only, which the upstream RFD permits
for servers. Streamable HTTP/SSE is not required.

- `GET /acp` with `Upgrade: websocket` opens the connection.
- The upgrade response includes a new `Acp-Connection-Id`.
- `initialize` must be the first JSON-RPC request on the socket.
- The initialize response is sent before live session-event forwarding starts,
  so it is the first server message on an accepted connection.
- Each WebSocket text frame contains one UTF-8 JSON-RPC message.
- Binary frames are ignored.
- WebSocket ping/pong provides liveness; it has no ACP meaning.
- ACP methods, notifications, request IDs, and session IDs retain their normal
  protocol semantics.

The remote surface issues Nori conversation IDs as its ACP session IDs.
Downstream agent session swaps that continue the same conversation (compact,
restore) are invisible to remote clients; the outward session ID never
changes for a continuing conversation. A fork starts a new conversation with
a new ID and the server closes the remote connection. On initialize, the Nori
remote-control surface advertises version 1 in
`_meta.nori.remoteControl` and includes the current stable ID as
`activeSessionId` when one exists. A recognizing client whose agent advertises
`loadSession` attaches directly with `session/load`; other ACP clients can
continue to discover the session through `session/list`.

The `/acp` transport adapts WebSocket frames to the same ACP Agent handler
used by the host. The separate `/sessions` endpoint carries manager envelopes
when `multi_session` is enabled.

## 5. Detach, reconnect, and close

A WebSocket connection and an ACP session have separate lifetimes.

The server accepts one remote controller at a time across all of its exact
listeners. Those listeners share connection identity state; a newer connection
on any address replaces the current one—last connect wins—and the replaced
socket is closed. The ordered harness fan-out is already shared with the TUI
and can supply future remote observers; accepting those observer connections
is not part of this version.

- Socket EOF or network loss detaches the remote client. It does not close the
  Nori harness session, stop the downstream agent, or exit the TUI.
- Reconnection creates a new transport connection and
  `Acp-Connection-Id`. The client initializes again. A Nori client recognizes
  the remote-control marker and immediately uses `session/load` for its stable
  session ID; other clients use their supported ACP discovery/resume path.
- `session/close` is terminal for the selected harness session.
- Foregrounding or detaching the Handroll-owned terminal does not affect the
  WebSocket connection.
- Disabling runtime remote control closes the active controller and stops every
  listener. Server-wide cancellation also rejects an accepted upgrade whose
  callback races with shutdown; consuming shutdown waits for the aborted accept
  tasks before reporting completion. It does not detach or shut down the hosted
  harness session.

The first version follows the RFD's v1 reliability model: no sequence numbers,
no replay of messages missed while disconnected, and no transparent retry of
an in-flight JSON-RPC request. The minimal implementation must therefore
advertise `loadSession`: `session/load` replays history from the Nori
transcript and is the recovery path after a reconnect. A client that calls
`session/load` while a turn is still streaming may see live updates
interleaved with the replayed prefix; without sequence numbers, v1 provides
no deduplication. The harness and transcript continue recording
activity while no remote client is attached. A disconnected controller's
unanswered delegated requests are cancelled so they cannot wedge the agent;
the active prompt is not cancelled merely because the socket disappeared.

## 6. TUI coexistence

The TUI remains attached to the same `HarnessHandle` and receives the same
ordered `SessionEvent` stream while a remote ACP client is connected. Remote
mutations also pass through that handle, so their prompts, updates, tool calls,
and results appear in the existing TUI state. A prompt is inserted by neither
frontend: once it becomes active, both render the harness's canonical
`user_message_chunk` sequence before agent output.

Opening the microVM terminal therefore reveals the already-running Nori TUI;
it does not reconstruct a second frontend or replace the WebSocket
controller. The first accepted prompt owns the turn. Local prompts retain the
existing harness queue; a remote prompt received while any turn is active is
rejected as busy and is never queued. A remote cancel affects only a
remote-owned turn, never activity started by the local TUI.

Observers receive the complete shared update stream, including final assistant
content. The harness brackets each active turn with `session/update`
`SessionInfoUpdate` notifications carrying `_meta.nori.status` values
`working` and `idle`. This optional Nori extension lets an observing frontend
enter and leave its working state without receiving the initiator's prompt
response or stop reason. The remote host only rewrites the session id and
forwards these notifications.

An agent switch uses the TUI's transactional session boundary. The remote host
continues following the current `HarnessHandle` while a candidate initializes,
lists sessions, or attempts activation. Candidate failure or cancellation is
therefore invisible to the remote attachment. Only the candidate's
`SessionStarted` commits the replacement; the harness seeds the new hosted
session from that already-observed event and subscribes from the commit
boundary onward. Replacing the hosted session disconnects the current remote
controller under the existing hosted-session replacement behavior, so it
reconnects and rediscovers the newly committed conversation.

The app attaches the stable host only after an active session publishes
`SessionStarted`, whether or not any listener is currently enabled. This makes
session identity explicit, keeps runtime enable independent of launch timing,
and applies the same commit boundary to ordinary launches and agent switches.
With `[features] multi_session`, the app remembers committed identities for
both visible and parked sessions. Navigation stages the on-screen session as
the default for the next `/acp` connection. A connected legacy controller stays
pinned to the session it attached to; local navigation does not retarget its
prompts or pending permissions. An unstarted or ended on-screen session clears
the next connection's default instead of falling back to an earlier session.
Managed clients use explicit session IDs to address any session independently
of the screen.

## 7. Runtime control and bind policy

The interactive TUI exposes these client-owned commands; they never become an
agent prompt and remain available when no agent session is active:

| Command | Effect |
| --- | --- |
| `/remote-control` or `/remote-control on` | Bind `127.0.0.1` on an allocated port. |
| `/remote-control on tailnet` | Run `tailscale status --json`, require a running node and exact IPv4, then bind loopback and that address on one shared allocated port. |
| `/remote-control on IP:PORT` | Bind loopback and the exact address on the requested port. A non-loopback address requires a red, one-shot confirmation that is never persisted. |
| `/remote-control off` | Disconnect the controller and stop all listeners without stopping the harness. |
| `/remote-control status` | Report scope, reachable endpoints, and controller state. |

Wildcard addresses are rejected. Successful enable and status results are
durable TUI history entries containing every reachable `ws://.../acp` URL;
loopback is always included while enabled. Local-only mode may report that
Tailscale is available and suggest `on tailnet`, but it must not present the
tailnet address as reachable before binding it. Explicit loopback targets,
including IPv6 loopback supplied at startup or runtime, remain local-only and
receive the same hint. Runtime control does not invoke Handroll, mutate
Tailscale Serve or Funnel state, discover other VPNs, or add an authentication
layer.

`nori --remote <PORT|IP:PORT>` enters the same app-owned lifecycle. A bare port
is loopback-only. An exact non-loopback startup address still requires
`--remote-allow-nonloopback`, then produces loopback and exact-address listeners
on the requested port. All requested sockets bind successfully before any one
of them begins serving, so a failure cannot leave a partial new surface.
Replacing a surface normally binds the new set before shutting down the old
one. If the new set reuses an exact nonzero address owned by the old set, the
manager first consumes and awaits the old server so the port is available;
it snapshots the old target and exact addresses before doing so. If the new
bind fails, the manager restores that previous surface and returns the original
error; its controller must reconnect. Only a second bind failure while
restoring leaves remote control disabled, and that error reports both failures.

## 8. Implementation boundary

```text
nori-rs/
├── acp-host/
│   ├── Cargo.toml                         # Remote transport deps (axum ws)
│   └── src/
│       ├── lib.rs                        # Exposes the remote module
│       └── remote/
│           ├── mod.rs                    # Public server API
│           ├── hosted_agent.rs           # Downward-facing control interface
│           ├── server.rs                 # Listener, WS upgrade, bind policy
│           ├── connection.rs             # Initialize gate, handlers, forwarding
│           └── wire.rs                   # Frame/line adapters, bounded output
├── harness/src/
│   ├── runtime.rs                        # Subscribable SessionEvent fan-out
│   └── remote_agent.rs                   # HostedAgent over HarnessHandle
├── tui/src/
│   ├── cli.rs                            # --remote / --remote-allow-nonloopback
│   ├── remote_control.rs                 # Runtime policy and app-owned lifecycle
│   ├── app/                              # Commands and SessionStarted attachment
│   └── chatwidget/                       # Input parsing and exposure confirmation
└── exec/src/managed.rs                    # Flag-on stdio manager/extension adapter
```

The `--remote` flag lives on the interactive CLI surface and is normalized into
the same TUI-owned manager as runtime commands; the `cli` crate itself remains
unchanged. Remote prompts use the same harness-originated user-message
notifications as local prompts, so their
original content is visible in the observing TUI and every attached frontend.

## 9. Planned compatibility and lifecycle follow-up

The following items are planned follow-up work rather than guarantees of the
v1 transport:

- A remote prompt can emit an immediate permission request before remote turn
  ownership is registered, silently dropping the request and wedging the turn.
- The TUI and remote controller can both answer the same delegated permission
  request and race contradictory decisions.
- `session/close` can cancel the WebSocket after `SessionEnded` but before its
  JSON-RPC success response is delivered.
- Initialization echoes unsupported requested protocol versions instead of
  negotiating the latest protocol version the server supports.
- `session/list`, `session/load`, and `session/resume` return success without
  applying required request context such as `cwd`, pagination cursors, and MCP
  server setup.
- An initialized socket receives live session updates before it explicitly
  attaches to the session through `session/load` or `session/resume`.

Authentication, TLS, endpoint discovery, broker routing, Handroll federation,
capability aggregation across hosts, Streamable HTTP/SSE, and replay across a
host-process restart remain outside this implementation. Legacy `/acp` retains
its transcript-based recovery; managed envelopes add bounded reconnect replay
within the current manager lifetime.

## 10. Managed session wire (`multi_session`)

The TUI adds `GET /sessions` WebSocket upgrades on the same exact listener
addresses as `/acp` only with `[features] multi_session`. The same flag selects
managed operation for `nori exec --acp`. Flag-off behavior and loopback defaults
are unchanged. `ManagedAgent` in
[`acp-host`](../nori-rs/acp-host/src/remote/managed.rs) defines the transport
boundary; [`ManagedRemoteHost`](../nori-rs/harness/src/managed_remote.rs)
implements it over the app's `SessionManager`, keeping mutation and policy in
the harness.

Each WebSocket text frame is a JSON-RPC 2.0 request with an ID and object
params. `initialize` must come first and advertises
`result.capabilities.nori_sessions` with version 1, envelopes, and reconnect.
The stdio facade retains standard ACP initialization and advertises version 1
at `agentCapabilities._meta["nori/multiSession"]`. It additionally accepts
standard `session/new` for independent managed sessions and repeated standard
prompts; the WebSocket extension controls sessions already owned by the TUI.

| Method | Params | Result |
| --- | --- | --- |
| `_nori/session/list` | `{}` | `selected_session_id`, `sessions`, `replay_cursors` |
| `_nori/session/subscribe` | Optional `session_id`, optional `cursors` map | `{ "subscribed": true }`, then envelope notifications |
| `_nori/session/prompt` | Optional `session_id`, ACP `prompt` content blocks, optional `meta` | `session_id`, admitted harness `request_id` |
| `_nori/session/respond` | Optional `session_id`, permission `request_id`, ACP permission `response` | `session_id`, `answered: true` |

A list row reports identity, agent, status, cwd, lineage, pending request IDs,
sequence bounds (`latest_seq`, `first_available_seq`), age in seconds, and a
title derived from observed session activity. `replay_cursors` includes every
conversation known to the journal. Restarting a failed session with Ctrl+R keeps
its Nori conversation ID, list row, and sequence counter. Existing subscriptions
and saved cursors continue to address that ID after the new agent process
starts; clients do not migrate to a replacement ID. The downstream ACP session
ID can change when native load is unavailable or fails. `resumed_from` remains
internal manager metadata and is omitted from the remote list.

An explicit `session_id` always wins. Omitting it uses the currently selected
TUI session (the first successfully started session in exec). Subscriptions pin that choice
at subscription time; later commands that omit the ID use the current default.
Use explicit IDs to keep mutations pinned across screen changes. Only subscribe
accepts `"*"`, meaning all current and subsequently created sessions.

Events are notifications named `_nori/session/event`; `params` is the complete
unchanged `SessionEnvelope`: `conversation_id`, `agent`, `seq`, and `event`.
The envelope supplies routing identity even when an embedded ACP payload uses
the downstream agent's own session ID. Both ACP and Nori lifecycle events cross
this boundary. A prompt result acknowledges admission, not completion; observe
the correlated response in the event stream for the completed turn. Busy
sessions reject remote prompts instead of queuing them. Permission replies
address the pair of conversation and request IDs, and a repeated managed answer
is rejected once claimed. `RequestResolved` tells subscribers that the request
has been answered. In managed exec, an extension answer can also resolve a
permission forwarded by a standard ACP prompt: the prompt collector observes
the matching manager resolution and continues to the original prompt result
without waiting for a second answer from the standard ACP caller.

Keep event payloads as raw JSON when storing or forwarding envelopes. ACP
response types are untagged; interpret a response using its correlated request
method rather than a generic deserialize/serialize round trip, which can choose
an empty response variant and discard fields. The transport serializes the
original manager event without that round trip.

### Reconnect and retention

The manager journals at most 4096 envelopes **total across sessions** in memory.
History is available only for the current manager lifetime, independently of
transcript persistence. Each session's sequence begins at 1. A cursor means
"last envelope the client has processed"; subscribe replays strictly greater
sequences. Omitted cursor entries mean zero, including newly created sessions.

The manager takes the replay snapshot and registers the live subscriber under
the same mutex used to number and publish events. The transport sends the
subscribe response, then the ordered replay, then live envelopes. There is no
gap between replay and live delivery and no overlap at that boundary. Persist
cursors after processing each envelope, then reconnect, initialize, and
resubscribe using those cursors. Replaying events does not retry mutations; a
client must not blindly resubmit a prompt whose acknowledgement was lost.

Example frames (replace the illustrative conversation IDs with list results):

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","id":2,"method":"_nori/session/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"_nori/session/subscribe","params":{"session_id":"*","cursors":{"<session-a>":42,"<session-b>":17}}}
```

After a disconnect, send the same initialization and subscription with the last
processed cursor for each conversation. A successful new subscription replaces
the connection's previous one. For a single-session subscription, send only
that session's cursor.

Negative, future, unknown-session, mismatched-session, and expired cursors fail
with `-32003`; missing or unknown command targets fail with `-32002`. Expiry is
explicit: a cursor older than `first_available_seq - 1` cannot be recovered
from this journal. To intentionally skip unavailable history, list again and
supply `replay_cursors` for all-session live following, or the chosen row's
`latest_seq` for one session. This is an explicit new observation point, not
recovery of the missing prefix.

`/acp` and `/sessions` share one controller slot across all exact listeners;
last connect wins. Whether future clients can both control, or one can control
while another watches, remains a pending D2 decision. Disconnect or listener
shutdown detaches managed clients without stopping sessions, cancelling admitted turns, or answering pending
permissions. Those permissions remain visible and answerable after reconnect
or from the TUI. A slow managed consumer is dropped when its bounded live queue
fills. The WebSocket adapter closes that connection after draining buffered
events; socket writes also have a deadline. The stdio adapter instead sends
`_nori/session/subscription_ended` with `{"reason":"replay_required"}` after
buffered events and keeps the ACP connection usable. On that notification,
resubscribe using the last processed cursor for each session. Reconnect and
resubscribe share the same cursor validation, so expiry cannot silently lose
history. Stdio encoding or writer failure cannot guarantee an ending
notification. At the exec process boundary, stdin EOF shuts down all managed
sessions.
