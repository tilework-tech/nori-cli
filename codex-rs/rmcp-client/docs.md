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
- `perform_oauth_login()` - Interactive OAuth flow that spins up a local `tiny_http` callback server, opens the browser, and waits for the OAuth redirect. The wait is cancellable: a background stdin reader thread lets the user press Enter to abort, implemented via `tokio::select!` between the callback receiver, a cancel signal, and a 5-minute timeout (`wait_for_callback_or_cancel()`).
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
- The OAuth login flow is cancellable by the user (Enter key) or by timeout, preventing the TUI from blocking indefinitely during authentication

Created and maintained by Nori.
