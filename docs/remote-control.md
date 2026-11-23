# Remote ACP Transport

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
session commands described in section 10. The separate `nori host` process
owns sessions without a terminal; `nori attach` is its feature-gated terminal
frontend. Its authenticated endpoint and controller/watcher policy are
described in section 11.
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
the same TUI-owned manager as runtime commands. Background host commands are
dispatched separately by [`cli/src/host`](../nori-rs/cli/src/host/docs.md).
Remote prompts use the same harness-originated user-message
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

These gaps belong to the legacy `/acp` surface, which Phase B of the host
contract retires. The background host's own ACP face (section 12) has none
of them: it negotiates the protocol version, applies `cwd` and paging on
`session/list`, checks `cwd` and accepts `mcpServers` on load and resume,
delivers updates only after a connection attaches, sends permission requests
only to the controller and keeps the first answer, and answers close only
after the session has ended.

The legacy `--remote` surface remains unauthenticated. The background host
requires a token and supports same-machine discovery (section 11). TLS
termination, broker routing, Handroll federation, capability aggregation across
hosts, Streamable HTTP/SSE, and replay across a host-process restart remain
outside this implementation. Legacy `/acp` retains
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
| `_nori/session/delegate` | Optional parent `session_id`, `agent`, `task` | Delegation record |
| `_nori/session/delegation/resolve` | Optional parent `session_id`, delegation `request_id`, `approved` boolean | Updated delegation record |

