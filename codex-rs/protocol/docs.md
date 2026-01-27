# Noridoc: codex-protocol

Path: @/codex-rs/protocol

### Overview

The protocol crate defines the internal message types used between Nori components. It specifies operations (`Op`), events (`EventMsg`), and approval-related types that flow between the TUI, core, and backend layers.

### How it fits into the larger codebase

This crate provides the contract between:
- `@/codex-rs/tui/` - consumes events, sends operations
- `@/codex-rs/core/` - processes operations, emits events
- `@/codex-rs/acp/` - translates ACP protocol to/from these types

The crate is a pure type definition library with serde serialization support.

### Core Implementation

**Operations** (`protocol.rs`): Commands sent from TUI to core:

| Op | Purpose |
|----|---------|
| `Configure` | Set session configuration |
| `UserTurn` | Send user message |
| `ApproveTool` / `RejectTool` | Handle approval requests |
| `CancelTurn` | Cancel current generation |

**Events** (`events.rs`): Messages from core to TUI:

| Event | Purpose |
|-------|---------|
| `TaskStarted` | Turn began processing |
| `AgentMessage` | Streaming AI response content |
| `ToolCall` / `ToolResult` | Tool invocation lifecycle |
| `ApprovalRequired` | User approval needed |
| `TaskComplete` | Turn finished |

**Approval Types** (`approvals.rs`): Defines `ExecApprovalRequestEvent` for shell commands and `ApplyPatchApprovalRequestEvent` for file edits. The `ReviewDecision` enum captures user responses.

**Conversation Types**: `ConversationId`, `ConversationStoredState`, `SessionSource` for session management.

### Things to Know

- Types are serde-serializable for persistence and wire transfer
- The `SandboxPolicy` enum defines allowed sandbox configurations
- `ResponseItem` wraps different response content types (text, tool calls, reasoning)
- `TokenUsage` tracks input/output/cache token counts

Created and maintained by Nori.
