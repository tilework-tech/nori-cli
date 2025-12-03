# Noridoc: app-server

Path: @/codex-rs/app-server

### Overview

The `codex-app-server` crate provides a JSON-RPC based server interface for Codex, communicating over stdin/stdout. It enables IDE integrations and other clients to interact with Codex programmatically using a structured message protocol. The server handles session management, model requests, and event streaming.

### How it fits into the larger codebase

App Server is invoked via `codex app-server`:

- **Uses** `codex-core` for conversation management and configuration
- **Uses** `codex-app-server-protocol` for message type definitions
- **Shares** authentication and config infrastructure with TUI/exec
- **Enables** the VS Code/Cursor/Windsurf IDE extensions

The protocol supports both v1 (legacy) and v2 (thread-based) API versions.

### Core Implementation

**Entry Point:**

`run_main()` in `lib.rs` sets up three concurrent tasks:
1. **stdin reader**: Parses JSON-RPC messages from stdin
2. **processor**: Routes messages through `MessageProcessor`
3. **stdout writer**: Serializes outgoing messages to stdout

**Message Processing:**

`message_processor.rs` handles:
- `process_request()`: Method calls requiring responses
- `process_notification()`: One-way messages
- `process_response()`: Responses to server-initiated requests
- `process_error()`: Error handling

**Codex Integration:**

`codex_message_processor.rs` bridges app-server protocol to core:
- Creates/resumes conversations via `ConversationManager`
- Translates protocol messages to `Op` operations
- Streams `Event` responses back as notifications

### Things to Know

**Protocol Versions:**

v2 (thread-based) methods:
- `thread/start`, `thread/resume`
- `turn/start`, `turn/interrupt`
- `model/list`, `account/status`
- `thread/list`, `thread/archive`

v1 (legacy) methods maintained for compatibility.

**Bespoke Event Handling:**

`bespoke_event_handling.rs` contains special-case event transformations for protocol compatibility.

**Fuzzy File Search:**

`fuzzy_file_search.rs` provides file finding capabilities for IDE autocomplete features.

**Model List:**

`models.rs` handles model enumeration and capability reporting.

**Channel Capacity:**

Uses 128-message bounded channels for stdin/stdout communication, balancing throughput and memory.

**Error Codes:**

`error_code.rs` defines JSON-RPC error codes for various failure conditions.

Created and maintained by Nori.
