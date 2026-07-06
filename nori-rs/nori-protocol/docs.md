# Noridoc: nori-protocol

Path: @/nori-rs/nori-protocol

### Overview

- Defines the normalized `ClientEvent` protocol between raw ACP session updates and the TUI rendering layer. `ClientEventNormalizer` converts provider messages, plans, tools, approvals, and session metadata into stable client events.
- Exposes backend-owned session state, including thread goals, through normalized client events so `@/nori-rs/tui` does not know ACP backend storage details.
- `@/nori-rs/nori-protocol/src/lib.rs` defines the client event vocabulary and normalization path, while `@/nori-rs/nori-protocol/src/session_runtime.rs` defines reducer-owned ACP runtime state shared with `@/nori-rs/harness`.

### How it fits into the larger codebase

```
agent_client_protocol_schema::SessionUpdate
    ──> ClientEventNormalizer ──> Vec<ClientEvent> ──> nori-tui
                                                                       (chatwidget, client_tool_cell, etc.)
```

- **Upstream dependency:** `agent-client-protocol-schema` provides the raw ACP schema types (`ToolCall`, `ToolCallUpdate`, `ContentChunk`, `Plan`, `RequestPermissionRequest`).
- **Downstream consumer:** `nori-tui` (`@/nori-rs/tui/`) is the primary consumer. The TUI renders `ToolSnapshot`, `MessageDelta`, `PlanSnapshot`, `ApprovalRequest`, reducer-owned `SessionPhaseChanged` / `PromptCompleted` / `QueueChanged` events, `ReplayEntry`, and `AgentCommandsUpdate` from this crate.
- `nori-harness` uses the same normalized events for both live updates and `session/load` replay, so this crate now has to preserve enough structure for replayable user-message chunks and pass-through session metadata notes.
- The `nori-harness` backend (`@/nori-rs/harness/`) now wraps the normalizer inside a serialized `SessionRuntime` driver. ACP prompt responses, `session/load`, `session/update`, cancellations, and permission requests are reduced in order before the backend forwards the resulting `ClientEvent` items to the TUI via `BackendEvent::Client`.
- Thread-goal events are produced by `@/nori-rs/harness/src/backend/thread_goal.rs`, recorded by `@/nori-rs/harness/src/backend/transcript.rs`, and consumed by `@/nori-rs/tui/src/chatwidget/goal.rs`. This crate defines the shared client-facing shape so live sessions, replay, and resume all speak the same event vocabulary.
- This crate intentionally has no TUI, rendering, or terminal dependencies. It is a pure data transformation layer.

### Core Implementation

- **`ClientEventNormalizer`** maintains a `HashMap<String, acp::ToolCall>` keyed by `call_id`. `ToolCallUpdate` messages always upsert into that map: if the ACP agent never sent an initial `ToolCall`, the normalizer synthesizes a placeholder `ToolCall`, applies the update fields, and still emits a visible `ToolSnapshot`.
- **`SessionRuntime` support types** in `session_runtime.rs` define the reducer-owned ACP runtime model used by `nori-harness`: `SessionPhase`, `PersistedSessionState`, `ActiveRequestState`, `OpenMessage`, and `QueuedPrompt`. These types let the backend treat prompt turns, `session/load`, queued prompts, and ownership of tool/approval updates as one ordered state machine instead of reconstructing turn state from racing tasks. `QueuedPromptKind` distinguishes visible user prompts, compaction prompts, and hidden goal continuations so `@/nori-rs/harness/src/backend/session_reducer.rs` can preserve the right queue, transcript, and completion behavior for each path. `ActiveRequestState` keeps the last flushed assistant text so `PromptCompleted { last_agent_message, .. }` remains correct even when a later reasoning chunk closes the assistant buffer before the turn ends.
- **Session update normalization** keeps the first pass intentionally small:
  - `UserMessageChunk` becomes `MessageDelta { stream: User, .. }`, which lets replay paths reconstruct visible user history during `session/load`.
  - `CurrentModeUpdate` becomes `ClientEvent::SessionModeChanged { current_mode_id }`; the TUI resolves the id to a human label using its cached mode list.
  - `ConfigOptionUpdate` becomes `SessionConfigUpdate`, preserving the full option snapshot so the TUI can show only changed user-facing option values.
  - `SessionInfoUpdate` becomes a lightweight `SessionUpdateInfo` summary.
  - `UsageUpdate` also becomes `SessionUpdateInfo`, but the usage variant additionally carries `SessionUsageState` so the TUI can update footer context without reparsing the display string.
