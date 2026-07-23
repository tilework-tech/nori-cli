# Noridoc: codex-rmcp-client

Path: @/nori-rs/rmcp-client

### Overview

`codex-rmcp-client` wraps `rmcp` with MCP connection, OAuth, credential storage,
server discovery, and elicitation support. It owns computed MCP authentication
status and does not participate in the ACP session protocol.

### How it fits into the larger codebase

- `nori-acp-host` loads stored OAuth tokens here while converting configured
  MCP servers for ACP session setup.
- TUI and CLI MCP workflows use the OAuth status and login APIs.
- MCP server configuration itself is owned by `nori-config`.

### Core Implementation

`RmcpClient` wraps server initialization, tool/resource/prompt access, and
elicitation. OAuth supports dynamic registration and preconfigured client
credentials, with a cancellable local callback flow. Token helpers persist to
the configured keyring or fallback file.

`McpAuthStatus::{Unsupported, NotLoggedIn, BearerToken, OAuth}` is defined here
because it is computed from MCP/OAuth behavior rather than user configuration
or ACP session traffic.

### Things to Know

- The crate no longer re-exports any Codex protocol types.
- `nori-config` owns MCP server and transport configuration; this crate owns
  OAuth credentials and computed auth state.
- `OAuthLoginHandle` cancellation and task ownership remain the caller's
  responsibility.
- Host-side token loading is eager during MCP-to-ACP conversion.

Created and maintained by Nori.
