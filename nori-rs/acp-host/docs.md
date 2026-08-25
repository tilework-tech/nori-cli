# Noridoc: nori-acp-host

Path: @/nori-rs/acp-host

### Overview

`nori-acp-host` is Nori's agent-agnostic, client-side ACP host. It owns the ACP
SDK connection, subprocess and wire lifecycle, agent registry, delegated client
requests, MCP forwarding, and ACP error categorization. It owns no TUI or
session-product state.

It also owns the optional remote ACP transport (`remote/`): a WebSocket server
implementing the WebSocket profile of the upstream ACP "Streamable HTTP &
WebSocket Transport" RFD, serving the hosted harness session outward as an ACP
Agent (see `@/docs/specs/remote-acp-transport.md`).

### How it fits into the larger codebase

```text
remote ACP client -- WebSocket /acp --> remote::RemoteAcpServer (this crate)
                                              |  calls
                                              v
                                        remote::HostedAgent (trait, this crate)
                                              ^
                                              |  implemented by
nori-harness ---------------------------------+
      |
      v
nori-acp-host <----> ACP agent subprocess
      +---- nori-protocol (schema and public envelopes)
      +---- nori-config (agent and MCP configuration)
      +---- codex-rmcp-client (stored MCP credentials)
```

The host is the only client-side product crate that directly uses the
`agent-client-protocol` SDK. Schema values are still imported through
`nori_protocol::acp`, never through the SDK or schema crate directly.

The remote server inverts the runtime call direction (a remote client drives
the harness through this crate) without inverting the crate dependency:
`remote/hosted_agent.rs` defines the downward-facing `HostedAgent` trait in
`nori-protocol` types, and `nori-harness` implements it over `HarnessHandle`.
The dependency direction stays `nori-harness -> nori-acp-host`.

### Core Implementation

- `connection/` spawns an agent, performs ACP initialization, exposes typed ACP
  methods, and emits one source-ordered `ConnectionEvent` stream.
- ACP notifications, delegated requests, and method responses become raw
  `nori_protocol::AcpEvent` values with their `RequestId` intact.
- Initialization can route its raw response to the harness independently of
  connection construction, preserving schema errors even when construction
  fails without duplicating successful initialize events.
- Spawn transfers the child to its watcher and immediately installs a
  `ChildHandle` cancellation guard before awaiting ACP initialization. If the
  initialization future is aborted, dropping that guard kills the process
  group and lets the watcher reap it instead of orphaning a half-initialized
  agent.
- Each prompt call issues exactly one ACP `session/prompt` request and publishes
  its transport-assigned ID. Cancellation does not trigger a hidden resend or
  cancel-tail absorption loop. A successful empty `EndTurn` response is a
  terminal result.
- Permission requests are delegated outward. Filesystem read/write requests are
  handled by the host and are not emitted a second time as public requests.
- A private `SessionUpdate` copy feeds the harness reducer after the matching
  raw notification; it is implementation state, not another public protocol.
- `registry.rs` resolves built-in and configured agents to spawnable process
  definitions. The built-in Codex definition uses the maintained
  `@agentclientprotocol/codex-acp` adapter and disables Codex-native goals in
  its subprocess configuration. It merges a valid ambient `CODEX_CONFIG`,
  preserving unrelated top-level and feature settings while forcing only
  `features.goals = false`, leaving Nori-owned goal state to the `nori-client`
  MCP server. `error_category.rs` preserves structured ACP errors before
  falling back to message classification.
- `remote/` serves one axum-based `/acp` endpoint: a fresh `Acp-Connection-Id`
  header on each 101 upgrade, `426 Upgrade Required` for plain HTTP, one
  JSON-RPC message per UTF-8 text frame adapted into the SDK's `Lines`
  transport (`remote/wire.rs`), binary frames ignored, and a bounded outgoing
  queue whose writer finishes with a best-effort close frame. `initialize`
  must be the first message on the socket, otherwise close code 1002. One
  connection is live at a time; a newer connection replaces the current one
  (last connect wins).
- The remote Agent advertises `loadSession` plus session list/resume/close
  capabilities and serves the corresponding session methods; `session/load`
  replays recorded history as `session/update` notifications ahead of its
  response. `session/new` is rejected with guidance: the remote surface
  exposes the running session, discovered via `session/list`.
- Responses are correlated at the boundary: `HostedAgent::prompt` returns the
  harness-issued request id, and the turn's final response arrives in stream
  order through the hosted event subscription, so a turn's `session/update`
  notifications always precede its response. Delegated permission requests
  round-trip through the remote controller; the subscription channel closing
  (queue overflow or replacement) tears the connection down. Wire behavior is
  exercised in `@/nori-rs/acp-host/tests/remote_ws.rs` against a fake
  `HostedAgent`.
