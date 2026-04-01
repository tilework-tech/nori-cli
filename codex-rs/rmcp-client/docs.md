# Noridoc: codex-rmcp-client

Path: @/codex-rs/rmcp-client

### Overview

The rmcp-client crate provides a high-level MCP client for connecting to remote MCP servers. It wraps the `rmcp` library with OAuth authentication, credential storage, and server discovery capabilities.

### How it fits into the larger codebase

Used by `@/codex-rs/core/` (`mcp_connection_manager.rs`) to establish connections to configured MCP servers that provide additional tools to the AI model.

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
  - `start_oauth_login()` - Non-blocking async flow that returns an `OAuthLoginHandle`. The handle exposes a `cancel_tx: Option<oneshot::Sender<()>>` for programmatic cancellation and a `task: JoinHandle<Result<()>>` for awaiting completion. Used by the TUI to run OAuth inline without suspending the terminal.
- Token refresh handling via `OAuthPersistor`, which is called after every MCP request

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
