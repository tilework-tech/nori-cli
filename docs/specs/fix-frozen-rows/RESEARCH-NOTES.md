# Research Notes

## Problem Summary

The APPLICATION-SPEC describes a persistent bug where the agent's final response fails to render after tool calls. The user sees many tool calls "frozen" on screen, and only after manually interrupting does the previous message appear.

## Root Cause (identified in previous work)

In ACP, tool call End events (`ExecCommandEnd`) arrive on a separate async channel from the agent's `PromptResponse`. The `turn_finished` gate in `on_agent_message()` blocks NEW tool events but also silently discards End events for ALREADY-STARTED tool calls. This leaves incomplete ExecCells stuck in `active_cell`, filling the viewport and blocking `insert_history_lines()` from rendering the agent's text response.

## Fix Already Implemented (unit tests only)

Two changes to `event_handlers.rs`:
1. `on_agent_message()`: Added `finalize_active_cell_as_failed()` and `pending_exec_cells.drain_failed()`
2. `on_task_complete()`: Added `finalize_active_cell_as_failed()` as safety net

Unit tests in `part6.rs` cover the core logic.

## Remaining Work: E2E Test

The APPLICATION-SPEC explicitly requires a real E2E test (not mocks) that reproduces the frozen display scenario. The existing `tui-pty-e2e` infrastructure provides:

- PTY-based testing via `portable_pty` + `vt100::Parser`
- Mock ACP agent (`mock-acp-agent`) with env-var-controlled scenarios
- Existing test patterns for tool call rendering in `acp_tool_calls.rs`

### Gap Analysis

Existing mock agent scenarios:
- `MOCK_AGENT_SEND_TOOL_CALL` - Basic tool call (completes normally)
- `MOCK_AGENT_INTERLEAVED_TOOL_CALL` - Text during tool execution
- `MOCK_AGENT_MULTI_CALL_EXPLORING` - Multiple exploring calls, out-of-order completion
- `MOCK_AGENT_TOOL_CALLS_DURING_FINAL_STREAM` - Completions arrive during text streaming
- `MOCK_AGENT_ORPHAN_TOOL_CELLS` - Cascade deferral bug
- `MOCK_AGENT_GENERIC_TOOL_CALL` - No raw_input, skipped Begin

**Missing scenario:** Tool calls that BEGIN but whose END events arrive AFTER the agent message (or never arrive). This is the exact stuck-cell scenario the spec describes.

### New Mock Scenario Needed: `MOCK_AGENT_STUCK_TOOL_CALLS`

Sequence:
1. Multiple tool calls start (send ToolCall with Pending status)
2. Agent sends final text response (these tool calls never complete before the text)
3. Turn ends

This exercises `finalize_active_cell_as_failed()` in the E2E path.

### E2E Test Assertions

1. The agent's final text message MUST render (not blocked by stuck cells)
2. The stuck tool cells should appear as failed/completed (not frozen)
3. No "frozen" spinner should remain visible after the turn ends
