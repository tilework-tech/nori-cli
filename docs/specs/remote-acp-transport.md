# Remote ACP Transport

**Status:** Draft

**Date:** 2026-08-23

**Upstream:** [ACP Streamable HTTP & WebSocket Transport RFD](https://agentclientprotocol.com/rfds/streamable-http-websocket-transport)

## 1. Decision

Nori CLI will optionally expose its running harness as an ACP Agent over the
WebSocket profile of the upstream remote transport RFD.

The server belongs in `nori-acp-host`, which already owns the ACP SDK,
subprocess connection, and wire lifecycle. It is disabled unless remote mode
is explicitly enabled. No separate session-host crate is introduced.

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

The transport adapts WebSocket frames to the same ACP Agent handler used by
the host. It does not introduce a Nori-specific message envelope.

## 5. Detach, reconnect, and close

A WebSocket connection and an ACP session have separate lifetimes.

- Socket EOF or network loss detaches the remote client. It does not close the
  Nori harness session, stop the downstream agent, or exit the TUI.
- Reconnection creates a new transport connection and
  `Acp-Connection-Id`. The client initializes again, then uses
  `session/resume` or the equivalent supported ACP resume path for the stable
  session ID.
- `session/close` is terminal for the selected harness session.
- Foregrounding or detaching the Handroll-owned terminal does not affect the
  WebSocket connection.

The first version follows the RFD's v1 reliability model: no sequence numbers,
no replay of messages missed while disconnected, and no transparent retry of
an in-flight JSON-RPC request. The harness and transcript continue recording
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
controller. Harness commands serialize mutations. Detailed policy for
simultaneous local and remote input is deferred.

## 7. Implementation boundary

```text
nori-rs/
├── acp-host/
│   ├── Cargo.toml                         # Optional remote transport deps
│   └── src/
│       ├── lib.rs                        # Feature/runtime entrypoint
│       └── remote/
│           ├── mod.rs                    # Public server API
│           ├── hosted_agent.rs           # Downward-facing control interface
│           ├── server.rs                 # Listener and WS upgrade
│           ├── connection.rs             # Initialize and controller state
│           └── wire.rs                   # Frames, queues, ping/pong
├── harness/src/
│   ├── runtime.rs                        # Subscribable SessionEvent fan-out
│   └── remote_agent.rs                   # HostedAgent over HarnessHandle
├── tui/src/chatwidget/agent.rs            # Share the launched harness
├── cli/src/
│   ├── main.rs                           # Remote-mode activation
│   └── remote.rs                         # Listener configuration and wiring
└── exec/src/lib.rs                        # Existing facade remains unchanged
```

Authentication, TLS, endpoint discovery, broker routing, Handroll federation,
capability aggregation across hosts, Streamable HTTP/SSE, and reliable replay
after disconnect are outside this spec.
