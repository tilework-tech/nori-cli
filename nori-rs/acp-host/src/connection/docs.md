# Noridoc: connection

Path: @/nori-rs/acp-host/src/connection

### Overview

This module implements the host side of an ACP stdio connection. It launches
the agent, performs the SDK handshake, owns the child lifecycle, and preserves
ACP messages in source order for the harness.

### How it fits into the larger codebase

`AcpConnection` is created by `nori-harness` session setup. The connection uses
the ACP SDK for transport, imports all schema values through
`nori_protocol::acp`, and reports through one `ConnectionEvent` inbox.

### Core Implementation

`ConnectionEvent` separates public ACP traffic from private host/runtime
signals:

- `Acp(AcpEvent)` carries raw notifications and correlated responses.
- `DelegatedRequest` pairs a raw `AgentRequest` and `RequestId` with its private
  responder.
- `SessionUpdate` is a private reducer input paired with the preceding raw ACP
  notification.
- `SessionClosed` records a successful ACP `session/close`.
- `ChildExited` reports process status and a bounded stderr tail.

`AcpConnection` exposes ACP initialize/session creation or loading, prompt,
cancel, session config, list, resume, fork, close, and shutdown behavior. Method
responses are published as raw `AcpEvent::Response` values; request IDs are
assigned by the SDK transport and retained end to end. Each call to `prompt`
issues one `session/prompt` request. A later response is never swallowed to
justify resending the prompt after cancellation; a successful empty `EndTurn`
is returned as that prompt's terminal result.

`fork_session` issues the unstable ACP `session/fork` request to branch a
session at its current head. Unlike `session/load` and `session/resume`, which
echo the input session id, fork returns the NEW forked session id taken from the
response body; the harness swaps its active session to it via
`swap_active_session` (see `@/nori-rs/harness/src/backend/session_swap.rs`). This
method requires the SDK `unstable` feature, enabled in `acp-host/Cargo.toml`.

The default spawn path publishes initialize on the ordered connection inbox.
The harness uses the opt-in initialize sink so the same raw response survives a
failed constructor; the two paths are exclusive, so successful initialization
is never duplicated.

The SDK handlers delegate permission requests but handle filesystem read/write
inside the host. File writes are restricted to the workspace or `/tmp`; reads
remain unrestricted. Host-handled requests do not leak duplicate public
`AcpEvent::Request` values.

### Things to Know

- The child environment is layered: `AcpAgentConfig.env` from the registry is
  applied first, then `spawn()` mutates individual vars on top. That ordering is
  load-bearing for the Claude model-list override
  (`@/nori-rs/acp-host/src/claude_models/docs.md`), which replaces the
  registry's `CLAUDE_CODE_EXECUTABLE` with a wrapper around that same binary. It
  requires `AgentKind::ClaudeCode`, that var already being present, a resolvable
  `NORI_HOME`, and a unix host; anything missing skips it and leaves the agent
  untouched.
- `spawn()` therefore performs network I/O before the subprocess exists. The
  requests are issued concurrently under a short timeout, so an unreachable host
  adds at most one timeout to session launch and never blocks startup.
- All event producers share one ordered inbox, including child exit. Do not
  split public ACP traffic into racing channels.
- Unix spawn makes the direct child its process-group leader, so the child PID
  is also the known process-group ID retained for teardown. The child watcher
  owns the `Child`, observes leader exit with `waitid(WNOWAIT)`, signals that
  stored group directly to sweep descendants, and only then reaps the leader;
  it must not try to rediscover the group through an exited leader.
- Startup races initialization against early child exit so authentication and
  spawn failures retain the child's stderr explanation.
- Orderly `session/close` is distinct from connection loss. The harness maps
  them to `SessionEnded(Closed)` and `SessionEnded(ConnectionLost)`
  respectively.
- The connection uses `ProtocolVersion::LATEST` while enforcing ACP v1 as the
  minimum supported version.

Created and maintained by Nori.
