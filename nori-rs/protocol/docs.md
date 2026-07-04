# Noridoc: codex-protocol

Path: @/nori-rs/protocol

### Overview

- Defines the internal message types used between Nori components. It specifies operations (`Op`), events (`EventMsg`), and approval-related types that flow between the TUI, core, and backend layers.
- Owns shared command contracts that must stay backend-agnostic, such as typed thread-goal operations and validation helpers used by both `@/nori-rs/tui` and `@/nori-rs/harness`.

### How it fits into the larger codebase

- `@/nori-rs/tui` consumes shared protocol types when turning user actions into backend operations.
- `@/nori-rs/harness` implements ACP-specific behavior behind the same `Op` surface, including thread-goal handling in `@/nori-rs/harness/src/backend/thread_goal.rs`.
- `@/nori-rs/core` provides shared infrastructure (config, auth) to the frontends and consumes this crate's types, including the MCP server config and shell environment policy types in `config_types`.
- `@/nori-rs/sandbox` (the exec engine) consumes `SandboxPolicy` and the shell environment policy types from this crate; that direction keeps `codex-sandbox` free of config dependencies.
- `@/nori-rs/nori-protocol` carries normalized ACP client events back toward the TUI; thread-goal commands start here as `Op` values and return there as normalized goal events.
- All consumers import this crate directly. `codex-core` used to re-export the `protocol` and `config_types` modules; those detour re-exports were removed in the crate-layering cleanup (`@/docs/specs/crate-layering.md`), and this crate is the sole home of the shared type vocabulary.
- The crate is a pure type definition library with serde and schema support; ownership of runtime state belongs to backend crates, not this crate.

### Core Implementation

**Core Types:** `@/nori-rs/protocol/src/protocol/mod.rs` defines `Submission`, `Op`, `Event`, and `EventMsg`, which form the shared SQ/EQ contract between the UI and whichever backend owns the active session.

**Operations** (`@/nori-rs/protocol/src/protocol/mod.rs`) group backend commands into user-input, lifecycle, approval, history, undo, custom-prompt, and session-control surfaces. The `/goal` feature belongs to that typed command surface through `ThreadGoalGet`, `ThreadGoalSet`, and `ThreadGoalClear`, rather than being smuggled through a normal user prompt.

**Events** (`@/nori-rs/protocol/src/protocol/mod.rs`) carry shared control-plane updates back to TUI-facing code. Examples include turn lifecycle events, approval prompts, compact-summary notifications, undo results, prompt summaries, hook output, and history lookup results. ACP session-domain rendering uses `@/nori-rs/nori-protocol` instead.

**Approval Types** (`approvals.rs`): Defines `ExecApprovalRequestEvent` for shell commands and `ApplyPatchApprovalRequestEvent` for file edits. The `ReviewDecision` enum captures user responses.

**Conversation Types**: `ConversationId`, `ConversationStoredState`, `SessionSource` for session management.

**Custom Prompt Types** (`custom_prompts.rs`): Defines types for user-authored custom prompts invoked via `/prompts:<name>` slash commands:

| Type | Purpose |
|------|--------|
| `CustomPrompt` | A single custom prompt with name, path, content, description, argument hint, and kind |
| `CustomPromptKind` | Discriminates between `Markdown` (template text expanded inline) and `Script { interpreter }` (executable whose stdout becomes the prompt) |
| `PROMPTS_CMD_PREFIX` | The slash command prefix constant (`"prompts"`) |

`CustomPromptKind::Script` carries an `interpreter` string (e.g. `"bash"`, `"python3"`, `"node"`) that determines how the script file is executed. `CustomPromptKind` defaults to `Markdown` and is serde-tagged as `"type"` for JSON serialization.

**Thread Goal Types** (`protocol/mod.rs`): The `/goal` feature uses typed operations rather than encoding commands as regular prompt text. `Op::ThreadGoalGet`, `Op::ThreadGoalSet`, and `Op::ThreadGoalClear` define the backend-facing command surface; `ThreadGoalStatus` defines the shared lifecycle labels; `validate_thread_goal_objective()` defines the cross-crate validation invariant for objective text before the TUI or backend accepts it.

**Compact Number Formatting** (`num_format.rs`): Shared user-facing formatters keep ACP backend prompt context and TUI summaries consistent. Token counts use SI suffixes, and whole-second goal elapsed time is rendered compactly as seconds or minute/second text.

**Config Types** (`config_types.rs`): Backend-agnostic configuration enums (`SandboxMode`, `ReasoningEffort`, `TrustLevel`, and friends) plus the MCP server configuration types `McpServerConfig` and `McpServerTransportConfig` (`Stdio` and `StreamableHttp` transports). The MCP types live here rather than in `codex-core` so that `@/nori-rs/acp-host/src/connection/mcp.rs` can convert configured servers to ACP schema values without a core dependency; core re-exports them via `@/nori-rs/core/src/config/types.rs` for its own config code, and `@/nori-rs/rmcp-client` OAuth flows in the TUI consume the same types. `McpServerConfig` has a custom `Deserialize` that validates transport-specific fields (e.g., OAuth client-credential fields are rejected for stdio transport).

The module also hosts the shell environment policy types (`ShellEnvironmentPolicy`, `ShellEnvironmentPolicyToml`, `ShellEnvironmentPolicyInherit`, `EnvironmentVariablePattern`), which describe how a child process environment is built (inherit mode, default excludes, exclude/include-only wildcard patterns, explicit sets). They moved here from core's config types so the `codex-sandbox` exec engine (`@/nori-rs/sandbox/src/exec_env.rs`) can consume them without a config dependency; core re-exports them the same way as the MCP types.

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
| `ContextCompactedEvent` | Carries an optional `summary: Option<String>` field. When emitted by the ACP backend (`@/nori-rs/harness/`), the summary contains the compact summary text so the TUI can render a session boundary and reprint it. When emitted by the core backend (`@/nori-rs/core/`), the summary is `None` and the TUI shows only an info message. |

**Thread Goal Invariants:**

- Goal objectives are validated in `@/nori-rs/protocol/src/protocol/mod.rs` so the same empty and maximum-length rules apply before `@/nori-rs/tui/src/chatwidget/goal.rs` submits a goal and before `@/nori-rs/harness/src/backend/thread_goal.rs` persists one.
- `ThreadGoalSet` accepts either a new objective, a status update for an existing goal, or both. The backend owns how that becomes session state and emits normalized `ThreadGoalUpdated` / `ThreadGoalCleared` events through `@/nori-rs/nori-protocol`.
- These operations are ACP-backend commands, not agent prompt text. The ACP backend may use the stored goal to transform later prompts, but the protocol operation itself never goes to the agent subprocess.

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
