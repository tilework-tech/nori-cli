# Remote Control (Remote ACP Transport)

`nori --remote` and `/remote-control` expose the running interactive Nori
session as an ACP [agent](glossary.md#actors) over the WebSocket profile of the
upstream [ACP Streamable HTTP & WebSocket Transport RFD](https://agentclientprotocol.com/rfds/streamable-http-websocket-transport).
A remote [client](glossary.md#actors) such as Zed or another Nori CLI drives
the same [session harness](glossary.md#nori-runtime-boundaries) the TUI is
showing.

This is not `nori exec --acp`. That command is a bounded, terminal-independent
stdio facade; remote control exposes the long-lived harness of the TUI.

## 1. Topology

```text
Zed
  └─ stdio ─► local Handroll facade
                 └─ WebSocket ─► microVM Nori CLI (TUI + harness)
                                      └─ stdio ACP ─► Codex or another agent

terminal user ─► microVM Handroll PTY attach ─► the same Nori CLI process
```

WebSocket ACP traffic terminates directly in Nori. Handroll in the microVM owns
only the PTY and terminal detach/attach; foregrounding or detaching that
terminal must not affect the WebSocket connection.

## 2. Security model

The surface is unauthenticated and plaintext (`ws://`). It must therefore be
off by default, must always include loopback while enabled, and must never
bind a wildcard address. Any non-loopback address requires an explicit,
per-run opt-in (§7). Nori must not add TLS, authentication, Tailscale
Serve/Funnel configuration, or Handroll calls on its own.

## 3. Code ownership

| Layer | Owns |
| --- | --- |
| `nori-acp-host` `src/remote/` | `/acp` endpoint, `Acp-Connection-Id`, initialize gate, frame adapters, bounded output, the outward ACP Agent, and the `HostedAgent` trait. |
| `nori-harness` `src/remote_agent.rs` | `HarnessRemoteHost`: `HostedAgent` over `HarnessHandle`, outward session IDs, turn ownership, delegated-request routing. |
| `nori-tui` `src/remote_control.rs`, `app/`, `chatwidget/` | `RemoteControlManager`: bind policy, listener lifecycle, `SessionStarted` attachment, slash command, exposure confirmation. |

Rules:

- `nori-acp-host` must not depend on `nori-harness`. The dependency direction
  is `nori-harness` → `nori-acp-host`; `HostedAgent` uses only
  `nori-protocol` types.
- The remote Agent must call `HostedAgent`, never the downstream
  `AcpConnection`. Bypassing the harness would skip hooks, transcripts, goals,
  permission policy, prompt state, and session switching.
- The remote host consumes the harness's ordered `SessionEvent` fan-out
  (`HarnessHandle::subscribe_events`) as a separate bounded subscriber beside
  the TUI. A slow subscriber must never block the harness or TUI; it is
  dropped when its queue fills.
- The app owns exactly one `HarnessRemoteHost` for its lifetime. Listeners come
  and go; the host stays.

## 4. WebSocket contract

- `GET /acp` with a WebSocket upgrade opens a connection. Any other request on
  `/acp` gets `426 Upgrade Required`. Streamable HTTP/SSE is not served.
- Every upgrade response carries a fresh UUID `Acp-Connection-Id`.
- One text frame carries one UTF-8 JSON-RPC message. Binary frames are
  ignored; ping/pong is transport liveness only.
- The first valid JSON message must be an `initialize` request with valid
  params. Unparseable frames before it get a JSON-RPC `-32700` parse error;
  any other first message closes the socket with code `1002`.
- The `initialize` response is the first server message. Event forwarding
  starts only after it is sent.
- Outbound frames pass through a 256-frame queue; a frame that cannot reach
  the peer within 30 s closes the connection.

`initialize` advertises `loadSession` and session `list`, `resume`, and
`close` capabilities, agent info `nori` / "Nori CLI", and the Nori marker:

```json
{ "_meta": { "nori": { "remoteControl": { "version": 1, "activeSessionId": "<id>" } } } }
```

`activeSessionId` is present only once a session has started. A Nori client
that sees version 1, a non-empty ID, and `loadSession` must skip its session
picker and resume that ID with `session/load`. Other clients discover the
session through `session/list`.

### Methods

| Method | Behavior |
| --- | --- |
| `session/list` | Returns the single hosted session (or none). |
| `session/new` | Attaches to the hosted session instead of creating one; responds with its ID, then replays history. `-32002` if none. |
| `session/load` | Flushes the transcript, replays history as `session/update` notifications, then responds. |
| `session/resume` | Validates the ID; no replay. |
| `session/prompt` | Submits through the harness; the response is the harness turn's outcome under the client's own request ID. |
| `session/cancel` | Cancels only a remote-owned turn. |
| `session/close` | Closes the hosted harness session (terminal). |

Unknown session IDs return `-32002`.

## 5. Session identity and event forwarding

The outward ACP session ID is the Nori [conversation ID](glossary.md#identity)
(the transcript ID, falling back to the downstream ACP session ID). Downstream
session swaps that continue the conversation (compact, restore) must stay
invisible: the outward ID does not change. A fork produces a new conversation
ID and closes the remote connection; the client reconnects and rediscovers it.

The remote Agent forwards, it does not translate:

- `session/update` notifications pass through with only the session ID
  rewritten outward.
- Delegated `session/request_permission` requests go to the remote client only
  when the current turn is remote-owned. Other delegated request types are
  answered `method not found`.
- `RequestFailed` for a remote prompt becomes a `-32000` error on that prompt.
  `SessionEnded` fails every pending remote prompt with `-32000` and closes
  the connection. No other `NoriEvent` crosses the wire.

Prompt display is canonical: when a queued prompt becomes active, the harness
emits one `user_message_chunk` sequence with the caller's original blocks and
one message ID, ahead of agent output. Neither the TUI nor the remote client
inserts its own copy. The remote `PromptRequest._meta` flows through
`HostedAgent` and `HarnessHandle` to the downstream `session/prompt`; the
harness adds a `nori.dev/promptEchoId` marker (`PROMPT_ECHO_ID_META_KEY`) if
absent. A downstream echo is suppressed only when its marker matches the
active prompt and its content matches the active wire content. Ownership must
never be guessed from content alone.

The harness brackets every active turn with `SessionInfoUpdate` notifications
carrying `_meta.nori.status` `working` and `idle`
([Nori agent-turn status](glossary.md#nori-extension)), so an observing
frontend can track activity without seeing the initiator's prompt response.

## 6. Controllers, turns, and reconnect

- One remote controller at a time across all listeners. A newer connection on
  any address replaces the current one (last connect wins); the replaced
  socket is closed and its unanswered delegated requests are answered
  `Cancelled`.
- Socket EOF or network loss detaches the controller only. It must not close
  the harness session, stop the downstream agent, cancel the active prompt, or
  exit the TUI. Unanswered delegated requests are cancelled so they cannot
  wedge the agent.
- Reconnect is a fresh connection: new `Acp-Connection-Id`, new `initialize`,
  then `session/load` to recover. There are no sequence numbers, no replay of
  missed frames, and no retry of in-flight requests. A `session/load` during a
  streaming turn may interleave live updates with replay.
- The TUI and remote controller share one `HarnessHandle`, so remote prompts,
  tool calls, and results render in the TUI. A remote prompt is rejected with
  `-32015` ("session already has an active turn") whenever a turn is active or
  queued; it is never queued. Local prompts keep the normal harness queue.
- The app attaches the host at `SessionStarted`, whether or not any listener
  is enabled, so enabling later exposes the running session immediately. On
  an agent switch the host keeps following the current session until the
  candidate's `SessionStarted` commits it; candidate failure is invisible
  remotely. Committing the new session disconnects the controller.

## 7. Enabling and bind policy

`/remote-control` is client-owned: it never reaches the agent and works with
no active session.

| Command | Effect |
| --- | --- |
| `/remote-control` or `/remote-control on` | Bind `127.0.0.1` on an allocated port. |
| `/remote-control on tailnet` | Run `tailscale status --json` (3 s timeout), require `BackendState: Running` and an IPv4, then bind loopback and that IP on one shared allocated port. |
| `/remote-control on IP:PORT` | Bind loopback and the exact address on `PORT`. Non-loopback addresses first show a red confirmation, valid for this run only. |
| `/remote-control off` | Disconnect the controller and stop all listeners; the harness keeps running. |
| `/remote-control status` | Report scope, `ws://…/acp` URLs, and controller state. |

Enable and status results are history entries listing every bound
`ws://IP:PORT/acp` URL. In local-only scope, status and runtime enable add a
hint when Tailscale is running, but must not list the tailnet address until it
is bound. Re-enabling the current target only reports status.

`nori --remote <PORT|IP:PORT>` uses the same manager at startup. A bare port
binds loopback only. A non-loopback `IP:PORT` fails unless
`--remote-allow-nonloopback` is passed, then binds loopback and that address
on `PORT`. Wildcard addresses are always rejected.

Binding is atomic: every socket in a surface binds before any serves. A
replacement surface binds before the old one stops, unless it reuses an exact
nonzero address of the old surface; then the old server is shut down first,
and if the new bind fails the previous addresses are rebound and the original
error returned. Only a failed restore leaves remote control off, reporting
both errors. Shutdown cancels pending upgrades before closing the controller,
and app exit disables remote control.

## 8. Known gaps

These are current behavior, not guarantees:

- `initialize` echoes the client's requested protocol version without
  negotiation.
- `session/list`, `session/load`, `session/resume`, and `session/new` ignore
  `cwd`, pagination cursors, and MCP server parameters.
- Live updates flow right after `initialize`, before the client attaches with
  `session/load` or `session/resume`.
- A permission request emitted before a remote prompt's request ID is
  registered is not forwarded remotely; only the TUI sees it.
- For remote-owned turns both the TUI and the remote controller can answer the
  same permission request.
- `SessionEnded` can close the socket before the `session/close` response is
  delivered.
- Out of scope: authentication, TLS, endpoint discovery, broker routing,
  Handroll federation, multi-host capability aggregation, observer
  connections, Streamable HTTP/SSE, and reliable replay.
