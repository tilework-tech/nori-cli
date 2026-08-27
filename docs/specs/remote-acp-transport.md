# Remote ACP Transport

**Status:** Implemented (v1)

**Date:** 2026-08-23 (revised 2026-08-26)

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

This is separate from `nori exec --acp`. That command remains a bounded,
terminal-independent stdio facade. Remote mode exposes the long-lived harness
used by the interactive Nori TUI.

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

## 4. WebSocket contract

The first implementation is WebSocket-only, which the upstream RFD permits
for servers. Streamable HTTP/SSE is not required.

- `GET /acp` with `Upgrade: websocket` opens the connection.
- The upgrade response includes a new `Acp-Connection-Id`.
- `initialize` must be the first JSON-RPC request on the socket.
- Each WebSocket text frame contains one UTF-8 JSON-RPC message.
- Binary frames are ignored.
- WebSocket ping/pong provides liveness; it has no ACP meaning.
- ACP methods, notifications, request IDs, and session IDs retain their normal
  protocol semantics.

The remote surface issues Nori conversation IDs as its ACP session IDs.
Downstream agent session swaps that continue the same conversation (compact,
restore) are invisible to remote clients; the outward session ID never
changes for a continuing conversation. A fork starts a new conversation with
a new ID: the server closes the remote connection, and a reconnecting client
rediscovers the forked session through `session/list`.

The transport adapts WebSocket frames to the same ACP Agent handler used by
the host. It does not introduce a Nori-specific message envelope.

## 5. Detach, reconnect, and close

A WebSocket connection and an ACP session have separate lifetimes.

The server accepts one remote connection at a time across all of its exact
listeners. Those listeners share connection identity state; a newer connection
on any address replaces the current one — last connect wins — and the replaced
socket is closed. Broadcasting one session's stream to several concurrent
remote connections is on the roadmap, not in this version.

- Socket EOF or network loss detaches the remote client. It does not close the
  Nori harness session, stop the downstream agent, or exit the TUI.
- Reconnection creates a new transport connection and
  `Acp-Connection-Id`. The client initializes again, then uses
  `session/resume` or the equivalent supported ACP resume path for the stable
  session ID.
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
and results appear in the existing TUI state.

Opening the microVM terminal therefore reveals the already-running Nori TUI;
it does not reconstruct a second frontend or replace the WebSocket
controller. While a remote controller is connected, the TUI acts as an
observer. An observer sees a turn's `session/update` notifications but not
its stop reason, which travels in the prompt response to the initiator.
Harness commands serialize mutations. Detailed policy for simultaneous local
and remote input is deferred.

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
└── exec/src/lib.rs                        # Existing facade remains unchanged
```

The `--remote` flag lives on the interactive CLI surface and is normalized into
the same TUI-owned manager as runtime commands; the `cli` crate itself remains
unchanged. A remote prompt's own text is not yet rendered by the observing TUI
(its updates, tool calls, and results are); surfacing the initiating message
locally is follow-up work.

## 9. Planned compatibility and lifecycle follow-up

The following items are planned follow-up work rather than guarantees of the
v1 transport:

- A remote prompt can emit an immediate permission request before remote turn
  ownership is registered, silently dropping the request and wedging the turn.
- The TUI and remote controller can both answer the same delegated permission
  request and race contradictory decisions.
- `session/close` can cancel the WebSocket after `SessionEnded` but before its
  JSON-RPC success response is delivered.
- The remote surface rejects mandatory ACP `session/new` requests and instead
  requires clients to discover and attach to the running session through
  `session/list` and `session/load`.
- Initialization echoes unsupported requested protocol versions instead of
  negotiating the latest protocol version the server supports.
- `session/list`, `session/load`, and `session/resume` return success without
  applying required request context such as `cwd`, pagination cursors, and MCP
  server setup.
- An initialized socket receives live session updates before it explicitly
  attaches to the session through `session/load` or `session/resume`.

Authentication, TLS, endpoint discovery, broker routing, Handroll federation,
capability aggregation across hosts, Streamable HTTP/SSE, and reliable replay
after disconnect are outside this spec.
