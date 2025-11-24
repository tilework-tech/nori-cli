# Noridoc: ACP Module

Path: @/codex-rs/acp

### Overview

- Implements Agent Context Protocol (ACP) for Codex to communicate with external AI agent subprocesses
- Uses the official `agent-client-protocol` v0.7 library instead of custom JSON-RPC implementation
- Includes `AcpModelClient` for high-level streaming interaction with ACP agents
- Manages agent lifecycle, initialization handshake, and stderr capture for diagnostic logging
- Exports `init_file_tracing()` for file-based structured logging at DEBUG level
- **Critical**: All ACP operations use !Send futures from `agent-client-protocol`, requiring `LocalSet` contexts

### How it fits into the larger codebase

- Used by `@/codex-rs/core/src/client.rs` to spawn and communicate with ACP-compliant agents via `WireApi::Acp` variant
- `AcpModelClient` is designed to mirror the `ModelClient` interface for future core integration
- Enables Codex to delegate AI operations to external providers (Claude, Gemini, etc.) that implement the ACP specification
- Complements the existing OpenAI-style API path in core by providing an alternative subprocess-based agent model
- Uses channel-based streaming pattern (mpsc) consistent with core's `ResponseStream`
- Provides structured error handling via library's typed error responses that core translates to user-facing messages
- TUI and other clients can access captured stderr for displaying agent diagnostic output
- Re-exports commonly used types from `agent-client-protocol` library for convenience (`Agent`, `Client`, `ClientSideConnection`, request/response types)

### Core Implementation

**High-Level API:** `AcpModelClient::stream()` in `@/codex-rs/acp/src/acp_client.rs`

- Encapsulates the full flow: spawn, initialize, session/new, session/prompt, stream notifications, complete
- Returns an `AcpStream` implementing the futures `Stream` trait for async iteration
- Events are delivered via `AcpEvent` enum: `TextDelta`, `ReasoningDelta`, `Completed`, `Error`
- Uses mpsc channel (capacity 16) for backpressure-aware event delivery
- **Spawns dedicated thread with LocalSet** because agent-client-protocol futures are !Send

**Low-Level Entry Point:** `AgentProcess::spawn()` in `@/codex-rs/acp/src/agent.rs`

- Creates a tokio subprocess with piped stdin/stdout/stderr
- Wraps stdio streams with tokio-util compat layer to bridge tokio's AsyncRead/Write with futures crate traits
- Creates `ClientSideConnection` from agent-client-protocol library, passing `spawn_local` callback for !Send futures
- Spawns detached tokio task to asynchronously read stderr lines into a thread-safe buffer
- Returns `AgentProcess` wrapping the connection, child process, and event channels

**Protocol Flow (via AcpModelClient and Library):**

```
AcpModelClient          AgentProcess           ClientSideConnection        Agent Subprocess
     |                       |                          |                         |
     |--- stream() -------->|                          |                         |
     | (spawns thread +     |                          |                         |
     |  LocalSet)           |                          |                         |
     |                      |--- spawn() -------------->|--- Command::spawn() -->|
     |                      |                          |                         |
     |                      |--- initialize() -------->|--- Agent::initialize -->|
     |                      |                          |<-- InitializeResponse --|
     |                      |                          |                         |
     |                      |--- new_session() ------->|--- Agent::new_session ->|
     |                      |                          |<-- NewSessionResponse --|
     |                      |                          |                         |
     |                      |--- prompt() ------------>|--- Agent::prompt ------>|
     |                      |                          |                         |
     |                      |<-- ClientEvent::SessionUpdate  <-- Client::session_notification()
     |<-- AcpEvent::TextDelta                         |                         |
     |                      |<-- ClientEvent::SessionUpdate  <-- Client::session_notification()
     |<-- AcpEvent::TextDelta                         |                         |
     |                      |                          |<-- PromptResponse ------|
     |<-- AcpEvent::Completed                         |                         |
     |                      |--- kill() -------------->|--- SIGKILL ------------>|
```

**Key Components:**

- `AcpModelClient` in `@/codex-rs/acp/src/acp_client.rs` - High-level client for streaming prompt responses, spawns dedicated thread with LocalSet
- `AcpStream` in `@/codex-rs/acp/src/acp_client.rs` - Futures-compatible stream wrapping mpsc receiver
- `AgentProcess` in `@/codex-rs/acp/src/agent.rs` - Wraps ClientSideConnection, manages subprocess lifecycle and stderr capture
- `AcpClientHandler` in `@/codex-rs/acp/src/client_handler.rs` - Implements Client trait for handling agent callbacks (permission requests, session updates)
- `ClientEvent` enum in `@/codex-rs/acp/src/client_handler.rs` - Forwarding type for client callbacks sent through mpsc channel
- `ClientSideConnection` from `agent-client-protocol` library - Implements Agent trait for sending requests to subprocess
- `AcpSession` in `@/codex-rs/acp/src/session.rs` - Session state management placeholder
- `get_agent_config()` in `@/codex-rs/acp/src/registry.rs` - Maps model names to subprocess commands, args, and provider identifier
- `AcpAgentConfig` in `@/codex-rs/acp/src/registry.rs` - Configuration struct containing provider, command, and args for spawning agent subprocess
- `init_file_tracing()` in `@/codex-rs/acp/src/tracing_setup.rs` - Initializes file-based tracing subscriber

