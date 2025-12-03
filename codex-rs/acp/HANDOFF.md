# ACP TUI Backend Integration - Handoff

## Key Learnings

- ACP library v0.7 uses schema v0.6.2 - type names and field names differ from what might be expected
- `ToolCall` uses `id` field (not `tool_call_id`)
- `ImageContent` requires `uri: Option<String>` field even in tests
- The `agent-client-protocol` library source is at `@other-repos/agent-client-protocol/` - always check there for type definitions
- `LocalBoxFuture` is `!Send`, requiring the dedicated worker thread pattern already in `connection.rs`
- Test snapshot changes for version numbers are pre-existing upstream issues, not caused by this work

## Remaining Work

- **MCP servers config**: The plan mentions passing `config.mcp_servers` to `NewSessionRequest`, but this is not yet implemented
- **Sandbox policy**: Currently read from config but not used - needs to be passed to agent
- **Error events need refinement**: Currently sends generic error text for unsupported Ops; may need structured error types
- **Tool call display**: `ToolCall` and `ToolCallUpdate` translation returns empty vec - needs implementation to show tool execution in TUI

## Ignored E2E Tests

| Test | File | Reason |
|------|------|--------|
| `test_submit_prompt_missing_model` | `prompt_flow.rs:44` | Falls back on HTTP model; needs purely ACP launch mode config |
| `test_acp_tool_call_rendered_in_tui` | `acp_tool_calls.rs:51` | Needs `MOCK_AGENT_SEND_TOOL_CALL` env var support and mock fixups |
| `test_acp_tool_call_completion_rendered_in_tui` | `acp_tool_calls.rs:123` | Same as above |
| `test_escape_cancels_streaming` | `streaming.rs:39` | Broken by new TUI event loop; needs cancellation Op support |
| `test_gemini_acp_live_response` | `live_acp.rs:22` | Opt-in live test requiring `GEMINI_API_KEY` |
| `test_claude_acp_live_response` | `live_acp.rs:67` | Opt-in live test requiring `ANTHROPIC_API_KEY` |
