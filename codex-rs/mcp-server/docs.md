# Noridoc: mcp-server

Path: @/codex-rs/mcp-server

### Overview

The `codex-mcp-server` crate implements an MCP (Model Context Protocol) server that exposes Codex tools to external MCP clients. This allows other AI agents to use Codex as a tool provider, enabling nested agent architectures where Codex can be invoked by tools like Claude Code.

### How it fits into the larger codebase

MCP Server is invoked via `codex mcp-server`:

- **Uses** `codex-core` for tool execution and configuration
- **Uses** `mcp-types` for protocol message definitions
- **Exposes** Codex tools (shell, apply_patch, etc.) via MCP tool protocol
- **Complements** Codex's role as an MCP client (connecting to external servers)

### Core Implementation

**Entry Point:**

`run_main()` in `lib.rs` mirrors app-server architecture:
1. **stdin reader**: Parses MCP JSON-RPC messages
2. **processor**: Routes through `MessageProcessor`
3. **stdout writer**: Serializes responses

**Message Processing:**

`message_processor.rs` (in module) handles MCP-specific methods:
- `initialize`: Protocol handshake
- `tools/list`: Enumerate available Codex tools
- `tools/call`: Execute a Codex tool

**Tool Execution:**

`codex_tool_runner.rs` wraps core tool execution:
- Converts MCP tool calls to Codex tool format
- Handles sandbox and approval policies
- Returns structured results

### Things to Know

**Exposed Tools:**

`codex_tool_config.rs` defines:
- `CodexToolCallParam`: Input parameters for tool calls
- `CodexToolCallReplyParam`: Tool execution results

Tools include: shell execution, file operations, patch application.

**Approval Handling:**

The `exec_approval.rs` and `patch_approval.rs` modules handle:
- `ExecApprovalElicitRequestParams`: Shell command approval
- `PatchApprovalElicitRequestParams`: File modification approval

These use MCP's elicitation protocol for interactive approval when needed.

**Transport:**

Uses unbounded channel for outgoing messages (vs app-server's bounded) since MCP servers typically have lower message volume.

**Usage:**

```bash
# Run directly
codex mcp-server

# Use with MCP inspector
npx @modelcontextprotocol/inspector codex mcp-server
```

Created and maintained by Nori.
