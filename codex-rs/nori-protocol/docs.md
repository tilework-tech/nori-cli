# Noridoc: nori-protocol

Path: @/codex-rs/nori-protocol

### Overview

- Defines the normalized `ClientEvent` protocol that sits between raw ACP session updates (from `agent-client-protocol-schema`) and the TUI rendering layer. All ACP tool calls, messages, plans, approvals, and replay entries are transformed into this crate's types before reaching the TUI.
- The `ClientEventNormalizer` is the stateful entry point: it accepts `acp::SessionUpdate` and `acp::RequestPermissionRequest` values and emits `Vec<ClientEvent>`.
- Session-scoped ACP metadata is normalized into `ClientEvent::SessionUpdateInfo`, giving the rest of the stack one minimal rendering/replay path for mode, config, and session-info updates while still letting usage updates carry structured footer state.
- Single-file crate (`lib.rs`) with no submodules.

### How it fits into the larger codebase

```
agent_client_protocol_schema::SessionUpdate
    ──> ClientEventNormalizer ──> Vec<ClientEvent> ──> nori-tui
                                                                       (chatwidget, client_tool_cell, etc.)
```

- **Upstream dependency:** `agent-client-protocol-schema` provides the raw ACP schema types (`ToolCall`, `ToolCallUpdate`, `ContentChunk`, `Plan`, `RequestPermissionRequest`).
- **Downstream consumer:** `nori-tui` (`@/codex-rs/tui/`) is the primary consumer. The TUI renders `ToolSnapshot`, `MessageDelta`, `PlanSnapshot`, `ApprovalRequest`, reducer-owned `SessionPhaseChanged` / `PromptCompleted` / `QueueChanged` events, `ReplayEntry`, and `AgentCommandsUpdate` from this crate.
- `codex-acp` uses the same normalized events for both live updates and `session/load` replay, so this crate now has to preserve enough structure for replayable user-message chunks and pass-through session metadata notes.
- The `codex-acp` backend (`@/codex-rs/acp/`) now wraps the normalizer inside a serialized `SessionRuntime` driver. ACP prompt responses, `session/load`, `session/update`, cancellations, and permission requests are reduced in order before the backend forwards the resulting `ClientEvent` items to the TUI via `BackendEvent::Client`.
- This crate intentionally has no TUI, rendering, or terminal dependencies. It is a pure data transformation layer.

### Core Implementation

- **`ClientEventNormalizer`** maintains a `HashMap<String, acp::ToolCall>` keyed by `call_id`. `ToolCallUpdate` messages always upsert into that map: if the ACP agent never sent an initial `ToolCall`, the normalizer synthesizes a placeholder `ToolCall`, applies the update fields, and still emits a visible `ToolSnapshot`.
- **`SessionRuntime` support types** in `session_runtime.rs` define the reducer-owned ACP runtime model used by `codex-acp`: `SessionPhase`, `PersistedSessionState`, `ActiveRequestState`, `OpenMessage`, and `QueuedPrompt`. These types let the backend treat prompt turns, `session/load`, queued prompts, and ownership of tool/approval updates as one ordered state machine instead of reconstructing turn state from racing tasks.
- **Session update normalization** keeps the first pass intentionally small:
  - `UserMessageChunk` becomes `MessageDelta { stream: User, .. }`, which lets replay paths reconstruct visible user history during `session/load`.
  - `CurrentModeUpdate`, `ConfigOptionUpdate`, and `SessionInfoUpdate` become lightweight `SessionUpdateInfo` summaries.
  - `UsageUpdate` also becomes `SessionUpdateInfo`, but the usage variant additionally carries `SessionUsageState` so the TUI can update footer context without reparsing the display string.
- **Persisted session metadata** now includes `session_info` and `session_usage` alongside available commands, current mode, and config options. `codex-acp` owns persistence, but these structs live here so the reducer and replay pipeline share one runtime model.
- **`is_generic_tool_call()`** gates initial `ToolCall` emission: tool calls with no `raw_input`, no `locations`, empty `content`, and no `/` in the title are suppressed (return empty `Vec`). The normalizer still records them internally so that later attributed `ToolCallUpdate` messages can refine the existing call without forcing the TUI to render a placeholder cell first.
- **Invocation priority cascade** in `invocation_from_tool_call()` resolves what the tool is doing, in priority order:

  | Priority | Source | Result |
  |----------|--------|--------|
  | 1 | Diff artifacts in `content` | `Invocation::FileChanges` |
  | 2 | Structured parsing of `raw_input` by `ToolKind` | `Invocation::Command`, `Read`, `Search`, `ListFiles`, `FileOperations`, `Tool` |
  | 3 | `raw_input` present but unrecognized | `Invocation::RawJson` |
  | 4 | No `raw_input`, but `locations` non-empty | Location fallback: synthesizes `Read` or `Search` from the first location path. Edit/Delete/Move are excluded (they need more context than a bare path) and fall through to the TUI's location-path display fallback. |

- **`sanitize_title()`** strips Gemini-specific metadata from tool call titles before they reach the TUI. It removes `[current working directory /path]` suffixes and any trailing `(description text)` that Gemini appends after the cwd bracket. Applied in `tool_snapshot_from_tool_call()` so all downstream consumers (TUI rendering, transcript, approvals) receive cleaned titles.
- **`structured_invocation_from_tool_call()`** performs kind-specific parsing of `raw_input` JSON. For `Execute` kind, it unwraps shell-wrapper command arrays (`["/usr/bin/zsh", "-lc", "actual command"]`). For `Read` and `Search` kinds, it also checks `parsed_cmd` metadata (used by the Codex backend) to extract structured paths, queries, and listing classifications.
- **Artifact extraction** (`artifacts_from_tool_call()`) collects `Diff` and `Text` artifacts from `content`, then falls back to `raw_output` fields (`stdout`, `formatted_output`, `aggregated_output`, `lines`, `count`) when no text artifact was found.

### Things to Know

- The `is_generic_tool_call()` filter means the normalizer is not 1:1 with incoming events. Initial `ToolCall` messages that are sufficiently sparse are silently dropped, but later `ToolCallUpdate` messages still become visible `ToolSnapshot`s even if no initial `ToolCall` ever arrived.
- `SessionUpdateInfo` stays intentionally lightweight, but it is no longer fully lossy: the `Usage` variant also carries structured `SessionUsageState` so replay and live footer updates can share the same path.
- The location fallback (tier 4) only handles `Read` and `Search` kinds. Edit/Delete/Move with locations but no `raw_input` return `None` from the normalizer and fall through to the TUI's location-path display fallback, avoiding creation of empty-diff `FileOperations` that would route to `PatchHistoryCell`.
- `sanitize_title()` is a two-pass operation: first strips the `[current working directory ...]` bracket, then strips trailing `(description)` parenthetical. The parenthetical strip only fires after a cwd bracket was found, because Gemini appends descriptions after the cwd metadata.
- Shell wrapper detection (`is_shell_wrapper()`) recognizes `bash`, `sh`, `zsh`, `fish`, `pwsh`, and `powershell` with `-c` or `-lc` flags. When a 3-element command array matches this pattern, only the script portion is extracted as the command string.
- **Agent commands normalization**: `push_session_update()` converts ACP `AvailableCommandsUpdate` into `ClientEvent::AgentCommandsUpdate`. Each ACP `AvailableCommand` is mapped to an `AgentCommandInfo` struct carrying `name`, `description`, and `input_hint` (extracted from `AvailableCommandInput::Unstructured` when present). Each `AvailableCommandsUpdate` fully replaces the previous set of commands -- there is no incremental merge.

Created and maintained by Nori.
