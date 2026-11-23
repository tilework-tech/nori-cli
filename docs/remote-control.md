# Remote ACP Transport

**Upstream:** [ACP Streamable HTTP & WebSocket Transport RFD](https://agentclientprotocol.com/rfds/streamable-http-websocket-transport)

## 1. Decision

Nori exposes sessions to remote and multi-client frontends only through the
background host, `nori host`. The host owns its sessions in a
`SessionManager` without a terminal and serves them to any number of clients
at once, all reaching the same sessions: over the ACP remote transport, an
authenticated WebSocket at `/acp` with `/sessions` as an alias (section 3),
over stdio (`nori host --stdio`, aliased as `nori exec --acp`, which joins the
running host when it can), and to terminals through `nori attach`. Its standard ACP
face is described in section 12; the `_nori/session/*` extension that the
host and `nori attach` share is described in section 10.

The transport server lives in `nori-acp-host`, which already owns the ACP SDK,
subprocess connection, and wire lifecycle. `nori-acp-host` must not depend on
`nori-harness`: it defines the small `ManagedAgent` interface, and
`nori-harness` implements it over the `SessionManager`, so the dependency
direction stays `nori-harness` → `nori-acp-host`. Remote mutations go through
the manager and harness rather than the downstream `AcpConnection`, so
Nori-owned hooks, transcripts, goals, permissions, and prompt state still apply.

## 2. Topology

```text
Zed or another ACP client
  └─ stdio ─► local Handroll facade
                 └─ authenticated WebSocket /sessions ─► microVM `nori host`
                                                            └─ stdio ACP ─► Codex or another agent

terminal user ─► `nori attach` ─► the same `nori host`
local ACP client ─ stdio ─► `nori host --stdio` ─ relays to ─► the same `nori host`
remote ACP client ─► TLS front (reverse proxy, tunnel) ─► `wss://…/acp` ─► the same `nori host`
```

The local Handroll process lists and flattens sessions across Nori hosts.
WebSocket ACP traffic terminates directly in the Nori host process. A
terminal is one more client of the host; detaching it does not stop sessions.

## 3. ACP remote transport

The background host serves its standard ACP face (section 12) over the
WebSocket profile of the ACP remote transport RFD:

- Accepted RFD, "Streamable HTTP & WebSocket Transport" (Draft; merged in
  [agent-client-protocol#721](https://github.com/agentclientprotocol/agent-client-protocol/pull/721)):
  <https://agentclientprotocol.com/rfds/streamable-http-websocket-transport>
- Open RFD on reconnect, resume and connection resiliency, "Durable &
  resumable HTTP/WS transport":
  [agent-client-protocol#2208](https://github.com/agentclientprotocol/agent-client-protocol/pull/2208)

| RFD requirement | `nori host` |
| --- | --- |
| One endpoint, `/acp` | Served. `/sessions` is an alias with the same behavior. `host.json` and `state.json` publish `ws://127.0.0.1:<port>/acp`. A managed host (`[host] managed = true`) serves no `/acp` and keeps publishing `/sessions` (legacy wire, unchanged). |
| WebSocket profile: `GET /acp` with `Upgrade: websocket`, `101` with `Acp-Connection-Id` | Served. Every upgrade gets a new `Acp-Connection-Id` UUID, beside `x-nori-host-instance`. |
| `initialize` is the first message | Enforced: anything else first gets `-32001` and the connection closes. |
| One JSON-RPC message per text frame; binary frames ignored | Served. |
| No batches: a JSON array gets one `-32600` error with `id: null` | Served; the connection stays open. |
| Streamable HTTP profile (`POST`/`GET` SSE/`DELETE`, `Acp-Session-Id`, HTTP/2) | **Not served.** The RFD lets servers support WebSocket only, and requires every client to support WebSocket. A non-upgrade request gets `426` with `Upgrade: websocket`. |
| Authentication is layered on top (headers, query, or subprotocol) | `Authorization: Bearer <token>` before the upgrade, on loopback too; the token is the one in `host.json`. Missing or wrong: `401`. No query or subprotocol token (a browser renderer goes through a supervisor that adds the header). |
| Sessions survive disconnects; reconnect with `session/load` | Served. Closing one connection only detaches it: sessions, turns and pending permission requests keep running for the host and other clients. A new connection re-`initialize`s, then `session/load` (history, then live) or `session/resume` (live only, standard ACP v1). Pending permission requests are asked again of the controller. |
| In-flight messages are not replayed | Same. Notifications sent while no connection was attached are recovered only through `session/load`'s history replay, not by message. |
| Cookies for sticky routing | Not needed: one host process. The host sets none. |
| Connection and session limits | Not defined by the RFD. `[host] max_clients` (default 16) bounds clients across `/acp` and `/sessions`; one more gets `503` with a message naming the limit. Authentication is checked first. `max_sessions` still bounds sessions (`-32017`). |
| TLS, Origin, bind address | Not defined by the RFD. The host binds `127.0.0.1` only and speaks plain `ws://`. To cross a network boundary, put a TLS-terminating front in front of it (reverse proxy, SSH or Tailscale tunnel) and use `wss://`. Never expose the plain port: the bearer token would travel in clear text. A browser page cannot add the `Authorization` header, so it cannot reach the loopback port. |

The accepted RFD conflicts with itself on disconnects: its WebSocket section
says the server cleans up a connection's sessions on disconnect, its v1
durability section says sessions survive disconnects. The host follows the
second, which is also the host contract's rule that the host outlives every
client.

Deferred to the durable-sessions design (from the open RFD and the
accepted RFD's v2 list): stream cursors (`_meta["acp/cursor"]`, SSE `id:`),
`Last-Event-ID` and WebSocket `stream/resume` replay of missed messages, the
`capabilities.transport.resumable` capability, negotiated keepalive, the
post-resume turn state (`state_update`, `_meta["acp/turnState"]`), a
reconnect grace period, `Acp-Protocol-Version`, and the Streamable HTTP
profile. None of these is on the wire today; the host adds no `_nori` method
or `_meta` key for them.

An earlier, unrelated surface also used `/acp`: the interactive TUI served
its own harness there without authentication, enabled with `nori --remote`
or `/remote-control`, with one last-connect-wins controller. That surface,
its flags and slash command, and the TUI's listener are gone. The TUI opens
no listener; `/acp` is now the background host's authenticated endpoint.

Sections 4–9 described that surface and were removed with it; the numbers
are left unused so that links to sections 10–12 stay stable.

Managed mode (`[host] managed = true`, `/managed`, `_nori/host/*`, and
`nori host --stdio --launch`) is a separate, still-supported surface; see the
[managed launch control contract](../nori-rs/cli/src/host/docs.md#managed-launch-control-v1).

## 10. Managed session wire

The background host serves these `_nori/session/*` commands on its
authenticated `/acp` endpoint and its `/sessions` alias (only `/sessions` on a
managed host) and on its stdio transport (sections 11 and
12); `nori attach` and `nori host ls` are their main clients. The interactive
TUI serves no listener. `ManagedAgent` in
[`acp-host`](../nori-rs/acp-host/src/remote/managed.rs) defines the transport
boundary; [`ManagedRemoteHost`](../nori-rs/harness/src/managed_remote.rs)
implements it over the host's `SessionManager`, keeping mutation and policy in
the harness.

Each WebSocket text frame is a JSON-RPC 2.0 request with an ID and object
params. `initialize` must come first and advertises
`result.capabilities.nori_sessions` with version 1, envelopes, and reconnect.

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

An explicit `session_id` always wins. Omitting it uses the host's default
session, the most recently created one. Subscriptions pin that choice
at subscription time; later commands that omit the ID use the current default.
Use explicit IDs to keep mutations pinned as new sessions start. Only subscribe
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
has been answered.

Keep event payloads as raw JSON when storing or forwarding envelopes. ACP
response types are untagged; interpret a response using its correlated request
method rather than a generic deserialize/serialize round trip, which can choose
an empty response variant and discard fields. The transport serializes the
original manager event without that round trip.

### Delegation

Delegation is a Nori-owned manager operation, available in every session the
session manager runs and shared by
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

The background host applies the controller/watcher policy in section 11.
Disconnect or listener
shutdown detaches managed clients without stopping sessions, cancelling admitted turns, or answering pending
permissions. Those permissions remain visible and answerable after reconnect. A slow managed consumer is dropped when its bounded live queue
fills. The WebSocket adapter closes that connection after draining buffered
events; socket writes also have a deadline. The background host's stdio
transport (section 12) cannot close and reopen, so it instead sends
`_nori/session/subscription_ended` with `{"reason":"replay_required"}` after
buffered events and keeps the stream usable. On that notification,
resubscribe using the last processed cursor for each session. Reconnect and
resubscribe share the same cursor validation, so expiry cannot silently lose
history. Stdio writer failure cannot guarantee an ending notification.

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
| `nori attach` | Open the host session list. |
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

`RemoteAcpServer::bind_host_with_limit` serves `/acp` and its alias
`/sessions` (section 3). A host configured with `[host] managed = true` serves
`/sessions` and the [scoped ACP transport](../nori-rs/acp-host/docs.md#managed-machine-connection) on `/managed` instead, with no `/acp` and no client limit. A valid `Authorization: Bearer <token>` header is required before the
WebSocket upgrade, including on loopback; a missing or wrong token returns
HTTP 401. Past `[host] max_clients` (default 16) concurrent connections the
upgrade returns HTTP 503 with the limit in the body (values below 1 count as
1). Every client counts, including `nori attach`, joined stdio clients and the
short connections of `nori host ls`/`new`/`stop`; a full host still answers
`nori host` with "already running", and `nori host stop` falls back to SIGTERM
on the locked host's PID. The host is the only listener Nori serves.

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
attached watchers --------> authenticated /acp ------> HostService
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
authenticated `/acp` socket (and its `/sessions` alias) with the `_nori/*`
extension, and the same face is also served on stdio (see [Stdio transport](#stdio-transport)). A
connection can work with any number of sessions. Managed mode (`[host] managed = true`)
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

The host's `_meta` keys use the registry prefix `nori/<camelCase>`
(clifford 2026-11-22): `nori/agent` on `session/new` params and `session/list`
rows, and `nori/session` on `session/list` rows; see the extension registry
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
| `session/new` | Any initialized client | `cwd` must be an absolute, existing folder (`-32602` otherwise). The folder's configuration applies. `mcpServers` (stdio and HTTP) are added to the session's MCP servers, and are kept when the client later changes the agent or the session restarts; SSE fails with `-32602`. Optional `_meta["nori/agent"]` starts that agent instead of the folder's default; agent aliases are accepted (an unknown agent fails with `-32602`); otherwise clients switch with the `agent` config option. `[host] default_mode` (`ask`, `agent`, `approve-all`), when set, replaces the folder's approval policy. The response waits until the agent has started and carries `sessionId`, `configOptions` and `modes`; the connection is attached. Capacity fails with `-32017`. An agent that refuses to start fails with its own ACP error code and Nori's explanation as the message, so an adapter needing sign-in returns `-32000` `Authentication required for <agent>.` with the sign-in hint; the session is then left out of `session/list`. |
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
covers the ACP v1 messages above, the `session/list` row and `session/new`
`_meta` objects (`nori/agent`, `nori/session`), the `host.json` file, and the `_nori/*` extension (control, subscribe with its
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

`_nori/session/list`, `new`, `prompt`, `respond`, `cancel`, `close` and
`resume` are deprecated aliases, kept for one more release.
`_nori/session/follow_up` is not an alias: it stays (contract C11). `nori
attach` sends `session/close`, the `session/cancel` notification and
`session/resume` (with the row's folder; an older host that does not report
the folder gets `_nori/session/resume`). It still uses `_nori/session/list`,
`new`, `prompt` and `respond`, and `nori host ls`, `nori host new` and the
attach ID lookup still use `list` and `new`, because the standard methods
lack what they need (row status, family and delegation fields, replay
cursors, prompt admission without waiting for the turn, a first prompt from
an observer). `_nori/agents/list` and `_nori/session/set_approval_policy`
were never released and do not exist.

### Sessions on the host

A session is **live** when the host's manager runs or ran it during this host
process (any status). It is **stored** when it has a transcript under
`$NORI_HOME/transcripts/by-project/*/sessions/*.jsonl` and the host is not
running it. Loading or resuming a stored session makes it live under the same
ID, continuing the same transcript file.

| Method | Who may call | Behavior |
| --- | --- | --- |
| `session/list` | Any initialized client | Live sessions first, then stored sessions, most recently written first. Optional `cwd` (absolute) keeps the sessions working in that folder. Optional `cursor` takes a previous `nextCursor`, which is opaque and absent on the last page. Pages hold 50 rows. Each row is a standard ACP `SessionInfo` (`sessionId`, `cwd`, `title`, `updatedAt` in RFC 3339). Its `_meta` carries `nori/agent` (the agent slug, when known) and `nori/session` `{live}`: `true` while the host runs the session's agent (status starting, working, waiting on you or ready), `false` for a closed, failed or done record and for every stored session. A row with `live: false` restarts when loaded or resumed. A bad cursor or relative `cwd` fails with `-32602`. Unreadable transcripts, and transcripts whose metadata names another session, are left out. |
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

### Stdio transport

`nori host --stdio` without `--launch`, and its alias `nori exec --acp`,
serve one client that speaks this face as newline-delimited JSON-RPC on stdin
and stdout. It is refused when `[host] managed = true`; managed hosts still
need `--launch`. The client either joins the home's running host or gets a
host of its own:

- **Joined.** When the home's `host.json` advertises `contract {acp: 1,
  nori: 1}`, the command has no configuration of its own (no `--agent`,
  `-c`, `--add-dir` or bypass flag), and the client's first line is an
  `initialize` that the running host answers with `control.role:
  "controller"`, the process relays the client to that host over `/acp` with
  the discovery token. The client then shares the host's sessions with every
  other client, and they outlive it. At stdin EOF the process still copies the
  replies to the requests it already forwarded, then disconnects and exits 0
  (a request from the host after EOF, which it can no longer answer, ends that
  wait). SIGINT or SIGTERM disconnects it and exits non-zero. When the host
  closes the connection (it stopped, or closed a client that fell too far
  behind) the process notes it on stderr and exits 0. `_nori/host/stop` stops
  the shared host. A client whose `initialize` sets `observe: true` joins as a
  watcher. Any other client never takes control from another client: when the
  running host answers as a watcher, or cannot be reached (including a stale
  `host.json`), the process closes that connection and falls back to a host of
  its own, replaying the same `initialize` to it. A joined client runs its
  agents with the running host's environment, not the caller's.
- **Own host.** Otherwise the process runs a headless local host for this one
  client. It takes no host lock and writes no `host.json`, `state.json` or
  `token`, so it runs beside a detached `nori host` of the same `NORI_HOME`.
  Its sessions end with it; their transcripts remain as stored sessions.
  Neither host sees the other's live sessions: each lists the other's as
  stored (`live: false`), and loading or resuming one there runs it twice on
  one transcript. Do not.

On its own host:

- Every method above and every `_nori/*` method is served exactly as on an
  authenticated `/acp` connection, any number of sessions on one stream.
- There is no bearer token. The stdio client is always the controller; an
  `observe: true` on `initialize` is ignored.
- stdin EOF or `_nori/host/stop` stops the sessions and exits 0; SIGINT or
  SIGTERM stops them and exits non-zero. A client the face disconnects (for
  example one more than 1024 events behind) cannot reconnect, so the host
  stops its sessions and exits non-zero.
- Blank lines are ignored; a line that is not JSON (including one that is
  not UTF-8) receives `-32700` with a null `id`. stdout carries only ACP lines; logs go to stderr.
- A `_nori/session/subscribe` subscription that falls behind ends with
  `_nori/session/subscription_ended` `{"reason":"replay_required"}` instead of
  a closed connection (section 10).

### One-shot exec as a client

`nori exec "prompt"` and `nori -p` use only this face. When the home's host
holds its lock, `host.json` advertises `contract {acp: 1, nori: 1}`, and
`initialize` (without `observe`) returns `control.role: "controller"`, the
prompt runs on that host over `/acp` with the discovery token; exec never
takes control from another client. Otherwise, and always with `-c` or
`--add-dir`, it runs an in-process host with the same face over in-memory
lines. Each run sends `session/new` (`mcpServers: []`, `_meta["nori/agent"]`
for `--agent`), `session/set_mode` (`approve-all` with the bypass flag, else
`agent`), `session/prompt`, and `session/close`; every permission request is
rejected.

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
  idempotency and generation fencing, and the remote-transport items listed
  in section 3 (cursors, `Last-Event-ID` / `stream/resume`, resumable
  capability, keepalive, turn state, Streamable HTTP). Managed mode keeps its
  current wire.
- `authenticate` / `logout` forwarding and aggregated `authMethods`.
- `ask` behaving differently from `agent` (adapter strictest modes).
- Follow-up (`steer`/`queue`) through `session/prompt`.
- Analytics for the headless host (`nori exec --acp` no longer sends the exec
  `session_mode = acp` event).
