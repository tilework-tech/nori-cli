# ACP TUI Backend Integration - Handoff

## What Was Done

- Created `acp/src/backend.rs` with `AcpBackend` and `AcpBackendConfig` types
- Added `AcpBackend::spawn()` for initializing ACP connection and session
- Added `AcpBackend::submit(Op)` for translating Codex Ops to ACP actions
- Implemented `translate_session_update_to_events()` to convert ACP `SessionUpdate` to `codex_protocol::Event`
- Added synthetic `SessionConfigured` event emission on backend spawn
- Exported new types from `acp/src/lib.rs`
- Modified `tui/src/chatwidget/agent.rs` with ACP mode detection and `spawn_acp_agent()`
- Added `codex-acp` dependency to `tui/Cargo.toml`
- Updated `acp/docs.md` and `tui/docs.md` with backend adapter documentation

## Key Learnings

- ACP library v0.7 uses schema v0.6.2 - type names and field names differ from what might be expected
- `ToolCall` uses `id` field (not `tool_call_id`)
- `ImageContent` requires `uri: Option<String>` field even in tests
- The `agent-client-protocol` library source is at `@other-repos/agent-client-protocol/` - always check there for type definitions
- `LocalBoxFuture` is `!Send`, requiring the dedicated worker thread pattern already in `connection.rs`
- Test snapshot changes for version numbers are pre-existing upstream issues, not caused by this work

## Approval Bridging - COMPLETED

The approval bridging is now working end-to-end:
- Permission requests from ACP agents are displayed in the TUI approval popup
- User decisions are sent back to the agent via `Op::ExecApproval`
- The TUI handles approval requests immediately (not deferred) to avoid deadlock with blocking agent subprocess
- E2E test `test_acp_approval_request_displayed_in_tui` passes

Key changes:
- Modified `tui/src/chatwidget.rs` to handle approval requests immediately
- Added `MOCK_AGENT_REQUEST_PERMISSION` env var support to mock agent
- Removed `#[ignore]` from approval bridging E2E test

## Remaining Work

- **MCP servers config**: The plan mentions passing `config.mcp_servers` to `NewSessionRequest`, but this is not yet implemented
- **Sandbox policy**: Currently read from config but not used - needs to be passed to agent
- **Error events need refinement**: Currently sends generic error text for unsupported Ops; may need structured error types
- **Tool call display**: `ToolCall` and `ToolCallUpdate` translation returns empty vec - needs implementation to show tool execution in TUI (E2E tests exist but fail)
