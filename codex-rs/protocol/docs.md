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

**Core Types:**

```rust
// Operation sent to conversation
pub enum Op {
    UserTurn { items, cwd, approval_policy, ... },
    Interrupt,
    Shutdown,
    // ...
}

// Event received from conversation
pub struct Event {
    pub id: String,
    pub msg: EventMsg,
}

pub enum EventMsg {
    SessionConfigured { ... },
    TurnStart { ... },
    Delta { ... },
    TurnComplete { ... },
    Error { ... },
    ShutdownComplete,
    // ...
}
```

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
- `ResponseItem` wraps different response content types (text, tool calls, reasoning)
- `TokenUsage` tracks input/output/cache token counts

**Approval Policy:**

`AskForApproval` enum controls when user confirmation is required:
- `UnlessTrusted`: Auto-approve known-safe read-only commands only
- `OnFailure`: Auto-approve in sandbox, escalate failures to user
- `OnRequest`: (Default) Model decides when to request approval
- `Never`: Fully autonomous (for automation)

**Sandbox Modes:**

`SandboxMode` in `config_types`:
- `ReadOnly`: No writes allowed
- `WorkspaceWrite`: Writes to cwd only
- `DangerFullAccess`: No restrictions

**ConversationId:**

The `ConversationId` type is a wrapper around UUID used to identify sessions. It provides string conversion and validation.

Created and maintained by Nori.
