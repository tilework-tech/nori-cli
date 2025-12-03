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

## Critical Changes to Forthcoming Work

- **Approval bridging is incomplete**: The `submit()` method handles `Op::ExecApproval` and `Op::PatchApproval` by storing decisions in `pending_approvals`, but the actual bridging logic to forward these to the ACP connection's `ClientDelegate` is not yet wired up
- **MCP servers config**: The plan mentions passing `config.mcp_servers` to `NewSessionRequest`, but this is not yet implemented
- **Sandbox policy**: Currently read from config but not used - needs to be passed to agent
- **Error events need refinement**: Currently sends generic error text for unsupported Ops; may need structured error types
- **E2E tests not yet written**: The plan lists tests in `tui-pty-e2e/tests/acp_mode.rs` that still need implementation
- **Tool call display**: `ToolCall` and `ToolCallUpdate` translation returns empty vec - needs implementation to show tool execution in TUI