- **Persisted session metadata** now includes `session_info` and `session_usage` alongside available commands, current mode, and config options. `nori-harness` owns persistence, but these structs live here so the reducer and replay pipeline share one runtime model.
- **Session capability projection**: `SessionCapabilitiesView` (with its nested `AgentCapabilitiesView`) is the client-facing snapshot behind `ClientEvent::SessionCapabilitiesChanged`. `AgentCapabilitiesView` carries the raw agent ACP capability projection -- `http_mcp`, `load_session`, and the session-lifecycle capabilities `session_list`, `session_resume`, and `session_close` -- built by `@/nori-rs/harness/src/backend/nori_client_mcp.rs` and consumed by `@/nori-rs/tui` to gate the agent-sourced `/resume` picker and the `/close` command. The three `session_*` fields are serde-defaulted so snapshots from older writers deserialize as `false` rather than failing.
- **Thread-goal client events** carry the current goal snapshot (`objective`, lifecycle status, token usage, active time, and timestamps) or a clear notification. They are not derived from ACP provider messages; they are backend session-state projections emitted through the same `ClientEvent` stream as normalized ACP data.
- **`is_generic_tool_call()`** gates initial `ToolCall` emission: tool calls with no `raw_input`, no `locations`, empty `content`, and no `/` in the title are suppressed (return empty `Vec`). The normalizer still records them internally so that later attributed `ToolCallUpdate` messages can refine the existing call without forcing the TUI to render a placeholder cell first.
- **Invocation priority cascade** in `invocation_from_tool_call()` resolves what the tool is doing, in priority order:

  | Priority | Source | Result |
  |----------|--------|--------|
  | 1 | Diff artifacts in `content` | `Invocation::FileChanges` |
  | 2 | Structured parsing of `raw_input` by `ToolKind` | `Invocation::Command`, `Read`, `Search`, `ListFiles`, `FileOperations`, `Tool` |
  | 3 | `raw_input` present but unrecognized | `Invocation::RawJson` |
  | 4 | No `raw_input`, but `locations` non-empty | Location fallback: synthesizes `Read` or `Search` from the first location path. Edit/Delete/Move are excluded (they need more context than a bare path) and fall through to the TUI's location-path display fallback. |

- **`sanitize_title()`** strips Gemini-specific metadata from tool call titles before they reach the TUI. It removes `[current working directory /path]` suffixes and any trailing `(description text)` that Gemini appends after the cwd bracket. Applied in `tool_snapshot_from_tool_call()` so all downstream consumers (TUI rendering, transcript, approvals) receive cleaned titles.
- **`structured_invocation_from_tool_call()`** performs kind-specific parsing of `raw_input` JSON. For `Execute` kind, it unwraps shell-wrapper command arrays (`["/usr/bin/zsh", "-lc", "actual command"]`). For `Read` and `Search` kinds, it also checks `parsed_cmd` metadata in `raw_input` to extract structured paths, queries, and listing classifications.
- **Artifact extraction** (`artifacts_from_tool_call()`) collects `Diff` and `Text` artifacts from `content`, then falls back to `raw_output` fields (`stdout`, `formatted_output`, `aggregated_output`, `lines`, `count`) when no text artifact was found.

### Things to Know

- The `is_generic_tool_call()` filter means the normalizer is not 1:1 with incoming events. Initial `ToolCall` messages that are sufficiently sparse are silently dropped, but later `ToolCallUpdate` messages still become visible `ToolSnapshot`s even if no initial `ToolCall` ever arrived.
- `SessionUpdateInfo` stays intentionally lightweight, but it is no longer fully lossy: the `Usage` variant also carries structured `SessionUsageState` so replay and live footer updates can share the same path.
- `ThreadGoalUpdated` is a full replacement snapshot for the client's current goal, while `ThreadGoalCleared` removes that state. The TUI should not infer a goal lifecycle by replaying command text; it should consume these events directly.
- Usage events and goal events intentionally remain separate: ACP `UsageUpdate` normalizes to `SessionUpdateInfo`, and the backend may follow it with a refreshed `ThreadGoalUpdated` when a goal exists. Goal `tokens_used` is accumulated by `@/nori-rs/harness/src/backend/thread_goal.rs` from positive ACP session-usage deltas, with context-window drops treated as new checkpoints rather than subtracting previously counted work.
- Hidden goal continuations are protocol-visible as `QueuedPromptKind::GoalContinuation`, but they are not user-visible prompt text. Reducer consumers should treat their assistant output like any other assistant turn while excluding their prompt text from visible `QueueChanged` entries and user transcript messages.
- The location fallback (tier 4) only handles `Read` and `Search` kinds. Edit/Delete/Move with locations but no `raw_input` return `None` from the normalizer and fall through to the TUI's location-path display fallback, avoiding creation of empty-diff `FileOperations` that would route to `PatchHistoryCell`.
- `sanitize_title()` is a two-pass operation: first strips the `[current working directory ...]` bracket, then strips trailing `(description)` parenthetical. The parenthetical strip only fires after a cwd bracket was found, because Gemini appends descriptions after the cwd metadata.
- Shell wrapper detection (`is_shell_wrapper()`) recognizes `bash`, `sh`, `zsh`, `fish`, `pwsh`, and `powershell` with `-c` or `-lc` flags. When a 3-element command array matches this pattern, only the script portion is extracted as the command string.
- **Agent commands normalization**: `push_session_update()` converts ACP `AvailableCommandsUpdate` into `ClientEvent::AgentCommandsUpdate`. Each ACP `AvailableCommand` is mapped to an `AgentCommandInfo` struct carrying `name`, `description`, and `input_hint` (extracted from `AvailableCommandInput::Unstructured` when present). Each `AvailableCommandsUpdate` fully replaces the previous set of commands -- there is no incremental merge.

Created and maintained by Nori.
