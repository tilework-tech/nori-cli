# Noridoc: Host session client

Path: @/nori-rs/tui/src/host_client

### Overview

- A client of a Nori host: one connection, JSON-RPC request correlation, and
  the host's notifications, with no terminal state. The connection is either
  an authenticated socket to a background `nori host` or an in-process pipe to
  the host owned by the foreground TUI.
- Separates wire traffic from frontends. A frontend names what it wants as a
  `HostCall`, attaches its own tag, and gets back `HostEvent`s with replies
  matched to that tag.
- Exists so more than one frontend can drive a host through the same code,
  because the host contract says the TUI must not own its own session manager.
  The [attach frontend](../attach/docs.md) drives a background host over the
  socket. Both foreground binaries drive their [owned host](../../../host-runtime/src/host/tui_owned.rs)
  over the pipe through [`HostLink`](../app/host_link.rs).

### How it fits into the larger codebase

```
CLI host dispatch ──endpoint, token, refresh callback──▶ HostEndpoint
                                                          │
attach Runtime ──HostCall + tag──▶ HostSessionClient ──JSON──▶ drive()      ── ManagedClient socket
App (HostLink) ──HostCall + tag──▶ HostSessionClient ──JSON──▶ drive_pipe() ── OwnedHost line channels
      ▲                                   │                        │
      └──────── HostEvent<tag> ◀──────────┴──── ConnectionEvent ◀──┘
```

- [CLI host dispatch](../../../cli/src/host/docs.md) resolves the endpoint and
  token and, for secure local discovery only, supplies a callback that rereads
  the local token. The frontend packs these into `HostEndpoint`; explicit
  remote endpoints get no callback.
- The socket is the harness's re-export of
  [ManagedClient](../../../acp-host/src/remote/client.rs). This module never
  touches the host's sessions or agent processes; the host keeps mutation
  policy and controller ownership.
- The frontend owns two channels: it spawns a driver ([`drive`](connection.rs)
  for a socket, [`drive_pipe`](pipe.rs) for a pipe) with the request receiver
  and event sender, and builds `HostSessionClient` over the request sender.
  Each `ConnectionEvent` it receives goes through `HostSessionClient::incoming`,
  so correlation and event mapping are identical for both transports.
- The pipe's two line channels come from the CLI's
  [owned TUI host](../../../cli/src/host/docs.md#owned-tui-host), which serves
  them with `RemoteAcpServer::serve_host_pipe_from` and binds the foreground
  user's identity to that connection.
- Method names and parameter shapes follow the
  [host contract](../../../../docs/remote-control.md#11-background-host-and-attached-terminals).
  Calls cover initialize, session list, subscribe, take control, new, prompt,
  follow-up, observed-turn cancel, close, resume, permission response,
  delegate, delegation resolve, and the prepared-agent picker used by `/agent`
  switching.

### Core Implementation

- [`HostSessionClient<T>`](mod.rs) allocates increasing JSON-RPC ids, stores
  the caller's tag per id, and serializes each `HostCall` to its wire method.
  If the connection cannot enqueue a request, `request` returns its tag so
  the caller can resolve the waiting operation. [PromptInterrupt](prompt_interrupt.rs) couples an early interrupt to the
  matching receipt, including when admission precedes the first phase event.
  `has_pending` lets a caller avoid duplicate calls, such as a second
  outstanding list.
- `incoming` maps connection events to `HostEvent`s:

  | Connection event | Host event |
  | --- | --- |
  | Connected | `Connected`; the frontend must initialize before anything else |
  | Disconnected | `Disconnected` with the reason and every unanswered tag, which are forgotten |
  | Stopped | `Stopped`; the client also stops |
  | `_nori/session/event` | `Session` with the envelope parameters |
  | `_nori/control/changed` | `RoleChanged` (missing role reads as `watcher`) |
  | Reply to a pending id | `Reply` with the tag and `Ok(result)` or `Err(HostError)` |
  | Any other host request or notification, or a reply to no pending id | `Ignored` |

- `Resume` carries the session row's folder when the host reported one and
  becomes ACP `session/resume`; without a folder (an older host) it becomes
  `_nori/session/resume`.
- The prepared-agent calls map to the host picker requests:
  `PrepareAgent` is `_nori/host/prepare_agent`, `DiscardPrepared` is
  `_nori/host/discard_agent`, and `ListPrepared` and `ActivatePrepared` are
  plain ACP `session/list` and `session/new` scoped by
  `_meta["nori/preparedAgent"]` (the exported `PREPARED_AGENT` key) with the
  selection ID, plus `savedSessionId` when activating an adapter's saved
  session. The scope makes `session/list` return the adapter's catalog rather
  than host conversations.
- [Connection driving](connection.rs) owns the socket and reconnects. Every
  received message renews a liveness deadline. Reads and sends share it, so a
  silent stall, not only a TCP error, ends the connection. The failed socket
  is dropped before a replacement is opened.
- Reconnect retries on a short fixed delay. Each attempt authenticates again
  and compares the new connection's host-instance UUID with the initial one
  before reporting `Connected`. A missing UUID or a different one stops the
  driver; token equality alone never proves the same host.
- On an authentication rejection the driver calls the local token refresh
  callback once and retries once with the new token. No callback, an
  unreadable token, or a second rejection stops the driver.
- [Pipe driving](pipe.rs) has no token, liveness deadline, or reconnect. It
  reports `Connected` at once, forwards each request as one JSON line, and
  parses each host line. A line that is not JSON is logged and skipped. When
  the host closes its end, or stops accepting lines, the driver reports
  `Stopped` and ends for good.

### Things to Know

- Unacknowledged requests are never replayed. The host may already have
  applied them, so the driver drains queued requests after a disconnect and
  again after reconnecting, and the client hands their tags back in
  `Disconnected`. Deciding whether an outcome is uncertain, and warning, is
  the frontend's job. Only event subscriptions resume, from the frontend's own
  cursors.
- The liveness deadline (socket only) assumes steady traffic from the host. The client does
  not generate it: the frontend must poll (attach refreshes the session list
  every second) or an idle but healthy connection times out.
- After `Stopped` or `stop()` the client sends nothing and returns `Unsent`
  for every call. `Unsent` is also returned when the driver has gone away.
- Host requests share the JSON-RPC id space with replies. Only messages
  without a method can settle a pending call, so a host request such as
  `session/request_permission` (sent for a resumed session) is never matched
  against the client's ids.
- Unit [tests](tests.rs) cover correlation and wire mapping;
  [connection tests](connection_tests.rs) cover token refresh, refresh
  rejection, remote auth failure, and host-restart detection;
  [pipe tests](pipe_tests.rs) cover the pipe driver. The
  [PTY scenarios](../../../tui-pty-e2e/tests/host_attach.rs) exercise dropped
  and silently stalled TCP connections end to end.

Created and maintained by Nori.
