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

- Uses `agent_client_protocol` crate for protocol implementation
- Runs on single-threaded tokio runtime with `LocalSet` for `!Send` futures
- Communicates via stdin/stdout with newline-delimited JSON-RPC messages

**Protocol Methods:**

| Method | Behavior |
|--------|----------|
| `initialize` | Returns mock capabilities, emits "Mock agent: initialize" to stderr |
| `new_session` | Generates incrementing session IDs |
| `prompt` | Sends two text chunks, optionally reads files via client |
| `cancel` | Sets flag to stop streaming |

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
| `MOCK_AGENT_REQUEST_FILE` | Reads file path via client during prompt |
| `MOCK_AGENT_STREAM_UNTIL_CANCEL` | Continuously streams until cancel notification |
| `MOCK_AGENT_STDERR_COUNT` | Emits N lines of `MOCK_AGENT_STDERR_LINE:{i}` to stderr during prompt |
| `MOCK_AGENT_RESPONSE` | Custom response text instead of default "Test message 1/2" (added for TUI testing) |
| `MOCK_AGENT_DELAY_MS` | Millisecond delay before completing stream to simulate realistic streaming (added for TUI testing) |

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

**Binary Name:**

Cargo renames hyphens to underscores in binary names, so the built artifact is `mock_acp_agent` (not `mock-acp-agent`). Tests in `@/codex-rs/acp/tests/integration.rs` use `mock_agent_binary_path()` helper to locate it.

Created and maintained by Nori.
