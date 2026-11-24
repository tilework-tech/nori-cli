# Noridoc: Host session client

Path: @/nori-rs/tui/src/host_client

### Overview

- A client of a background `nori host`: one authenticated connection, JSON-RPC
  request correlation, and the host's notifications, with no terminal state.
- Separates wire traffic from frontends. A frontend names what it wants as a
  `HostCall`, attaches its own tag, and gets back `HostEvent`s with replies
  matched to that tag.
- Exists so more than one frontend can drive the host through the same code.
  The [attach frontend](../attach/docs.md) is the first user; the plan is for
  the normal `nori` TUI to become a host client through it too, because the
  host contract says the TUI must not own its own session manager.

### How it fits into the larger codebase

```
CLI host dispatch ──endpoint, token, refresh callback──▶ HostEndpoint
                                                          │
frontend (attach Runtime) ──HostCall + tag──▶ HostSessionClient ──JSON──▶ drive()
          ▲                                       │                        │
          └────────────── HostEvent<tag> ◀────────┴──── ConnectionEvent ◀──┘
                                                               ManagedClient socket
```

- [CLI host dispatch](../../../cli/src/host/docs.md) resolves the endpoint and
  token and, for secure local discovery only, supplies a callback that rereads
  the local token. The frontend packs these into `HostEndpoint`; explicit
  remote endpoints get no callback.
- The socket is the harness's re-export of
  [ManagedClient](../../../acp-host/src/remote/client.rs). This module never
  touches the host's sessions or agent processes; the host keeps mutation
  policy and controller ownership.
- The frontend owns two channels: it spawns [`drive`](connection.rs) with the
  request receiver and event sender, and builds `HostSessionClient` over the
  request sender. Each `ConnectionEvent` it receives goes through
  `HostSessionClient::incoming`.
- Method names and parameter shapes follow the
  [host contract](../../../../docs/remote-control.md#11-background-host-and-attached-terminals).
  Calls cover initialize, session list, subscribe, take control, new, prompt,
  follow-up, cancel, observed-turn cancel, close, resume, permission response,
  delegate, and delegation resolve.

### Core Implementation

- [`HostSessionClient<T>`](mod.rs) allocates increasing JSON-RPC ids, stores
  the caller's tag per id, and serializes each `HostCall` to its wire method.
  `request` expects a reply; `notify` sends a JSON-RPC notification with no id
  (`Cancel` is sent as the standard `session/cancel` notification).
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

### Things to Know

- Unacknowledged requests are never replayed. The host may already have
  applied them, so the driver drains queued requests after a disconnect and
  again after reconnecting, and the client hands their tags back in
  `Disconnected`. Deciding whether an outcome is uncertain, and warning, is
  the frontend's job. Only event subscriptions resume, from the frontend's own
  cursors.
- The liveness deadline assumes steady traffic from the host. The client does
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
  rejection, remote auth failure, and host-restart detection. The
  [PTY scenarios](../../../tui-pty-e2e/tests/host_attach.rs) exercise dropped
  and silently stalled TCP connections end to end.

Created and maintained by Nori.
