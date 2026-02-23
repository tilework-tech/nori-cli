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

**Mock Behaviors**: Controlled via environment variables that the E2E tests set on the mock agent process. Each env var activates a specific behavior scenario. Key scenarios include multi-turn conversations, tool call streaming, permission requests, file operations, race condition simulations, and session lifecycle behaviors.

**Session Lifecycle Testing**: Several env vars control `session/load` behavior for testing the resume path in `@/codex-rs/acp/src/backend.rs`:
- `MOCK_AGENT_SUPPORT_LOAD_SESSION` -- when set, the agent advertises `load_session: true` in its capabilities during `initialize()`
- `MOCK_AGENT_LOAD_SESSION_FAIL` -- when set, the `load_session()` handler returns an error instead of succeeding, allowing tests to exercise the runtime-failure fallback path
- `MOCK_AGENT_LOAD_SESSION_NOTIFICATION_COUNT` -- when set to an integer N, the `load_session()` handler sends N text-chunk notifications (via `send_text_chunk()`) before returning, simulating history replay with a configurable volume of events. Used to test the deferred-relay pattern in `resume_session()` that prevents deadlocks when the notification count exceeds the bounded `event_tx` channel capacity.

**Race Condition Simulation**: The `MOCK_AGENT_TOOL_CALLS_DURING_FINAL_STREAM` env var triggers a scenario that reproduces the timing where tool call completions arrive while the final text response is streaming. This is structured in phases:
1. Tool calls that complete before text streaming starts (rendered normally)
2. Text streaming begins (activates the TUI's stream_controller)
3. Additional tool calls begin and complete during text streaming (get deferred by the TUI's interrupt queue)
4. Final text chunk sent and turn ends

This simulates the real-world race condition that the `InterruptManager.flush_completions_and_clear()` in `@/codex-rs/tui/src/chatwidget.rs` handles at task completion.

**Cascade Deferral / Orphan Cell Reproduction**: The `MOCK_AGENT_ORPHAN_TOOL_CELLS` env var triggers a scenario where a tool Begin is cascade-deferred (deferred because the queue is non-empty, even though the stream has ended). The sequence:
1. Tool A Begin handled immediately (no stream active)
2. Text streaming starts (activates `stream_controller`)
3. Tool A End deferred (stream active), making the queue non-empty
4. Tool B Begin deferred (queue non-empty -- cascade deferral)
5. Tool B End deferred
6. Turn ends -- `flush_completions_and_clear` must discard both Begin-B and End-B to avoid creating an orphan `ExecCell` with the raw `call_id` as the command name

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