### Things to Know

**LocalSet Requirement and !Send Futures:**

The `agent-client-protocol` library uses !Send futures, requiring all ACP operations to run within a `LocalSet`:
- `AcpModelClient::stream()` spawns a dedicated thread with single-threaded runtime + LocalSet to isolate !Send futures
- All tests wrap execution in `LocalSet::run_until()`
- `AgentProcess::spawn()` provides `spawn_local` callback to ClientSideConnection for spawning !Send tasks
- This isolation prevents !Send futures from leaking into the main Codex runtime which uses multi-threaded tokio
- The thread boundary acts as an isolation layer: Send channel messages cross thread boundaries, !Send futures stay contained

**Compat Layer Requirement:**

Bridging tokio and futures crate async traits requires tokio-util compat layer:
- tokio provides `tokio::io::{AsyncRead, AsyncWrite}`
- agent-client-protocol expects `futures::io::{AsyncRead, AsyncWrite}`
- `TokioAsyncReadCompatExt::compat()` and `TokioAsyncWriteCompatExt::compat_write()` convert between them
- Applied to child process stdin/stdout before passing to ClientSideConnection

**Client Callback Architecture:**

Agent callbacks are handled through a channel-based forwarding pattern:
- `AcpClientHandler` implements the `Client` trait from agent-client-protocol
- Callbacks (`request_permission`, `session_notification`) forward to mpsc channel as `ClientEvent` enum
- `AgentProcess::next_client_event()` exposes the receiver for consuming callbacks
- `AcpModelClient` processes `ClientEvent::SessionUpdate` to emit `AcpEvent::{TextDelta, ReasoningDelta}`
- Permission requests currently auto-cancel (TODO: implement proper permission handling)
- File operations and terminal operations return `method_not_found` errors

**Model Registry and Lookup Architecture:**

The ACP registry in `@/codex-rs/acp/src/registry.rs` is **model-centric** rather than provider-centric:
- `get_agent_config()` accepts model names (e.g., "mock-model", "gemini-flash-2.5") instead of provider names
- Called from `@/codex-rs/core/src/client.rs` with `self.config.model` when handling `WireApi::Acp`
- Returns `AcpAgentConfig` containing three fields:
  - `provider`: Identifies which agent subprocess to spawn (e.g., "mock-acp", "gemini-acp")
  - `command`: Executable path or command name
  - `args`: Arguments to pass to the subprocess
- Model names are normalized to lowercase for case-insensitive matching (e.g., "Gemini-Flash-2.5" → "gemini-flash-2.5")
- Uses exact matching only (no prefix matching) - each model must be explicitly registered
- The `provider` field enables future optimization to determine when existing subprocess can be reused vs when new one must be spawned when switching models

**Session Lifecycle:**

Each `AcpModelClient::stream()` call spawns a fresh agent process:
- Agent is initialized with hardcoded capabilities: `fs.readTextFile`, `fs.writeTextFile`, `terminal`
- Session created via `session/new` with `cwd` and empty `mcpServers`
- Prompt sent via `session/prompt` with text content block format
- Library's `Agent::prompt()` returns when final response received; session updates arrive via Client callbacks
- Agent is killed after stream completes or errors

**Typed Request/Response Pattern:**

The agent-client-protocol library provides typed request/response structs:
- `InitializeRequest`/`InitializeResponse` - Protocol handshake with capability negotiation
- `NewSessionRequest`/`NewSessionResponse` - Session creation with cwd and MCP servers
- `PromptRequest`/`PromptResponse` - Prompt submission with content blocks
- `SessionNotification` - Async notifications for session updates (agent message/thought chunks)
- Eliminates manual JSON-RPC message construction and parsing
- Provides compile-time type safety for protocol conformance

**Stderr Capture Implementation:**

- Buffer uses `Arc<Mutex<Vec<String>>>` for thread-safe access between reader task and caller
- Bounded at 500 lines with FIFO eviction when full
- Individual lines truncated to 10KB
- Reader task runs until EOF or error, logging warnings via tracing

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

- Thin slice integration tests in `@/codex-rs/acp/tests/thin_slice.rs` verify end-to-end streaming with mock agent, wrapped in LocalSet
- Unit tests in `agent.rs` use shell commands to test stderr capture, buffer overflow, and line truncation - all wrapped in LocalSet
- Integration tests in `@/codex-rs/acp/tests/integration.rs` test protocol handshake with typed requests/responses from library
- Tracing integration test in `@/codex-rs/acp/tests/tracing_test.rs` validates file creation, log level filtering, and re-initialization error handling
- TUI black-box tests in `@/codex-rs/tui-integration-tests` exercise full application flow including ACP protocol

**Removed Custom Implementation (~250 lines eliminated):**

Prior to this refactoring, the crate contained custom JSON-RPC implementations that have been replaced by the agent-client-protocol library:
- `protocol.rs` (~120 lines) - Custom `JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcNotification`, `JsonRpcError` types
- `transport.rs` (~130 lines) - Custom `StdioTransport` with manual JSON-RPC serialization/deserialization
- Eliminated manual JSON parsing, error handling, and protocol conformance issues
- Library provides spec compliance, type safety, and automatic protocol updates

Created and maintained by Nori.
