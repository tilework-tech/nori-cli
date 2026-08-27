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
assigned by the SDK transport and retained end to end. A failed session-config
write is the deliberate exception: its error is returned to the caller but not
mirrored onto the public event boundary, where the caller's friendly failure
handling would otherwise produce a duplicate raw error. Each call to `prompt`
issues one `session/prompt` request; `prompt_with_request_id` preserves optional
top-level prompt metadata on that request. A later response is never swallowed
to justify resending the prompt after cancellation; a successful empty
`EndTurn` is returned as that prompt's terminal result.

`fork_session` issues the unstable ACP `session/fork` request to branch a
session at its current head. Unlike `session/load` and `session/resume`, which
echo the input session id, fork returns the NEW forked session id taken from the
response body; the harness swaps its active session to it via
`swap_active_session` (see `@/nori-rs/harness/src/backend/session_swap.rs`). This
method requires the SDK `unstable` feature, enabled in `acp-host/Cargo.toml`.

The default spawn path publishes initialize on the ordered connection inbox.
The harness uses the opt-in initialize sink so the same raw response survives a
failed constructor; the two paths are exclusive, so successful initialization
is never duplicated. As soon as the direct child moves into the exit watcher,
spawn installs a `ChildHandle` that owns its kill notification and exit state.
That handle exists before the initialization await, so aborting a preparation
drops the handle, signals teardown, and leaves the watcher responsible for
process-group cleanup and reaping.

The SDK handlers delegate permission requests but handle filesystem read/write
inside the host. File writes are restricted to the workspace or `/tmp`; reads
remain unrestricted. Host-handled requests do not leak duplicate public
`AcpEvent::Request` values.

### Things to Know

- All event producers share one ordered inbox, including child exit. Do not
  split public ACP traffic into racing channels.
- Unix spawn makes the direct child its process-group leader, so the child PID
  is also the known process-group ID retained for teardown. The child watcher
  owns the `Child`, observes leader exit with `waitid(WNOWAIT)`, signals that
  stored group directly to sweep descendants, and only then reaps the leader;
  it must not try to rediscover the group through an exited leader.
- Startup races initialization against early child exit so authentication and
  spawn failures retain the child's stderr explanation.
- Startup cancellation is also an owned teardown path: once the child has
  spawned, no await point in initialization exists without a child-lifecycle
  guard.
- Orderly `session/close` is distinct from connection loss. The harness maps
  them to `SessionEnded(Closed)` and `SessionEnded(ConnectionLost)`
  respectively.
- The connection uses `ProtocolVersion::LATEST` while enforcing ACP v1 as the
  minimum supported version.

Created and maintained by Nori.