- `registry.rs` also owns spawn-time **model injection**. Runtime model changes
  normally go over the live ACP `session/set_config_option` RPC, but adapters
  advertise only a subset of the models they can run and reject anything outside
  that catalog. `ModelInjection` ({ `Env { var }`, `Arg { flag }`, `CodexConfig`,
  `None` }) forces a chosen model through the agent's own out-of-band channel at
  spawn, so the model becomes the adapter's advertised `currentValue` (which ACP
  always treats as valid), bypassing the catalog. `AgentKind::model_injection()`
  maps built-in agents to their channel (Claude → `ANTHROPIC_MODEL` env, Gemini
  → `GEMINI_MODEL` env, Codex → merge `model` into the `CODEX_CONFIG` JSON).
  Custom/BYO agents resolve their strategy from `[[agents]].model_override` in
  `nori-config`; the default is `None` (no channel → live RPC only).
  `AcpAgentConfig::inject_model` applies the resolved strategy to the spawn env
  or args and is a no-op for `None`, so callers (the harness) may invoke it
  unconditionally; `supports_model_injection()` reports whether a channel exists.
- `AgentKind::other_models()` returns a curated, per-agent
  `&'static [OtherModel]` (`{ id, label }`) of real models the adapter does *not*
  advertise but that generally run when forced through injection. It is the
  durable complement to the advertised catalog: the advertised set is
  agent/version/account-dependent and adapters do not hardcode it, so this static
  list plus render-time dedup against the live catalog is how the TUI populates
  the `/model` picker's "Other" section without a second source of truth.
  Selecting one routes through injection exactly like a `[default_models]` custom
  id. `OtherModel` is re-exported from `nori-harness` for that picker.

### Things to Know

- The dependency direction is `nori-harness -> nori-acp-host`, never the
  reverse. The remote module preserves it: its handlers call only the
  `HostedAgent` interface, never the downstream `AcpConnection`, whose direct
  use would bypass harness-owned hooks, transcripts, goals, and permission
  policy.
- Disabling native goals is a policy of Nori's built-in Codex launch only.
  User-defined agent processes retain their explicit command and environment;
  goal ownership and routing remain a harness concern.
- Ambient `CODEX_CONFIG` must be a JSON object whose `features` value, when
  present, is also an object. Invalid JSON or incompatible shapes fail built-in
  Codex configuration explicitly instead of silently discarding user settings.
  Codex model injection merges only the `model` key into that same object,
  preserving the goals-disabled flag.
- Model injection does not validate the model id: an invalid model is accepted at
  spawn and only fails at the first prompt. For env-based channels, nori's
  injected value takes precedence over the agent's own configured model (e.g.
  `~/.claude/settings.json`) for nori-spawned sessions — nori's `[default_models]`
  is authoritative here by design.
- `set_config_option` mirrors only a *successful* response onto the public event
  boundary. Its error is returned to the caller (the `/model` flow renders a
  friendly message and may persist-and-restart), so mirroring the raw JSON-RPC
  error too would surface a confusing duplicate "Internal error" cell. Raw wire
  errors are still captured by ACP wire recording, which taps stdio separately.
- Terminal and extension request families are not advertised by the current
  host. Adding them requires an explicit host-policy decision, not a generic
  protocol mirror.
- The remote WebSocket server is unauthenticated. `parse_bind_addr` treats a
  bare port as a loopback bind and refuses non-loopback addresses without the
  caller's explicit opt-in flag.
- A remote disconnect detaches the controller but never ends the harness
  session. Per the RFD's v1 reliability model there are no sequence numbers and
  no replay of missed messages; `session/load` from the transcript is the
  recovery path after reconnect, so the server must advertise `loadSession`.
- `nori exec --acp` remains a separate, bounded stdio facade (see
  `@/nori-rs/exec/docs.md`); the remote module instead exposes the long-lived
  interactive harness session.
- The `agent-client-protocol` SDK is built with the `unstable` feature so the
  host can call `session/fork` (branch-at-head). See `fork_session` in
  `@/nori-rs/acp-host/src/connection/docs.md`.
- Shutdown closes stdin and optionally waits for a caller-selected grace period.
  The child owner then kills the process group before reaping its leader, so
  MCP servers and other descendants cannot survive a cooperative agent exit.
  A nonzero grace is reserved for bounded cooperative cleanup, including the
  short pre-session grace for an abandoned prepared connection and the longer
  cloud-detach path.
- The removed ACP-to-Codex translator must not be recreated. Display-friendly
  projection belongs privately in a consumer such as the TUI.

Created and maintained by Nori.
