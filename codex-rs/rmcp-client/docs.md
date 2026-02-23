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
- `perform_oauth_login()` - Interactive OAuth flow
- Token refresh handling

**Auth Status** (`auth_status.rs`):
- `determine_streamable_http_auth_status()` - Check authentication state
- `supports_oauth_login()` - Check server OAuth capability

**Program Resolution** (`program_resolver.rs`): Resolves MCP server executables from configuration.

**Credential Storage** (`oauth.rs`):
- `OAuthCredentialsStoreMode` - Keyring vs file storage
- `save_oauth_tokens()` / `delete_oauth_tokens()` - Credential management

### Things to Know

- Re-exports types from `codex-protocol` and `rmcp` crate
- Supports both streamable HTTP and stdio transport
- OAuth tokens can be stored in system keyring or fallback file
- The `Elicitation` type handles server-initiated user input requests
- Server discovery uses `~/.codex/` (or `~/.nori/cli/`) home directory

Created and maintained by Nori.
