# Noridoc: mock-acp-agent

Path: @/codex-rs/mock-acp-agent

### Overview

The mock-acp-agent crate provides a mock ACP agent binary for testing the Nori TUI. It simulates an AI agent's behavior including streaming responses, tool calls, and permission requests.

### How it fits into the larger codebase

Used by `@/codex-rs/tui-pty-e2e/` for end-to-end integration testing. The mock agent is spawned as a subprocess and communicates over stdin/stdout using the ACP protocol.

### Core Implementation

**MockAgent**: Implements the ACP `Agent` trait, handling:
- Session creation/destruction
- Prompt processing with simulated responses
- Tool call execution (shell, apply_patch, etc.)
- Permission request/response flow
- Cancellation

**Mock Behaviors**: Controlled via environment variables that the E2E tests set on the mock agent process. Each env var activates a specific behavior scenario. Key scenarios include multi-turn conversations, tool call streaming, permission requests, file operations, and race condition simulations.

**Race Condition Simulation**: The `MOCK_AGENT_TOOL_CALLS_DURING_FINAL_STREAM` env var triggers a scenario that reproduces the timing where tool call completions arrive while the final text response is streaming. This is structured in phases:
1. Tool calls that complete before text streaming starts (rendered normally)
2. Text streaming begins (activates the TUI's stream_controller)
3. Additional tool calls begin and complete during text streaming (get deferred by the TUI's interrupt queue)
4. Final text chunk sent and turn ends

This simulates the real-world race condition that the `InterruptManager.flush_completions_and_clear()` in `@/codex-rs/tui/src/chatwidget.rs` handles at task completion.

**Client Requests**: Outbound requests to the client:
- `ReadFile` - Request file contents
- `WriteFile` - Request file write
- `RequestPermission` - Ask user for tool approval

### Things to Know

- The mock is a binary crate (no lib.rs) intended only for testing
- Uses the same ACP protocol as real agents for realistic testing
- Simulates streaming with configurable chunk delays
- Supports permission options (accept, deny, skip)
- Session state is tracked per-session ID
- Sleep durations between mock events are tuned to create reliable timing in E2E tests

Created and maintained by Nori.
