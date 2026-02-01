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
| `Undo` | Undo the most recent turn (sequential pop from snapshot stack) |
| `UndoList` | Request the list of available undo snapshots |
| `UndoTo { index }` | Restore to a specific snapshot by display index (0 = most recent) |

**Events** (`events.rs`): Messages from core to TUI:

| Event | Purpose |
|-------|---------|
| `TaskStarted` | Turn began processing |
| `AgentMessage` | Streaming AI response content |
| `ToolCall` / `ToolResult` | Tool invocation lifecycle |
| `ApprovalRequired` | User approval needed |
| `TaskComplete` | Turn finished |
| `UndoCompleted` | Result of an undo operation (success/failure with message) |
| `UndoListResult` | Response to `UndoList` containing available `SnapshotInfo` entries |

**Approval Types** (`approvals.rs`): Defines `ExecApprovalRequestEvent` for shell commands and `ApplyPatchApprovalRequestEvent` for file edits. The `ReviewDecision` enum captures user responses.

**Conversation Types**: `ConversationId`, `ConversationStoredState`, `SessionSource` for session management.

**Custom Prompt Types** (`custom_prompts.rs`): Defines types for user-authored custom prompts invoked via `/prompts:<name>` slash commands:

| Type | Purpose |
|------|--------|
| `CustomPrompt` | A single custom prompt with name, path, content, description, argument hint, and kind |
| `CustomPromptKind` | Discriminates between `Markdown` (template text expanded inline) and `Script { interpreter }` (executable whose stdout becomes the prompt) |
| `PROMPTS_CMD_PREFIX` | The slash command prefix constant (`"prompts"`) |

`CustomPromptKind::Script` carries an `interpreter` string (e.g. `"bash"`, `"python3"`, `"node"`) that determines how the script file is executed. `CustomPromptKind` defaults to `Markdown` and is serde-tagged as `"type"` for JSON serialization.

### Things to Know

- Types are serde-serializable for persistence and wire transfer
- `ResponseItem` wraps different response content types (text, tool calls, reasoning)
- `TokenUsage` tracks input/output/cache token counts

**Undo Types:**

| Type | Purpose |
|------|---------|
| `SnapshotInfo` | Display metadata for a single undo snapshot: `index` (display order, 0 = most recent), `short_id` (7-char commit hash), `label` (user message) |
| `UndoListResultEvent` | Wraps `Vec<SnapshotInfo>` for the `UndoListResult` event |
| `UndoCompletedEvent` | Contains `success: bool` and optional `message` describing the result |

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
