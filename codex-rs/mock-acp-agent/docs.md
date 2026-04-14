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

**Environment Variable Echo**: The `MOCK_AGENT_ECHO_ENV` env var causes the mock agent's `prompt()` handler to respond with `ENV:<name>=<value>` (or `ENV:<name>=<unset>` if the variable is absent). Used by `test_codex_home_not_inherited_by_agent_subprocess` in `@/codex-rs/acp/src/connection/tests.rs` to verify that the parent's `CODEX_HOME` is not inherited by the spawned ACP subprocess.

**Prompt Echo**: The `MOCK_AGENT_ECHO_PROMPT` env var causes the mock agent's `prompt()` handler to echo back the full prompt text it received. Used by session context tests in `@/codex-rs/acp/src/backend/tests/part5.rs` to verify that `AcpBackendConfig.session_context` is correctly prepended to the first user prompt and consumed after that.

**Stuck Tool Calls (No Completion)**: The `MOCK_AGENT_STUCK_TOOL_CALLS` env var triggers a scenario where 3 Read tool calls are sent with `Pending` status but never receive completion updates. After a short delay the agent sends its final text response and ends the turn. This reproduces the frozen-display bug where incomplete ExecCells fill the viewport and block `insert_history_lines()` from rendering the agent's text. The fix under test is `finalize_active_cell_as_failed()` in `@/codex-rs/tui/src/chatwidget.rs`.

**Runaway Search Snapshot Amplification**: The `MOCK_AGENT_RUNAWAY_SEARCH` env var triggers a deterministic Search tool-call stream that repeatedly emits `InProgress` updates for the **same** `call_id` while the text artifact grows cumulatively on every update. Tunables:
- `MOCK_AGENT_RUNAWAY_SEARCH_UPDATES` -- number of `ToolCallUpdate(InProgress)` events to emit
- `MOCK_AGENT_RUNAWAY_SEARCH_LINES_PER_UPDATE` -- how many search-result lines to append per update
- `MOCK_AGENT_RUNAWAY_SEARCH_LINE_LEN` -- target width for each generated result line
- `MOCK_AGENT_RUNAWAY_SEARCH_DELAY_MS` -- delay between updates
- `MOCK_AGENT_RUNAWAY_SEARCH_SKIP_COMPLETION` -- if set, do not send a final `Completed` update
- `MOCK_AGENT_RUNAWAY_SEARCH_SKIP_FINAL_TEXT` -- if set, do not send a final text chunk

Used by `@/codex-rs/tui-pty-e2e/tests/acp_runaway_search.rs` to reproduce the current ACP backend bug where one streaming search is normalized and recorded as many full snapshots, eventually crashing `nori` under constrained memory.

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

**Skipped-Begin / Generic Tool Call**: The `MOCK_AGENT_GENERIC_TOOL_CALL` env var triggers a scenario where a `ToolCall` is sent with a generic title ("Terminal") and no `raw_input`. The ACP translation layer in `@/codex-rs/acp/` skips emitting `ExecCommandBegin` for such calls (no useful display info). On completion, only `ExecCommandEnd` is emitted with the resolved title. This tests the TUI's `handle_exec_end_now` `None` branch -- that it uses `ev.command` from the End event instead of falling back to the raw `call_id`.

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
