# Current Progress

## Completed: Fix stuck ExecCell causing frozen display

### Root Cause
In ACP, tool call End events (ExecCommandEnd) arrive on a separate async channel from the agent's PromptResponse. The `turn_finished` gate in `on_agent_message()` correctly blocks NEW tool events from appearing after the agent message, but it also silently discards End events for ALREADY-STARTED tool calls. This leaves incomplete ExecCells stuck in `active_cell`, filling the viewport and blocking `insert_history_lines()` from rendering the agent's text response.

### Fix
Two changes to `event_handlers.rs`:
1. **`on_agent_message()`**: Added `finalize_active_cell_as_failed()` and `pending_exec_cells.drain_failed()` to clean up incomplete tool cells when the agent message arrives. This frees the viewport immediately.
2. **`on_task_complete()`**: Added `finalize_active_cell_as_failed()` as a safety net for cases without a preceding AgentMessage.

### Tests Added (part6.rs)
- `task_complete_finalizes_stuck_active_cell` - Safety net test
- `agent_message_finalizes_incomplete_active_cell` - Primary fix test
- `agent_message_finalizes_multiple_incomplete_cells` - Multi-cell cleanup
- `streaming_with_stuck_exec_cell_finalized_on_task_complete` - Streaming scenario

## Completed: E2E test for stuck ExecCell scenario

### What was added
A PTY-driven E2E test that exercises the real `nori` binary, verifying the stuck ExecCell fix works end-to-end (not just in unit tests).

### New mock agent scenario: `MOCK_AGENT_STUCK_TOOL_CALLS`
Added to `mock-acp-agent/src/main.rs`. Sends 3 Read tool calls (Begin only, no completions) followed by final text. This reproduces the exact race condition where tool call End events never arrive before the agent message.

### E2E test: `test_stuck_tool_calls_dont_block_agent_message`
Added to `tui-pty-e2e/tests/acp_tool_calls.rs`. Verifies:
1. The agent's final text message renders (not blocked by stuck ExecCells)
2. The stuck tool cells are finalized and visible as "Explored"
3. The prompt indicator returns after the turn completes
4. Snapshot captures the correct visual rendering

### Remaining from APPLICATION-SPEC
- Monitor for any remaining display issues in production
