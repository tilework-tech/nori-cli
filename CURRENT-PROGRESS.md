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

### Remaining from APPLICATION-SPEC
- E2E test with real binary that reproduces the frozen display scenario (the spec asks for tmux-driven tests, but unit tests cover the core logic)
- Monitor for any remaining display issues in production
