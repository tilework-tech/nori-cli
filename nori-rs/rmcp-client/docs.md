# Noridoc: codex-rmcp-client

Path: @/nori-rs/rmcp-client

### Overview

- `codex-rmcp-client` wraps the `rmcp` SDK with MCP connections, OAuth,
  credential storage, server discovery, and elicitation support.
- It owns computed MCP authentication status and does not participate in the
  ACP session protocol.

### How it fits into the larger codebase

- [`nori-acp-host`](../acp-host/src/connection/mcp.rs) loads stored OAuth tokens
  here while converting configured MCP servers for ACP session setup.
- TUI and CLI MCP workflows use the OAuth status and login APIs exported by
  [`lib.rs`](src/lib.rs).
- MCP server configuration itself is owned by [`nori-config`](../nori-config/docs.md).

### Core Implementation

- [`RmcpClient`](src/rmcp_client.rs) initializes stdio subprocess or streamable
  HTTP transports and exposes tool calls, resource access, notifications, and
  elicitation. It does not expose prompt listing or retrieval. Initialization
  consumes the pending transport once; subsequent operations require the ready
  service and apply the caller's timeout.
- HTTP uses `rmcp`'s reqwest transport with configured headers. An explicit
  bearer token takes precedence over stored OAuth credentials; OAuth-backed
  connections refresh and persist tokens through [`oauth.rs`](src/oauth.rs)
  using the configured keyring or file storage policy.
- [`perform_oauth_login.rs`](src/perform_oauth_login.rs) supports SDK-managed
  dynamic registration and a separate preconfigured-client flow with PKCE and
  CSRF validation. The latter uses the [`oauth2` HTTP adapter](src/oauth_http_client.rs)
  over reqwest 0.13, preserving the configured HTTP client, vendor token fields,
  and OAuth error responses while bounding token-response reads at 1 MiB.
- [`utils.rs`](src/utils.rs) converts the supported wire shapes between local
  `mcp-types` and SDK models through JSON; these schemas need not track the same
  protocol revision. Tool-result conversion normalizes missing content to an
  empty array. Conversion and RPC errors propagate with operation context.
- `McpAuthStatus::{Unsupported, NotLoggedIn, BearerToken, OAuth}` is defined
  here because it is computed from MCP/OAuth behavior rather than user
  configuration or ACP session traffic.

### Things to Know

- The crate no longer re-exports any Codex protocol types.
- `nori-config` owns MCP server and transport configuration; this crate owns
  OAuth credentials and computed auth state.
- `OAuthLoginHandle` cancellation and task ownership remain the caller's
  responsibility.
- Host-side token loading is eager during MCP-to-ACP conversion.
- [`HTTP dependencies`](Cargo.toml) explicitly enable HTTP/2 and system proxy
  support. TLS uses rustls with platform certificate verification and the OS
  trust store, including when this crate is built alone. Certificate validation
  can differ from the previous native-TLS path, particularly on Linux; isolated
  deployments need OS CA certificates rather than relying on bundled roots.
- The patched SDK bounds streamable HTTP JSON success bodies at 16 MiB and
  error bodies at 64 KiB, including responses without `Content-Length`.
  [`Transport integration tests`](tests/transports.rs) exercise these limits
  and real stdio/HTTP initialization, tool listing, and tool execution with
  bearer authentication and custom headers.
- SDK-managed OAuth discovery validates issuer metadata and rejects missing
  issuers when the expected issuer can be derived from the discovery URL.
  The callback server forwards the optional `iss` parameter to SDK validation
  before token exchange; a mismatched issuer, or a missing issuer when the
  authorization server requires it, fails login.
  Preconfigured-client login retains its separate metadata discovery path.
- SDK-managed login uses initial scope selection: unspecified scopes do not
  expand to every scope advertised by the authorization server. The SDK still
  honors explicit scopes; otherwise it selects scopes from the initial resource
  challenge or protected-resource metadata. It can append advertised
  `offline_access` to a nonempty selection.
- SDK upgrades do not imply a protocol-version upgrade: callers supply client
  initialization versions, and the standalone fixture servers explicitly
  retain MCP `2025-03-26`.

Created and maintained by Nori.
