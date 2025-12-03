# Noridoc: rmcp-client

Path: @/codex-rs/rmcp-client

### Overview

The `codex-rmcp-client` crate implements an MCP client for connecting to external MCP servers. It enables Codex to access tools and resources provided by MCP servers configured in `config.toml`.

### How it fits into the larger codebase

RMCP client is used by core for MCP server connections:

- **Core** `mcp_connection_manager.rs` uses for server connections
- **Uses** `mcp-types` for message structures
- **Uses** `rmcp` crate (external) as protocol foundation

### Core Implementation

The crate wraps the external `rmcp` crate to provide:
- Connection management
- Tool invocation
- Resource access
- Proper error handling for Codex context

### Things to Know

**Configuration:**

MCP servers are configured in `~/.codex/config.toml`:
```toml
[[mcp_servers]]
name = "server-name"
command = ["path/to/server"]
```

**Transport:**

Connects to MCP servers via stdio (subprocess) transport.

**Tool Integration:**

Tools from connected MCP servers are registered in core's tool registry and available for model use.

Created and maintained by Nori.
