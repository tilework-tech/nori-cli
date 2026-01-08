# Noridoc: Mock ACP Agent

Path: @/codex-rs/mock-acp-agent

### Overview

- Standalone Rust binary implementing a mock ACP-compliant agent for testing
- Implements the full ACP protocol (initialize, authenticate, new_session, prompt, cancel)
- Provides configurable behavior via environment variables for test scenarios

### How it fits into the larger codebase

- Used by integration tests in `@/codex-rs/acp/tests/integration.rs` to test ACP protocol flow
- Used by TUI black-box tests in `@/codex-rs/tui-pty-e2e` as the `--model mock-acp-agent` backend
- Enables end-to-end testing of `AgentProcess` without requiring real AI providers
- Produces diagnostic stderr output that tests use to verify stderr capture functionality
- Not shipped in production; exists solely for development and CI testing

### Core Implementation

**Entry Point:** `main()` in `@/mock-acp-agent/src/main.rs`

- Uses `agent_client_protocol` v0.9 crate with `unstable` feature for model switching support
- Uses builder patterns for protocol types (e.g., `ToolCall::new()`, `ModelInfo::new()`)
- Runs on single-threaded tokio runtime with `LocalSet` for `!Send` futures
- Communicates via stdin/stdout with newline-delimited JSON-RPC messages

**Protocol Methods:**

| Method | Behavior |
|--------|----------|
| `initialize` | Returns mock capabilities, emits "Mock agent: initialize" to stderr |
| `new_session` | Generates incrementing session IDs, returns `session_model_state` with 3 test models |
| `prompt` | Sends two text chunks, optionally reads files via client |
| `cancel` | Sets flag to stop streaming |
| `set_session_model` | Updates the current model ID in the session |

**Internal Architecture:**

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│ AgentSideConn   │<--->│ MockAgent        │<--->│ Channels        │
│ (I/O handling)  │     │ (impl Agent)     │     │ (updates/reqs)  │
└─────────────────┘     └──────────────────┘     └─────────────────┘
```

### Things to Know

**Environment Variables for Test Control:**

| Variable | Effect |
|----------|--------|
| `MOCK_AGENT_HANG` | Sleeps 60s during initialize (timeout testing) |
| `MOCK_AGENT_REQUIRE_AUTH` | Returns error code -32000 "Authentication required" during initialize (auth failure testing) |
| `MOCK_AGENT_REQUEST_FILE` | Reads file path via client during prompt |
| `MOCK_AGENT_STREAM_UNTIL_CANCEL` | Continuously streams until cancel notification |
| `MOCK_AGENT_STDERR_COUNT` | Emits N lines of `MOCK_AGENT_STDERR_LINE:{i}` to stderr during prompt |
| `MOCK_AGENT_RESPONSE` | Custom response text instead of default "Test message 1/2" (added for TUI testing) |
| `MOCK_AGENT_DELAY_MS` | Millisecond delay before completing stream to simulate realistic streaming (added for TUI testing) |
| `MOCK_AGENT_REQUEST_PERMISSION` | Triggers a permission request to the client before responding, used for testing ACP approval bridging |
| `MOCK_AGENT_SEND_TOOL_CALL` | Sends a tool call sequence (pending → in_progress → completed) for testing tool call display |
| `MOCK_AGENT_WRITE_FILE` | Path to write via client's `fs/write_text_file` method - combines with `MOCK_AGENT_WRITE_CONTENT` to test file write implementation; on success, sends `File written successfully` and reads back file to verify |
| `MOCK_AGENT_WRITE_CONTENT` | Content to write when `MOCK_AGENT_WRITE_FILE` is set (defaults to "default content") |
| `MOCK_AGENT_MULTI_CALL_EXPLORING` | Sends 3 Read tool calls with interleaved text streaming and out-of-order completion (call-2, call-3, call-1) to test multi-call exploring cell handling |
| `MOCK_AGENT_NO_FINAL_TEXT` | Suppresses final text message after tool calls complete; combine with `MOCK_AGENT_MULTI_CALL_EXPLORING` to test immediate flush without subsequent text |

**Stderr Output for Testing:**

The agent writes to stderr at key points for observability:
- "Mock agent: initialize" on initialization
- "Mock agent: new_session id={id}" on session creation
- "Mock agent: prompt" on prompt request
- "Mock agent: cancel" on cancellation
- `MOCK_AGENT_STDERR_LINE:{i}` lines when `MOCK_AGENT_STDERR_COUNT` is set

This allows tests to verify stderr capture by checking for known strings.

**File Read Client Request:**

The agent can request file reads from the client via `conn.read_text_file()`. This exercises bidirectional client<->agent communication. Set `MOCK_AGENT_REQUEST_FILE=/path/to/file` to trigger.

**Permission Request Client Request:**

The agent can request permission from the client via `conn.request_permission()`. When `MOCK_AGENT_REQUEST_PERMISSION` is set:
- Creates a `ToolCallUpdate` describing a shell command execution
- Provides two `PermissionOption` choices: "Allow" (`AllowOnce`) and "Reject" (`RejectOnce`)
- Blocks waiting for the client's response, exercising the full approval bridging flow
- Sends a confirmation message indicating which option was selected

**Model State for Testing:**

The agent provides test models in `session_model_state` for model switching tests:
- `mock-model-1` (default): "Mock Model 1"
- `mock-model-2`: "Mock Model 2"
- `mock-model-3`: "Mock Model 3"

The current model ID is tracked internally and updated via `set_session_model()`.

**Binary Name:**

Cargo renames hyphens to underscores in binary names, so the built artifact is `mock_acp_agent` (not `mock-acp-agent`). Tests in `@/codex-rs/acp/tests/integration.rs` use `mock_agent_binary_path()` helper to locate it.


**Output Formatting for TUI Testing:**

The agent wraps success/failure messages with newlines to prevent text wrapping issues in TUI tests:
- File write success: `"\nFile written successfully\n"` followed by `"\nVerified content:\n{content}\n"`
- File write failure: `"\nFailed to write file: {error}\n"`

This formatting ensures test assertions don't break when terminal width causes line wrapping (e.g., 80-column TUI terminals). Without the newlines, long messages could wrap mid-line and break string matching in test assertions.

Created and maintained by Nori.
