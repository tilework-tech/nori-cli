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

**Operations** (`protocol/mod.rs`): Commands sent from TUI to core:

| Op | Purpose |
|----|---------|
| `Configure` | Set session configuration |
| `UserTurn` | Send user message |
| `ApproveTool` / `RejectTool` | Handle approval requests |
| `CancelTurn` | Cancel current generation |
| `Undo` | Undo the most recent turn (sequential pop from snapshot stack) |
| `UndoList` | Request the list of available undo snapshots |
| `UndoTo { index }` | Restore to a specific snapshot by display index (0 = most recent) |
| `SearchHistoryRequest { max_results }` | Request all history entries for client-side search; response via `SearchHistoryResponse` |

**Events** (`events.rs`): Messages from core to TUI:

| Event | Purpose |
|-------|---------|
| `TaskStarted` | Turn began processing |
| `AgentMessage` | Streaming AI response content |
| `ToolCall` / `ToolResult` | Tool invocation lifecycle |
| `ApprovalRequired` | User approval needed |
| `TaskComplete` | Turn finished |
| `ContextCompacted` | Conversation history was compacted; carries optional summary text for TUI session boundary rendering |
| `UndoCompleted` | Result of an undo operation (success/failure with message) |
| `UndoListResult` | Response to `UndoList` containing available `SnapshotInfo` entries |
| `PromptSummary` | Short summary of the first user prompt for display in the footer |
| `HookOutput` | Output from a hook script, routed by level (Info/Warn/Error) for TUI display |
| `SearchHistoryResponse` | Response to `SearchHistoryRequest` with deduplicated history entries (newest first). Not persisted to rollout files. |

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

**Module Structure:** The `protocol` module uses a directory layout (`protocol/mod.rs` + submodules) instead of a single `protocol.rs` file. Submodules include `display.rs` (Display impls), `history.rs` (conversation history types), `legacy_events.rs` (legacy event types), `sandbox.rs` (sandbox config types), `token_usage.rs` (token tracking types), and `tests.rs`.

- Types are serde-serializable for persistence and wire transfer
- `ResponseItem` wraps different response content types (text, tool calls, reasoning)
- `TokenUsage` tracks input/output/cache token counts

**Undo Types:**

| Type | Purpose |
|------|---------|
| `SnapshotInfo` | Display metadata for a single undo snapshot: `index` (display order, 0 = most recent), `short_id` (7-char commit hash), `label` (user message) |
| `UndoListResultEvent` | Wraps `Vec<SnapshotInfo>` for the `UndoListResult` event |
| `UndoCompletedEvent` | Contains `success: bool` and optional `message` describing the result |

**Prompt Summary Types:**

| Type | Purpose |
|------|---------|
| `PromptSummaryEvent` | Carries a `summary: String` field with a short summary of the first user prompt. Emitted by the ACP backend and rendered in the TUI footer. Not persisted to rollout files. |

**Hook Output Types:**

| Type | Purpose |
|------|---------|
| `HookOutputLevel` | Enum with `Info`, `Warn`, `Error` variants controlling TUI display style |
| `HookOutputEvent` | Carries a `message: String` and `level: HookOutputLevel`. Emitted by the ACP backend's hook routing. Not persisted to rollout files. |

**Search History Types:**

| Type | Purpose |
|------|--------|
| `SearchHistoryResponseEvent` | Wraps `Vec<HistoryEntry>` (from `codex_protocol::message_history`). Each entry has `conversation_id`, `ts`, and `text`. Not persisted to rollout files. |

**Context Compaction Types:**

| Type | Purpose |
|------|---------|
| `ContextCompactedEvent` | Carries an optional `summary: Option<String>` field. When emitted by the ACP backend (`@/codex-rs/acp/`), the summary contains the compact summary text so the TUI can render a session boundary and reprint it. When emitted by the core backend (`@/codex-rs/core/`), the summary is `None` and the TUI shows only an info message. |

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
