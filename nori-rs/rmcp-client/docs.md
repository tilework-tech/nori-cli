# Noridoc: codex-rmcp-client

Path: @/nori-rs/rmcp-client

### Overview

The rmcp-client crate provides a high-level MCP client for connecting to remote MCP servers. It wraps the `rmcp` library with OAuth authentication, credential storage, and server discovery capabilities.

### How it fits into the larger codebase

Used by `@/nori-rs/core/` (`mcp_connection_manager.rs`) to establish connections to configured MCP servers that provide additional tools to the AI model.

### Core Implementation

**RmcpClient** (`rmcp_client.rs`): Main client interface providing:
- Server initialization and handshake
- Tool listing and invocation
- Resource and prompt access
- Elicitation (user input requests from server)

**OAuth Flow** (`oauth.rs`, `perform_oauth_login.rs`):
- `StoredOAuthTokens` - Persisted token storage
- Two OAuth login entry points, both backed by the same `wait_for_callback_or_cancel()` mechanism (biased `tokio::select!` between callback, cancel signal, and 5-minute timeout):
  - `perform_oauth_login()` - Blocking/interactive flow that uses `println!` for status and `stdin` Enter for cancellation. Used by CLI contexts where TUI suspension is acceptable.
  - `start_oauth_login()` - Non-blocking async flow that returns an `OAuthLoginHandle`. The handle exposes a `cancel_tx: Option<oneshot::Sender<()>>` for programmatic cancellation and a `task: JoinHandle<Result<()>>` for awaiting completion. Used by the TUI to run OAuth inline without suspending the terminal. Accepts optional `client_id` and `client_secret` parameters to select between two OAuth paths (see below).
- Token refresh handling via `OAuthPersistor`, which is called after every MCP request

**Two OAuth Credential Paths** (`perform_oauth_login.rs`):

`start_oauth_login()` branches based on whether a `client_id` is provided:

| Path | When | Mechanism | Registration |
|------|------|-----------|-------------|
| Dynamic registration | `client_id` is `None` | `rmcp::OAuthState` | Server-side dynamic client registration |
| Pre-configured credentials | `client_id` is `Some(...)` | `oauth2` crate directly via `start_oauth_login_preconfigured()` | User provides client ID (and optionally secret) from a manually-created OAuth app |

The pre-configured path uses `discover_oauth_metadata()` to fetch the server's `AuthorizationMetadata` from RFC 8414 well-known endpoints (tries path-scoped URL first, then root). It builds an `oauth2::BasicClient` with the pre-configured `client_id`, optional `ClientSecret`, discovered auth/token endpoints, and a PKCE challenge. CSRF state validation is performed explicitly (unlike the `rmcp` path which handles it internally). Both paths store tokens via `save_oauth_tokens()` using the same `StoredOAuthTokens` format.

**Auth Status** (`auth_status.rs`):
- `determine_streamable_http_auth_status()` - Check authentication state
- `supports_oauth_login()` - Check server OAuth capability

**Program Resolution** (`program_resolver.rs`): Resolves MCP server executables from configuration.

**Credential Storage** (`oauth.rs`):
- `OAuthCredentialsStoreMode` - Keyring vs file storage
- `save_oauth_tokens()` / `delete_oauth_tokens()` - Credential management

### Things to Know

- Re-exports types from `codex-protocol` and `rmcp` crate (currently rmcp 0.12.0)
- Supports both streamable HTTP and stdio transport
- OAuth tokens can be stored in system keyring or fallback file
- The `Elicitation` type handles server-initiated user input requests
- Server discovery uses `~/.codex/` (or `~/.nori/cli/`) home directory
- rmcp 0.12.0 treats OAuth dynamic client registration failure as a hard error (earlier versions silently fell back to a default `client_id`), so connection failures surface immediately rather than producing broken auth URLs
- The OAuth login flow is cancellable by the user (Enter key in CLI mode, or programmatically via `OAuthLoginHandle.cancel_tx` in TUI mode) or by timeout, preventing blocking indefinitely during authentication
- `OAuthLoginHandle` has no `Drop` implementation; the caller is responsible for managing both the cancel sender and the task handle

Created and maintained by Nori.