A list row reports identity, agent, status, `family_status` (the most urgent
status across the row's whole session family, rooted at its top-level session),
cwd, lineage, pending request IDs, `source`, delegation `depth`, the parent's `delegations`, sequence bounds (`latest_seq`, `first_available_seq`), age in seconds, and a
title derived from observed session activity (the agent's `session_info_update`
title or the prompt summary, whichever came last; without one, or once the
agent clears its title, the first prompt's first line). Rows
also carry additive display fields that older hosts omit: `reply_preview` (the
start of the agent's latest reply, up to 200 characters on one line),
`waiting_request` (the oldest open permission request or delegation awaiting
approval, as `approve: <subject>` or `approve: start <agent> · <task>`; lists
show it instead of the reply on a row whose own status is `waiting_on_you`),
`model` (the display name of the agent's model option), `idle_seconds` (since
the session's latest event), and `context_used` / `context_size` (from the
agent's latest `usage_update`). `replay_cursors` includes every
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
sessions return `-32015` for `_nori/session/prompt`. The authenticated background host additionally accepts an explicit [mid-turn follow-up](../nori-rs/cli/src/host/docs.md#mid-turn-follow-ups) from its controller. Permission replies
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

### Delegation

Delegation is a Nori-owned manager operation behind `multi_session`, shared by
remote controllers, the local `/delegate <agent> <task>` command, and the
session-bound `delegate` tool on Nori's authenticated HTTP `nori-client` MCP
server. This puts approval, session capacity, and child lifetime under the
same authority even when no terminal is attached. The MCP tool accepts
`agent` and `task`; its parent identity is bound by Nori to the calling session.
Agents without HTTP MCP support retain user-driven `/delegate`, but have no
stdio agent-tool fallback.

A delegation record contains `request_id`, `parent_id`, `child_id`,
`parent_agent`, `agent`, `task`, `depth`, `status`, optional `result`, and
`origin`. `origin` is `agent` when the parent agent called the MCP `delegate`
tool and `user` when the request came through the manager directly (`/delegate`
or `_nori/session/delegate`); records from hosts that predate the field
deserialize as `agent`. Clients use it only to word who asked for the child.
The child conversation ID is reserved before approval. The statuses are
`awaiting_approval`, `running`, `declined`, `completed`, `failed`, and
`cancelled`; these describe delegation state, separately from session status.
`DelegationChanged` Nori events carry the record through ordered manager
envelopes. Lists expose current records for recovering approval presentation.
All-session subscribers also receive the child's normal envelopes; a
parent-only subscription does not become a subscription to every descendant.

Same-kind children start immediately. A different-kind request returns
`awaiting_approval`; resolve it for the requesting parent and request ID with
`approved: true` or `approved: false` (the terminals label these
"Yes, start it" and "No, decline"). Only the controller may
request or resolve delegation. Unknown, resolved, or wrongly attributed
requests fail rather than starting another child. Approval does not reserve a
process indefinitely: actual startup must pass the shared session cap. Depth
is measured in delegation edges, separately from forks. Children may delegate
one level further; a request deeper than two levels below the top session
returns an error to its requester. Branch-at-head transfers parent or child
ownership to the process's new conversation without increasing delegation
depth. The same delegation request ID remains valid, while `DelegationChanged`
publishes updated attribution; pending approvals and result delivery follow
the new parent identity.

A child's ACP permission request belongs to that child's conversation and
request ID. Answer it after selecting the child, with Alt+Down in the terminal
for next waiting session. Instead of opening the child's permission popup in
the parent, the parent receives a notice that names both sessions readably:
`<agent> · <child name> is waiting on you (delegated by <parent agent> · <parent
name>). Press Alt+Down to answer it.` The child name is the first non-empty
line of the task (else `new session`) and the parent name is its recorded
title, each cut to 40 characters in the middle (start, `…`, end) so names
sharing a long prefix stay distinct. A completed child's result is automatically queued as attributed
input to the parent, including when the parent is busy. That prompt's first
line is `[Nori delegation result] from <agent> · <child name> · <outcome>`
(`done`, `failed`, `declined`, or `cancelled`; the child name here is the
task's whole first line), its second is
`Child session: <child id>`; a delegation the user started with `/delegate`
then adds `The user started this delegation with /delegate; you did not
request it.` so the parent agent does not deny starting it; the rest is the child's result text (its last
turn's message text, with a paragraph break wherever a tool call, permission
request, or other update interrupted it; for a crashed child, the first line of
its failure). A declined delegation never started a child, so its prompt has no
`Child session:` line. The parent's own ID is not included, and the prompt
never becomes the parent's title. The manager retains delivery until that parent's correlated
ACP prompt response, rather than treating queue admission as acknowledgment.
If the parent process crashes before acknowledgment, stable-ID resume submits
the result again. A delegated child with descendants waits for them and its
result-driven continuation before reporting its final result. The child process
then ends and frees its cap slot; its record and transcript remain inspectable.

Explicit parent close cancels descendants and outstanding approvals, even if
the parent's process has already failed. Parent crash preserves running
children and holds results until stable-ID resume (Ctrl+R). Child failure
returns a failure result without failing the parent. This state belongs to the
current manager lifetime; host restart does not restore it from transcripts.
After a restart a finished child is not listed by `nori host ls` or
`nori attach`, and `/resume` on its transcript reopens it as a live top-level
`ready` session without its parent link.

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

On the legacy `--remote` surface, `/acp` and `/sessions` share one controller
slot across all exact listeners; last connect wins. The background host uses
the controller/watcher policy below instead. Disconnect or listener
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

## 11. Background host and attached terminals

This section covers the default local host. Machine adapters that opt into
`[host] managed = true` use the [managed launch control contract](../nori-rs/cli/src/host/docs.md#managed-launch-control-v1).

`nori host` starts a detached Unix process with an initially empty
`SessionManager`. The host owns the agent processes and remains alive when
every terminal detaches or the last session ends. It binds only loopback,
defaults to port 7420, and accepts `--port 0` to allocate a port. `nori host stop`
authenticates, requests shutdown, and waits for the host's singleton lock to be
released after cleanup. Startup and stop do not require a terminal.

| Command | Behavior |
| --- | --- |
| `nori host` | Start the background host, or report the already-running host. |
| `nori host ls` | List host sessions with delegated children under their parent and `├─`/`└─`/`│` tree guides inside the title column. Top-level rows show their `family_status` when the host sends it. On a TTY it prints the same two-row layout as the attached and local session lists (no ID column): status glyph, title, agent, status, and context, then a faint row with the session's short folder (`~/rel` up to 20 characters, else the folder name) and ` · ` before the reply start (or, while waiting on you, what the session asks for), model, and time since last activity; status words are colored (waiting on you yellow, working cyan, ready green, failed red, done in the default color, starting and closed dimmed) and every other session is shaded when stdin and stdout are terminals that report their background and support 24-bit colour. Piped output is one plain `ID AGENT STATUS AGE CWD TITLE` row per session with full values (CWD is the full canonical folder), no color, and `  (parent <full id>)` on every row with a parent, including a child whose parent is no longer listed and so prints at top level. |
| `nori host ls --json` | Print a `sessions` array from the authenticated host. Each row contains the stable `session_id` returned by `host new` or managed ensure and the binding’s public `credential_refs` labels. Ordinary local sessions have an empty label list. |
| `nori attach` | Open the host session list; requires `[features] multi_session = true`. |
| `nori attach <id>` | Open the named Nori conversation. Any unique start of the ID works, and a full ID may be typed in any case, with or without hyphens: the CLI resolves it against `_nori/session/list` (3 s timeout) before the terminal opens, lists the candidates (ID, agent, title) when several match, and points at `nori host ls \| cat` when none does. |
| `nori attach --host <URL> --token-file <path>` | Connect to an explicit managed endpoint using a private token file. |
| `nori host new [--cwd DIR] [--agent AGENT] [--prompt TEXT] [--json]` | Start a session on the running host (local, or `--host <URL> --token-file <path>`) and print its ID, or the `{session_id, cwd}` reply with `--json`. `--cwd` resolves against the caller's directory, and the host checks the folder exists on its machine. Without `--cwd` the local host uses the caller's directory and a `--host` host uses its own folder. Refuses a host that does not advertise `session_cwd`. |
| `nori host stop` | Stop the listener and all host-owned sessions. |

The CLI stores discovery state, token, log, and lock under `$NORI_HOME/host`.
The directory is created with mode `0700`; files are created with mode `0600`.
Existing paths must be owned by the effective user and inaccessible to group
and other users; files must be regular and cannot be symlinks. An OS-held lock,
not the saved PID, determines whether the host is running. Each host start
generates a new token and publishes its endpoint only after binding. Local
commands read that token automatically; startup prints its path, never its
value. Background hosting is refused on platforms without the implemented Unix
permission checks.

`RemoteAcpServer::bind_host` serves `/sessions`. A host configured with
`[host] managed = true` also serves the [scoped ACP transport](../nori-rs/acp-host/docs.md#managed-machine-connection) on `/managed`. A valid `Authorization: Bearer <token>` header is required before the
WebSocket upgrade, including on loopback; a missing or wrong token returns
HTTP 401. This policy applies to `nori host`, not the separate `--remote` or
`/remote-control` listeners.

Successful authenticated upgrades include `x-nori-host-instance`, a UUID
generated for that router lifetime independently of the bearer token. It stays
stable across connections and changes when a new host router starts, even if
the address and token are reused. Reconnecting clients check this identity
before initialization or replay so an old cursor cannot silently target a new
manager.

The terminal table budgets display cells, preserving grapheme boundaries and
adding an ellipsis when a title exceeds the remaining width; it cuts the status
glyph and title together at the same point the TUI session lists do. If the terminal
cannot fit the metadata columns, the row prefix is clipped too. Piped output
has no width limit; control characters are sanitized in either mode.

```text
attached controller -- commands --+
                                 v
attached watchers --------> authenticated /sessions --> HostService
       ^                         |                         |
       |                         +-- role changes          v
       +---------- ordered replay/live events ------ SessionManager
                                                           |
                                                     ACP agent processes
```

### Control and lifecycle requests

The first successful interactive `initialize` claims control when the slot is
empty. Later clients remain connected as watchers. `initialize` with
`{"observe":true}` never claims the slot, so polling `host ls` and running
administrative `new` and stop do not take control. Initialization returns
`result.control` containing `role` and `controller_connected`, and the host
adds `session_cwd: true` to `result.capabilities.nori_sessions` to advertise
per-session folders. Clients send `cwd` only when it is advertised.

| Method | Meaning |
| --- | --- |
| `_nori/control/status` | Return this connection's role and whether a controller is connected. |
| `_nori/control/take` | Transfer control to the caller without closing another connection. |
| `_nori/session/new` | Start a session; any authenticated initialized client may call it, since it changes no existing session; the new session becomes the host's selected session. Optional `cwd` (absolute; canonicalized, and refused when relative, missing, or not a directory) loads the host's configuration in that folder with the host's command-line overrides, so the folder's own project trust and approval settings apply; without it the host's start configuration is used. Optional `agent` selects the agent; optional `prompt` is sent as the first prompt after the session starts, without delaying the reply. Replies `{session_id, cwd}`. Resume reloads configuration from the record's cwd the same way. |
| `_nori/session/close`, `_nori/session/cancel`, `_nori/session/resume` | Apply the operation to the required `session_id`; resume retains the Nori conversation ID. Resume also accepts the ID of a [stored session](#12-standard-acp-v1-on-the-background-host) and restarts it from its transcript. On the background host, `_nori/session/list` and `_nori/session/resume` are aliases kept for `nori attach` and `nori host ls`; new clients use `session/list` and `session/resume`. |
| `_nori/host/stop` | Request host shutdown; any authenticated initialized client may call it. |

`_nori/control/changed` notifications carry the new `role` to terminals affected
by a handoff. The admission mutex serializes the ownership check with handoff,
then releases before awaiting the backend. An already-admitted operation may
finish after control moves; new mutations from the previous controller are
refused. A slow operation on one connection does not hold up another client's
initialization, list, takeover, or stop. Watchers can list, subscribe, start new
sessions, stop the host, and take control; other session mutations, including prompts and permission answers, fail with
`-32016` and a read-only error. When the controller disconnects, existing
watchers are not automatically promoted. They may take control explicitly, or
a new interactive initializer may claim the empty slot.

### Attached frontend and recovery

The dedicated [`attach` frontend](../nori-rs/tui/src/attach/docs.md) uses the
managed client through harness re-exports. It owns terminal state and replay
cursors, never the host's agent processes. Alt+T takes control; Ctrl+D or
`/quit` detaches. The configured `hotkeys.open_session_list` binding, Left with
an empty composer, or `/sessions` opens the session list. Ctrl+R resumes its
highlighted failed session with the same conversation ID. The status line shows
controlling, watching/read-only, reconnecting, or disconnected state. The
list uses the shared two-row session layout (title, agent, status, context;
then reply start, model, time since last activity) and marks the current
session inside the Session column. Its keys match the other pickers: arrows
and `j`/`k` move, `/`, `f`, or Ctrl+F search, and Esc leaves a search; while
the composer holds text, only arrows, PageUp/PageDown, and Ctrl keys act on
the list.
Session commands address the host; they do not launch local agents.
`/new [DIR] [AGENT]` (which still requires control in the terminal) starts a
session in the attaching terminal's directory (the host's own folder for a `--host` host), or in `DIR`, made absolute
first. A lone argument is a folder when it starts with `/`, `~`, or `.`,
contains `/`, or names an existing folder under the attaching directory;
otherwise it is an agent. Tab completes the folder against the attaching
machine's filesystem like a shell, listing several matches in the notice
line. The identity line reads `agent · title · folder · status`, and list
rows show the short folder before the reply.
`/delegate <agent> <task>` is the delegation entry point; the list does not
start delegations. The configured `hotkeys.next_waiting_session` binding
(Alt+Down by default) selects the next waiting child or other session, even
with an unsent draft (with nothing waiting, a draft keeps the key for the
composer), and
`hotkeys.parent_session` (Alt+Up by default) returns from a delegated child to
its parent. Both the attached list and `nori host ls` order children directly
under their parent and draw tree guides inside the title column, so the other
columns stay aligned, and both show a top-level row's `family_status`.
While a permission prompt or delegation approval shows for the on-screen
session, the composer and its cursor are hidden and only request keys and
PageUp/PageDown act. Esc rejects (a permission's reject-once option, else
reject-always, else `cancelled`; a delegation is declined); one Ctrl+C cancels (permission outcome
`cancelled`; a delegation is declined). The draft is kept either way.
Option keys (digits, Enter, plain letters other than `j`/`k`) are ignored until a request has
been on screen for a short grace period (400 ms), so a key typed into the
composer just as the request appears does not answer it; Esc, Ctrl+C,
arrows, and `j`/`k` act at once. The local TUI's approval overlay and `/delegate` approval
prompt follow the same rules, and its overlay's Esc picks the same reject option.
Each session keeps its own unsent draft: switching sessions parks the
composer text with the session it was typed in and restores the target's.
The identity line names the selected session; a top-level session shows its
family status, as the lists do. Inside a delegated child (not a fork), it reads
as a breadcrumb of agents and names from the top-level session down to the
child. Both are fitted to the terminal width: the parent key gives way before
the current name is cut, and names are cut in the middle. A second line names
the next other session waiting on you, under its parent, with the jump key. A
delegated child whose task finished lists as `done` (default color); `closed`
(muted) means a session the user closed or shut down. A child's history ends
with `This child finished its task` or `This child session ended`, plus the
parent key. An attached parent shows a delivered result prompt as one
`› delegation result from <agent> · <name> · <status> · delivered to this
session` line, which drops the delivered suffix and then cuts the name to fit
one row. A running child's delegation card reads `started`, because cards are
printed once and do not update.

Socket loss or a receive-liveness deadline starts recovery. Periodic session
list requests keep idle connections observable; receiving JSON renews the
deadline, while outgoing traffic alone does not. The deadline also bounds
blocked sends. The frontend retains its rendered sessions, reconnects, verifies
the host-instance identity, initializes again, and subscribes to the selected
conversation with its last applied cursor. Switching conversations uses that conversation's saved cursor;
the session list refreshes independently without subscribing to every session.
It advances a cursor only after applying that envelope and ignores duplicates.
Transcript restoration is distinct: `ReplayStarted` replaces the rendered
conversation and pending permissions without resetting transport sequence
tracking. Permission prompts also clear when the turn becomes idle or begins
cancelling, the session ends, or a matching answer is resolved. The frontend
does not retransmit mutations whose acknowledgement may have been lost.
Replay expiry or an unexpected sequence gap is visible as an error; the
frontend does not silently jump to live output. Replay is bounded to the
current host process lifetime, so a fresh attachment may also encounter an
expired prefix. Restarting a host does not restore its in-memory manager.

On HTTP 401 or 403 during reconnect, a locally discovered attachment rereads
the current token file using the same ownership, permission, and regular-file
checks as startup, then retries once. An explicit `--host` attachment never
rediscovers local credentials. An unavailable token, failed retry, or changed
or unavailable host identity stops recovery and leaves a persistent message
in the terminal. A confirmed identity change reports that the host restarted
and its sessions were lost; authentication failures report that the host may
have restarted. The rendered history and unsent draft remain visible. Detach
with Ctrl+D and rerun `nori attach` to connect afresh.

## 12. Standard ACP v1 on the background host

The normative contract for `nori host` is
`docs/specs/nori-native/host-contract.md` in the workspace monorepo. This
section records what the CLI implements. Standard ACP v1 methods share the
authenticated `/sessions` socket with the `_nori/*` extension. A connection
can work with any number of sessions. Managed mode (`[host] managed = true`)
does not serve these methods: its supervisor wire (`_nori/session/ensure`,
`_nori/session/status`, `_nori/host/*`, `/managed`, `--stdio --launch`) is
unchanged and stays as legacy until durable sessions get their own design.

### Handshake and discovery

`initialize` answers with:

- `protocolVersion: 1`, whatever the client asked for (the host speaks only
  ACP v1; a client that cannot speak it disconnects);
- `agentInfo` `{name: "nori-host", version}`;
- `agentCapabilities`: `loadSession: true`,
  `sessionCapabilities` with `list`, `resume`, `close` and `delete`, and the
  ACP baseline for prompts and MCP servers (no image, audio, embedded context,
  HTTP or SSE promises, because those depend on each session's adapter);
- `authMethods: []` (see deferred items below);
- no Nori `_meta` key. The contract version is in `host.json`; a client
  without it derives the version from `agentInfo.version`.

The host's only new `_meta` key is `nori/agent` (registry prefix `nori/<camelCase>`, clifford 2026-11-22) (on `session/new` params
and `session/list` rows); see the extension registry
(`docs/acp-extensions.md` in the workspace monorepo).
The `_nori/*` methods are unchanged, including `_nori/session/follow_up`
and `_nori/session/subscribe`.

The existing keys (`capabilities.nori_sessions`, `server_info`, `control`)
stay in the same result because `nori attach` reads them. `params.observe =
true` keeps a client from taking control, as before.

After binding, the host writes `$NORI_HOME/host/host.json` (mode `0600`,
written to a temporary file and renamed) beside the existing `state.json`
and `token`, and removes it on stop:
`{endpoint, token, pid, instanceId, version, contract: {acp: 1, nori: 1}, startedBy}`.
`startedBy` is `NORI_HOST_STARTED_BY` when set, else `nori-cli`. Managed hosts
do not write it.

### Sessions and turns

| Method | Who may call | Behavior |
| --- | --- | --- |
| `session/new` | Any initialized client | `cwd` must be an absolute, existing folder (`-32602` otherwise). The folder's configuration applies. `mcpServers` (stdio and HTTP) are added to the session's MCP servers, and are kept when the client later changes the agent or the session restarts; SSE fails with `-32602`. Optional `_meta["nori/agent"]` starts that agent instead of the folder's default (an unknown agent fails with `-32602`); otherwise clients switch with the `agent` config option. `[host] default_mode` (`ask`, `agent`, `approve-all`), when set, replaces the folder's approval policy. The response waits until the agent has started and carries `sessionId`, `configOptions` and `modes`; the connection is attached. Capacity fails with `-32017`. An agent that refuses to start fails with its own ACP error, so an adapter needing sign-in returns `-32000` `Authentication required`; the session is then left out of `session/list`. |
| `session/prompt` | Controller | Runs one turn and answers with the agent's `PromptResponse` (`stopReason`). The connection receives the turn's updates before the response. A prompt while a turn runs fails with `-32015`; it is never queued. A connection that is not attached to the session is attached (as by `session/resume`) first. |
| `session/cancel` | Controller (notification) | Cancels the session's turn; the prompt answers with `stopReason: "cancelled"`. Ignored from watchers. |
| `session/close` | Controller | Stops the session's agent and frees its slot. It stays in `session/list` as `stored` (status `closed`), and `session/load` or `session/resume` restarts it under its ID. |
| `session/delete` | Controller | Closes the session if it runs, removes its transcript under `$NORI_HOME`, and leaves it out of `session/list` and load for the rest of this host's life. Nothing outside `$NORI_HOME` is touched. |

The connection that controls the host receives `session/request_permission`
requests for every session it is attached to, with `sessionId` set to the
conversation ID and a host-chosen string ID. Watchers never receive them.
Requests in replayed history are not asked: a controller that attaches
later, or takes control with `_nori/control/take`, is asked only about
requests the session still waits on. Only the current controller's answer is
used; the first answer (including one through the legacy
`_nori/session/respond`) wins and later ones are ignored. An error response
counts as `cancelled`. Under the
`approve-all` mode the host answers itself and no client is asked.

### Errors from the agent

When the agent fails to start, every command waiting for it (a queued prompt
or follow-up) is answered with that failure and the agent's ACP error code
instead of being dropped. `session/new` and the legacy `_nori/session/prompt`
therefore report `-32000 … Authentication required` for an adapter that needs
sign-in, not `ACP agent did not accept the prompt`.

### Schema and TypeScript types

`nori-rs/nori-protocol/schema/host-wire.schema.json` (JSON Schema 2020-12)
covers the ACP v1 messages above, the `nori/agent`
`_meta` object, the `host.json` file, and the `_nori/*` extension (control, subscribe with its
`SessionEnvelope`, host stop, delegation). `nori-protocol/typescript/host-wire.ts`
is generated from it by a pinned `json-schema-to-typescript`. Regenerate both
with `scripts/generate-host-wire-types.sh`; `cargo test -p nori-cli --test
host_wire_schema` fails when either drifts (the TypeScript half is skipped
without Node.js).

### The `_nori/*` extension on the background host

| Family | Methods |
| --- | --- |
| `subscribe` | `_nori/session/subscribe` and its `_nori/session/event` envelopes, unchanged (section 10) |
| `control` | `_nori/control/status`, `_nori/control/take`, `_nori/control/changed` |
| `host` | `_nori/host/stop` |
| `delegation` | `_nori/session/delegate`, `_nori/session/delegation/resolve` |

`_nori/session/list`, `new`, `prompt`, `respond`, `cancel`, `close`,
`resume` and `follow_up` remain as aliases for one release, because
`nori attach`, `nori host ls`, `nori host new` and the attach ID lookup still
use them; Phase B moves those clients onto ACP. `_nori/agents/list` and
`_nori/session/set_approval_policy` were never released and do not exist.

### Sessions on the host

A session is **live** when the host's manager runs or ran it during this host
process (any status). It is **stored** when it has a transcript under
`$NORI_HOME/transcripts/by-project/*/sessions/*.jsonl` and the host is not
running it. Loading or resuming a stored session makes it live under the same
ID, continuing the same transcript file.

| Method | Who may call | Behavior |
| --- | --- | --- |
| `session/list` | Any initialized client | Live sessions first, then stored sessions, most recently written first. Optional `cwd` (absolute) keeps the sessions working in that folder. Optional `cursor` takes a previous `nextCursor`, which is opaque and absent on the last page. Pages hold 50 rows. Each row is a standard ACP `SessionInfo` (`sessionId`, `cwd`, `title`, `updatedAt` in RFC 3339). Its `_meta` carries `nori/agent` (the agent slug, when known). A closed, failed or stored session restarts when loaded or resumed; whether a row is live is not reported yet (pending a decision on reusing the broker's `nori.dev/session` row object). A bad cursor or relative `cwd` fails with `-32602`. Unreadable transcripts, and transcripts whose metadata names another session, are left out. |
| `session/load` | Any initialized client | Attaches the connection to the session. Its history arrives as `session/update` notifications **before** the response, then live updates follow. A live session replays from the manager journal (sequence 1 onward) with no gap before live delivery. If the journal has evicted the session's start, the transcript is replayed instead and live delivery starts at the journal's latest event, so updates in that interval can be missed. A stored session replays its transcript, then restarts on a new agent process in its recorded folder with its recorded agent; the response waits until the session has started. A failed live session restarts the same way. |
| `session/resume` | Any initialized client | Like `session/load` without history: only updates after the response arrive. History the harness replays to the restarted agent is not forwarded. |

`sessionId` is the Nori conversation ID. A `sessionId` that is neither live
nor stored fails with `-32002`. `cwd` must be absolute and name the session's
own folder; a different folder fails with `-32602` rather than moving the
session. `mcpServers` is accepted and ignored for sessions that are already
configured.

Attached connections receive `session/update` notifications whose
`sessionId` is the conversation ID. Several connections can attach to
one session, and each sees every update. Attachment ends when the connection
closes. A connection that falls more than 1024 events behind a session is
closed; reconnect and load again.

### Session configuration

`session/load` and `session/resume` return the session's `configOptions` and
`modes`. Every session's `configOptions` contain, in this order:

| ID | Category | Values |
| --- | --- | --- |
| `agent` | `_nori_agent` | Every agent the host can start (built-in and `[[agents]]`). Each value's `description` is `Installed` when its adapter command resolves on the host, else says the adapter is not installed and how to install it. |
| the adapter's options | as the adapter reports them | Passed through, including its `model` select. Adapter options with ID `agent` or `mode`, or with category `mode`, are left out: the host owns those. |
| `mode` | `mode` | `ask`, `agent`, `approve-all` |

Permission modes are also the ACP session modes (`currentModeId`,
`availableModes`) and stay in sync with the `mode` option:

| Mode | Label | `approval_policy` | Behavior today |
| --- | --- | --- | --- |
| `ask` | Ask | `untrusted` | As `agent`: Nori only sees the permission requests an adapter sends. Mapping onto the adapter's strictest mode is planned (contract Phase C). |
| `agent` | Agent | `on-request` | Permission requests go to clients. |
| `approve-all` | Approve all | `never` | Nori answers every permission request with its first allow option. |

A session's mode starts from the `approval_policy` its folder's configuration
sets (`on-failure` reads as `agent`).

| Method | Who may call | Behavior |
| --- | --- | --- |
| `session/set_mode` | Controller | Sets the mode. Attached connections receive `current_mode_update` and a `config_option_update` with the complete list. An unknown mode fails with `-32602` and changes nothing. |
| `session/set_config_option` | Controller | `mode` acts as `session/set_mode`. `agent` replaces the adapter process under the same session ID while the session has had no prompt; afterwards, or for an unknown agent, it fails with `-32602`. Any other ID goes to the adapter; a refused value fails with `-32602`. Replies `{configOptions}` with the complete list, and attached connections receive the same list as `config_option_update`. |

An adapter's own `config_option_update` reaches clients as the complete list,
host options included. Its own `current_mode_update` is not forwarded.
Watchers get `-32016` from both methods.

The mode, every adapter option a client sets, and the `mcpServers` a
session was created with are kept in `$NORI_HOME/host/session-settings.json`
(mode `0600`). Restarting, changing the agent of, and deleting a session are
serialized, so two clients loading one stored session start it once. When a failed or stored session
restarts, its mode becomes the starting approval policy, its `model` value
becomes the adapter's starting model, and the other options are set again
once the adapter has started. Changing the agent clears the remembered
adapter options.

### Deferred

- Durable sessions: checkpoints, acknowledgements, durable replay,
  incremental reconnect cursors on ACP updates, import/release, command-ID
  idempotency and generation fencing. Managed mode keeps its current wire.
- `authenticate` / `logout` forwarding and aggregated `authMethods`.
- `ask` behaving differently from `agent` (adapter strictest modes).
- Follow-up (`steer`/`queue`) through `session/prompt`.
- `nori host --stdio` without `--launch`.
