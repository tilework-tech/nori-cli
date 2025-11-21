# Noridoc: ACP Module

Path: @/codex-rs/acp

### Overview

- Implements Agent Context Protocol (ACP) for Codex to communicate with external AI agent subprocesses
- Provides JSON-RPC 2.0-based IPC over stdin/stdout pipes
- Includes `AcpModelClient` for high-level streaming interaction with ACP agents
- Manages agent lifecycle, initialization handshake, and stderr capture for diagnostic logging
- Exports `init_file_tracing()` for file-based structured logging at DEBUG level

### How it fits into the larger codebase

- Used by `@/codex-rs/core/src/client.rs` to spawn and communicate with ACP-compliant agents
- `AcpModelClient` is designed to mirror the `ModelClient` interface for future core integration
- Enables Codex to delegate AI operations to external providers (Claude, Gemini, etc.) that implement the ACP specification
- Complements the existing OpenAI-style API path in core by providing an alternative subprocess-based agent model
- Uses channel-based streaming pattern (mpsc) consistent with core's `ResponseStream`
- Provides structured error handling via JSON-RPC error responses that core translates to user-facing messages
- TUI and other clients can access captured stderr for displaying agent diagnostic output

### Core Implementation

**High-Level API:** `AcpModelClient::stream()` in `@/codex-rs/acp/src/acp_client.rs`

- Encapsulates the full flow: spawn, initialize, session/new, session/prompt, stream notifications, complete
- Returns an `AcpStream` implementing the futures `Stream` trait for async iteration
- Events are delivered via `AcpEvent` enum: `TextDelta`, `ReasoningDelta`, `Completed`, `Error`
- Uses mpsc channel (capacity 16) for backpressure-aware event delivery

**Low-Level Entry Point:** `AgentProcess::spawn()` in `@/codex-rs/acp/src/agent.rs`

- Creates a tokio subprocess with piped stdin/stdout/stderr
- Spawns a detached tokio task to asynchronously read stderr lines into a thread-safe buffer
- Exposes `transport_mut()` for direct transport access when streaming notifications

**Protocol Flow (via AcpModelClient):**

```
AcpModelClient                AgentProcess                Agent Subprocess
     |                             |                            |
     |--- spawn agent ------------>|--- Command::spawn() ------>|
     |                             |                            |
     |--- initialize() ----------->|--- JSON-RPC "initialize" ->|
     |                             |<-- JSON-RPC response ------|
     |                             |                            |
     |--- session/new ------------>|--- JSON-RPC request ------>|
     |                             |<-- sessionId response -----|
     |                             |                            |
     |--- session/prompt --------->|--- JSON-RPC request ------>|
     |                             |<-- session/update notif ---|  (TextDelta)
     |                             |<-- session/update notif ---|  (TextDelta)
     |                             |<-- JSON-RPC response ------|
     |                             |                            |
     |--- kill() ----------------->|--- SIGKILL --------------->|
```

**Key Components:**

- `AcpModelClient` in `@/codex-rs/acp/src/acp_client.rs` - High-level client for streaming prompt responses
- `AcpStream` in `@/codex-rs/acp/src/acp_client.rs` - Futures-compatible stream wrapping mpsc receiver
- `StdioTransport` in `@/codex-rs/acp/src/transport.rs` - Serializes/deserializes JSON-RPC messages over async streams
- `JsonRpcRequest/Response/Notification` in `@/codex-rs/acp/src/protocol.rs` - Protocol data structures
- `AcpSession` in `@/codex-rs/acp/src/session.rs` - Session state management placeholder
- `get_agent_config()` in `@/codex-rs/acp/src/registry.rs` - Maps provider names to subprocess commands and args
- `init_file_tracing()` in `@/codex-rs/acp/src/tracing_setup.rs` - Initializes file-based tracing subscriber

### Things to Know

**Provider Registry and Name Normalization:**

The `get_agent_config()` function in `registry.rs` accepts provider names in multiple formats:
- Canonical IDs: "mock-acp", "gemini-acp" (lowercase with hyphens)
- Display names: "Mock ACP", "Gemini ACP" (from `ModelProviderInfo.name` in `@/codex-rs/core/src/model_provider_info.rs`)
- Mixed case variations: "GeMiNi-AcP"

Names are normalized by converting to lowercase and replacing spaces with hyphens before matching. This allows `@/codex-rs/core/src/client.rs` to pass `provider.name` directly to `get_agent_config()` without transformation. The registry maps normalized names to `AcpAgentConfig` structs containing the subprocess command and arguments.

**Streaming Notification Pattern:**

The `stream_prompt()` function handles interleaved messages from the agent:
- Uses `write_raw()` and `read_line()` for direct transport access during streaming
- Distinguishes responses (have `id`, no `method`) from notifications (have `method`)
- Processes `session/update` notifications with `sessionUpdate` types: `agent_message_chunk` and `agent_thought_chunk`
- Loops until a JSON-RPC response is received, then extracts `stopReason` and sends `Completed` event

**Session Lifecycle:**

Each `AcpModelClient::stream()` call spawns a fresh agent process:
- Agent is initialized with hardcoded capabilities: `fs.readTextFile`, `fs.writeTextFile`, `terminal`
- Session created via `session/new` with `cwd` and empty `mcpServers`
- Prompt sent via `session/prompt` with text content block format
- Agent is killed after stream completes or errors

**Stderr Capture Implementation:**

- Buffer uses `Arc<Mutex<Vec<String>>>` for thread-safe access between reader task and caller
- Bounded at 500 lines with FIFO eviction when full
- Individual lines truncated to 10KB
- Reader task runs until EOF or error, logging warnings via tracing

**Transport Layer Extensions:**

`StdioTransport` includes low-level methods for streaming:
- `write_raw(&str)` - Write JSON string directly to stdin
- `read_line()` - Read single line from stdout
- Used by `stream_prompt()` for notification-aware communication

**File-Based Tracing:**

The `init_file_tracing()` function in `@/codex-rs/acp/src/tracing_setup.rs` provides structured file logging:
- Sets global tracing subscriber that writes to a user-specified file path
- Filters at DEBUG level and above (TRACE is excluded)
- Uses non-blocking file appender for async-safe writes
- Creates parent directories automatically if they don't exist
- Returns error on re-initialization since global subscriber can only be set once per process
- Guard is intentionally leaked via `std::mem::forget()` to keep non-blocking writer alive for program lifetime
- ANSI colors disabled for clean file output
- Automatically initialized by the CLI (`@/codex-rs/cli/src/main.rs`) at startup, writing to `.codex-acp.log` in the current working directory

**Test coverage:**

- Thin slice integration tests in `@/codex-rs/acp/tests/thin_slice.rs` verify end-to-end streaming with mock agent
- Unit tests in `agent.rs` use shell commands to test stderr capture, buffer overflow, and line truncation
- Integration tests in `@/codex-rs/acp/tests/integration.rs` test with actual mock-acp-agent binary
- Tracing integration test in `@/codex-rs/acp/tests/tracing_test.rs` validates file creation, log level filtering, and re-initialization error handling
- TUI black-box tests in `@/codex-rs/tui-integration-tests` exercise full application flow including ACP protocol

Created and maintained by Nori.
