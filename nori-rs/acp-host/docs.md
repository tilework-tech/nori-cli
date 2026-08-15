# Noridoc: nori-acp-host

Path: @/nori-rs/acp-host

### Overview

`nori-acp-host` is Nori's agent-agnostic, client-side ACP host. It owns the ACP
SDK connection, subprocess and wire lifecycle, agent registry, delegated client
requests, MCP forwarding, and ACP error categorization. It owns no TUI or
session-product state.

### How it fits into the larger codebase

```text
nori-harness
      |
      v
nori-acp-host <----> ACP agent subprocess
      |
      +---- nori-protocol (schema and public envelopes)
      +---- nori-config (agent and MCP configuration)
      +---- codex-rmcp-client (stored MCP credentials)
```

The host is the only client-side product crate that directly uses the
`agent-client-protocol` SDK. Schema values are still imported through
`nori_protocol::acp`, never through the SDK or schema crate directly.

### Core Implementation

- `connection/` spawns an agent, performs ACP initialization, exposes typed ACP
  methods, and emits one source-ordered `ConnectionEvent` stream.
- ACP notifications, delegated requests, and method responses become raw
  `nori_protocol::AcpEvent` values with their `RequestId` intact.
- Initialization can route its raw response to the harness independently of
  connection construction, preserving schema errors even when construction
  fails without duplicating successful initialize events.
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

### Things to Know

- The dependency direction is `nori-harness -> nori-acp-host`, never the
  reverse.
- Disabling native goals is a policy of Nori's built-in Codex launch only.
  User-defined agent processes retain their explicit command and environment;
  goal ownership and routing remain a harness concern.
- Ambient `CODEX_CONFIG` must be a JSON object whose `features` value, when
  present, is also an object. Invalid JSON or incompatible shapes fail built-in
  Codex configuration explicitly instead of silently discarding user settings.
- Terminal and extension request families are not advertised by the current
  host. Adding them requires an explicit host-policy decision, not a generic
  protocol mirror.
- The `agent-client-protocol` SDK is built with the `unstable` feature so the
  host can call `session/fork` (branch-at-head). See `fork_session` in
  `@/nori-rs/acp-host/src/connection/docs.md`.
- Shutdown closes stdin and optionally waits for a caller-selected grace period.
  The child owner then kills the process group before reaping its leader, so
  MCP servers and other descendants cannot survive a cooperative agent exit.
  A nonzero grace is reserved for flows such as cloud detach.
- The removed ACP-to-Codex translator must not be recreated. Display-friendly
  projection belongs privately in a consumer such as the TUI.

Created and maintained by Nori.
