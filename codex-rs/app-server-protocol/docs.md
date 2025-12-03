# Noridoc: app-server-protocol

Path: @/codex-rs/app-server-protocol

### Overview

The `codex-app-server-protocol` crate defines the JSON-RPC message types for communication between the app server and IDE clients. It includes both v1 (legacy) and v2 (thread-based) protocol definitions, plus code generation utilities for TypeScript bindings.

### How it fits into the larger codebase

App server protocol is used by:

- **App server** for message parsing/serialization
- **IDE extensions** (VS Code, Cursor, Windsurf) via generated TypeScript types
- **Export utilities** for TypeScript and JSON Schema generation

### Core Implementation

**Key Files:**

- `protocol/v1.rs`: Legacy protocol messages
- `protocol/v2.rs`: Thread-based protocol messages
- `protocol/common.rs`: Shared types
- `jsonrpc_lite.rs`: JSON-RPC base structures
- `export.rs`: TypeScript/JSON Schema generation

**Protocol Methods (v2):**

```
thread/start, thread/resume, thread/list, thread/archive
turn/start, turn/interrupt
model/list
account/status
```

### Things to Know

**Code Generation:**

`export.rs` and `bin/export.rs` provide:
- TypeScript type generation using `ts-rs`
- JSON Schema generation using `schemars`
- Prettier formatting for generated code

**Auth Modes:**

`AuthMode` enum distinguishes:
- `ChatGPT`: OAuth-based
- `ApiKey`: Direct API key

**TypeScript Output:**

Generated types go to IDE extension codebases for type-safe client implementation.

Created and maintained by Nori.
