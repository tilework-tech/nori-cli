# Noridoc: mcp-types

Path: @/nori-rs/mcp-types

### Overview

The mcp-types crate provides auto-generated Rust types for the Model Context Protocol (MCP) specification. These types define the JSON-RPC messages for communication between MCP clients and servers.

### How it fits into the larger codebase

Used by:
- `@/nori-rs/core/` - for MCP server communication
- `@/nori-rs/rmcp-client/` - for MCP client implementation
- `@/nori-rs/tui/` - for MCP-related type handling

### Core Implementation

**Generated Types**: The source is auto-generated from the MCP schema. Do not edit directly - regenerate using `./generate_mcp_types.py`. The types are organized across submodules: `content.rs` (content types), `dispatch.rs` (request/notification dispatch enums and TryFrom impls), `jsonrpc.rs` (JSON-RPC message types), `prompts.rs` (prompt-related types), `resources.rs` (resource-related types), `schema.rs` (JSON schema types), and `tools.rs` (tool-related types). The `lib.rs` re-exports all public types.

**Key Traits**:
- `ModelContextProtocolRequest` - Paired request/response types with `METHOD`, `Params`, `Result`
- `ModelContextProtocolNotification` - One-way messages with `METHOD`, `Params`

**Core Message Types**:

| Request | Description |
|---------|-------------|
| `InitializeRequest` | Handshake and capability negotiation |
| `ListToolsRequest` | Enumerate available tools |
| `CallToolRequest` | Invoke a tool with arguments |
| `ListResourcesRequest` | Enumerate available resources |
| `ReadResourceRequest` | Fetch resource contents |
| `ListPromptsRequest` | Enumerate prompt templates |
| `GetPromptRequest` | Fetch expanded prompt |

**Content Types**: `TextContent`, `ImageContent`, `AudioContent`, `EmbeddedResource`, `ResourceLink`

**JSON-RPC Types**: `JSONRPCRequest`, `JSONRPCResponse`, `JSONRPCError`, `JSONRPCNotification`

### Things to Know

- Schema version is `2025-06-18` (MCP_SCHEMA_VERSION constant)
- Types derive `Serialize`, `Deserialize`, `JsonSchema`, and `TS` (TypeScript)
- The `RequestId` type supports both string and integer IDs
- `TryFrom` implementations convert raw JSON-RPC to typed enums
- Includes comprehensive tool annotation hints (destructive, idempotent, read-only)

Created and maintained by Nori.
