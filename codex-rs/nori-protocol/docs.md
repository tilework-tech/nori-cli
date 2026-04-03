# Noridoc: nori-protocol

Path: @/codex-rs/nori-protocol

### Overview

- Defines the normalized `ClientEvent` protocol that sits between raw ACP session updates (from `sacp`) and the TUI rendering layer. All ACP tool calls, messages, plans, approvals, and replay entries are transformed into this crate's types before reaching the TUI.
- The `ClientEventNormalizer` is the stateful entry point: it accepts `acp::SessionUpdate` and `acp::RequestPermissionRequest` values and emits `Vec<ClientEvent>`.
- Single-file crate (`lib.rs`) with no submodules.

### How it fits into the larger codebase

```
sacp::SessionUpdate ──> ClientEventNormalizer ──> Vec<ClientEvent> ──> nori-tui
                                                                       (chatwidget, client_tool_cell, etc.)
```

- **Upstream dependency:** `sacp` crate provides the raw ACP schema types (`ToolCall`, `ToolCallUpdate`, `ContentChunk`, `Plan`, `RequestPermissionRequest`).
- **Downstream consumer:** `nori-tui` (`@/codex-rs/tui/`) is the primary consumer. The TUI renders `ToolSnapshot`, `MessageDelta`, `PlanSnapshot`, `ApprovalRequest`, `TurnLifecycle`, `ReplayEntry`, and `AgentCommandsUpdate` from this crate.
- The `codex-acp` backend (`@/codex-rs/acp/`) drives the normalizer, calling `push_session_update()` for each ACP event and forwarding the resulting `ClientEvent` items to the TUI via `BackendEvent::Client`.
- This crate intentionally has no TUI, rendering, or terminal dependencies. It is a pure data transformation layer.

### Core Implementation

- **`ClientEventNormalizer`** maintains a `HashMap<String, acp::ToolCall>` keyed by `call_id`. This allows `ToolCallUpdate` messages to be merged onto the original `ToolCall` state before emitting a fresh `ToolSnapshot`.
- **`is_generic_tool_call()`** gates initial `ToolCall` emission: tool calls with no `raw_input`, no `locations`, empty `content`, and no `/` in the title are suppressed (return empty `Vec`). These are placeholder tool calls that will be refined by subsequent `ToolCallUpdate` messages. This prevents the TUI from showing bare, detail-free cells before the agent populates the real data.
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

- The `is_generic_tool_call()` filter means the normalizer is not 1:1 with incoming events. Initial `ToolCall` messages that are sufficiently sparse are silently dropped. The TUI will only see tool cells once a `ToolCallUpdate` adds enough detail -- or once the `ToolCall` itself has locations or `raw_input`.
- The location fallback (tier 4) only handles `Read` and `Search` kinds. Edit/Delete/Move with locations but no `raw_input` return `None` from the normalizer and fall through to the TUI's location-path display fallback, avoiding creation of empty-diff `FileOperations` that would route to `PatchHistoryCell`.
- `sanitize_title()` is a two-pass operation: first strips the `[current working directory ...]` bracket, then strips trailing `(description)` parenthetical. The parenthetical strip only fires after a cwd bracket was found, because Gemini appends descriptions after the cwd metadata.
- Shell wrapper detection (`is_shell_wrapper()`) recognizes `bash`, `sh`, `zsh`, `fish`, `pwsh`, and `powershell` with `-c` or `-lc` flags. When a 3-element command array matches this pattern, only the script portion is extracted as the command string.
- **Agent commands normalization**: `push_session_update()` converts ACP `AvailableCommandsUpdate` into `ClientEvent::AgentCommandsUpdate`. Each ACP `AvailableCommand` is mapped to an `AgentCommandInfo` struct carrying `name`, `description`, and `input_hint` (extracted from `AvailableCommandInput::Unstructured` when present). Each `AvailableCommandsUpdate` fully replaces the previous set of commands -- there is no incremental merge.

Created and maintained by Nori.
