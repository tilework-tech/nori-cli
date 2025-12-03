# Noridoc: mcp-types

Path: @/codex-rs/mcp-types

### Overview

The `codex-mcp-types` crate defines JSON-RPC message types for the Model Context Protocol (MCP). It provides the data structures for MCP client-server communication used by both the MCP server and rmcp-client.

### How it fits into the larger codebase

MCP types is a shared dependency:

- **MCP server** uses for message parsing/serialization
- **RMCP client** uses for MCP server communication
- **Defines** protocol-compliant message structures

### Core Implementation

**Key Types:**

```rust
pub enum JSONRPCMessage {
    Request(Request),
    Response(Response),
    Notification(Notification),
    Error(ErrorResponse),
}
```

Plus method-specific request/response types for:
- `initialize`
- `tools/list`
- `tools/call`
- `resources/list`
- etc.

### Things to Know

**Protocol Compliance:**

Types are designed to match the MCP specification for interoperability with other MCP implementations.

**Serde Integration:**

All types derive serde traits for JSON serialization with appropriate rename rules for camelCase JSON fields.

Created and maintained by Nori.
