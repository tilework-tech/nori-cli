# ACP TUI Rendering Fixes — Application Specification

## Overview

This document consolidates six specs (02, 04–08) that fix the TUI rendering of ACP tool call events. The current `ClientToolCell` path produces verbose, flat, unpolished output compared to the legacy `ExecCell` exploring mode. These specs collectively bring the ACP rendering to visual parity with the old path and handle edge cases from different ACP agents (Claude, Gemini, Codex).

**Goal**: Make ACP tool calls render as compactly, cleanly, and informatively as the legacy ExecCell path — without relying on the synthetic `ExecCommandBeginEvent`/`ExecCommandEndEvent` translation layer.

## Architecture Context

### Event Flow

```
ACP Agent subprocess
  └─ sacp::schema::SessionUpdate
      │
      ▼
codex-acp backend  (normalize_session_update)
  └─ nori_protocol::ClientEvent::ToolSnapshot(ToolSnapshot)
      │
      ▼
nori-tui ChatWidget  (handle_client_event → handle_client_tool_snapshot)
  ├─ Edit/Delete/Move + Completed + file_changes → PatchHistoryCell
  ├─ Execute                                     → ClientToolCell (native rendering)
  ├─ Read/Search/Fetch/Think/Other               → synthetic ExecCell (legacy translation)
  └─ Non-completed Edit/Delete/Move              → _ => {} (DROPPED — spec 05 fixes this)
```

### Key Types

| Type | Crate | Role |
|------|-------|------|
| `ToolSnapshot` | `nori-protocol` | Normalized tool call state: call_id, title, kind, phase, locations, invocation, artifacts, raw_input/output |
| `ToolKind` | `nori-protocol` | Read, Search, Execute, Edit, Delete, Move, Fetch, Think, Other(String) |
| `ToolPhase` | `nori-protocol` | Pending, PendingApproval, InProgress, Completed, Failed |
| `Invocation` | `nori-protocol` | Structured input: Command, Read, Search, ListFiles, FileChanges, FileOperations, Tool, RawJson |
| `Artifact` | `nori-protocol` | Output data: Text { text }, Diff(FileChange) |
| `ClientToolCell` | `nori-tui` | Single-snapshot cell for ACP tool rendering |
| `ExecCell` | `nori-tui` | Legacy multi-call cell with exploring grouping |
| `PatchHistoryCell` | `nori-tui` | Completed edit diff rendering |

### Key Files

| File | Purpose |
|------|---------|
| `tui/src/chatwidget/event_handlers.rs` | ToolSnapshot routing dispatch |
| `tui/src/client_tool_cell.rs` | ClientToolCell struct, rendering, lifecycle |
| `tui/src/client_event_format.rs` | Format helpers: tool headers, invocations, artifacts, exploring classification |
| `tui/src/diff_render.rs` | Diff summary rendering, `display_path_for()` path normalization |
| `tui/src/exec_cell/render.rs` | Legacy ExecCell rendering including `exploring_display_lines()` |
| `tui/src/exec_command.rs` | `relativize_to_home()` utility |
| `nori-protocol/src/lib.rs` | ClientEventNormalizer, ToolSnapshot construction, artifact/invocation extraction |

---

## Spec 02: Exploring Cell Grouping

### Problem

Each Read/Search/ListFiles tool call renders as a separate `ClientToolCell` with generic `"Tool [phase]: title (kind)"` formatting. The old ExecCell path grouped consecutive exploring calls into a single compact cell:

```
• Explored
  └ Read README.md
    Search List /home/clifford/...
```

Currently, each read is its own cell with verbose output:

```
• Tool [completed]: Read /home/.../README.md (1 - 5) (read)
  └ Read: /home/.../README.md
    Output:
    ```
         1→# Nori CLI
    ```
```

### Required Changes

1. **Extend `ClientToolCell`** to hold a `Vec<ToolSnapshot>` (or parallel sub-items list) for exploring mode, similar to `ExecCell`'s `Vec<ExecCall>`.

2. **Merge exploring snapshots** in `handle_client_native_tool_snapshot`: when the active cell is an exploring `ClientToolCell` and the new snapshot is also exploring, merge the snapshot into the existing cell instead of creating a new one.

3. **Port sub-item rendering** from `ExecCell::exploring_display_lines()`:
   - Group consecutive reads by filename: `Read file1.rs, file2.rs`
   - Show `Read`, `Search`, `List` labels with compact arguments
   - Header: `Exploring` (bold + spinner) while active, `Explored` (bold + dim bullet) when completed
   - Tree prefix via `prefix_lines`: `└` for first sub-item, spaces for subsequent

4. **Omit read output content** from exploring cells — reads are informational and content is noise in history.

### Affected Code

- `tui/src/client_tool_cell.rs` — data model and rendering
- `tui/src/chatwidget/event_handlers.rs:handle_client_native_tool_snapshot` — merge logic
- `tui/src/client_event_format.rs:is_exploring_snapshot` — already exists, used for flush decisions
- `tui/src/exec_cell/render.rs:exploring_display_lines` — reference implementation to port

---

## Spec 04: Path Display Normalization

### Problem

Absolute paths appear verbatim throughout tool cell rendering, consuming excessive terminal width:

```
• Tool [completed]: Read /home/clifford/Documents/source/nori/cli/.worktrees/acp-event-model-refactor/README.md (1 - 5) (read)
```

Expected:

```
• Read README.md
```

### Required Changes

1. **Thread `cwd: &Path`** into `ClientToolCell` (at construction or via config reference).

2. **Apply path relativization** in:
   - `format_tool_header`: strip cwd prefix from `snapshot.title` (regex-replace absolute paths within the title string)
   - `format_invocation`: relativize `path` in Read, Search, ListFiles, FileChanges, FileOperations
   - Sub-item rendering for exploring cells (spec 02)

3. **Reuse existing utilities**:
   - `diff_render.rs:display_path_for(path, cwd)` — cwd-relative with git-repo awareness
   - `exec_command.rs:relativize_to_home` — home-relative fallback

4. **Path display rules**:
   - Inside cwd → relative (`README.md`)
   - Outside cwd but inside home → `~/...`
   - Outside home → absolute

### Affected Code

- `tui/src/client_event_format.rs:format_tool_header` — title path stripping
- `tui/src/client_event_format.rs:format_invocation` — invocation path normalization
- `tui/src/client_tool_cell.rs` — threading cwd into the cell
- `tui/src/diff_render.rs:display_path_for` — existing utility to reuse

---

## Spec 05: In-Progress Edit/Delete/Move Rendering

### Problem

Non-completed Edit/Delete/Move snapshots hit the `_ => {}` fallback arm and are silently dropped. Users see no visual feedback while an edit is being applied or awaiting approval.

```rust
// Current routing in handle_client_tool_snapshot_now:
Edit/Delete/Move + Completed + file_changes → handle_client_edit_tool_snapshot
Execute/Read/Search/...                      → handle_client_native_tool_snapshot
_ => {}  // ← Non-completed Edit/Delete/Move land here
```

### Expected Behavior

```
⠋ Editing README.md        ← in-progress with spinner
```

Transitions to `PatchHistoryCell` diff view on completion:

```
• Edited README.md (+1 -1)
    1 -# Nori CLI
    1 +# Nori CLI (TEST EDIT)
```

### Required Changes

1. **Route non-completed Edit/Delete/Move** to `handle_client_native_tool_snapshot` so they render as a `ClientToolCell` with a spinner.

2. **On completed Edit/Delete/Move arrival**: replace the in-progress `ClientToolCell` in the active cell slot with the `PatchHistoryCell` (flush the spinner cell, add the diff cell).

3. **Failed edits**: show a red bullet with the error from artifacts.

4. **Verb-appropriate labels** extracted from `snapshot.locations[0].path` or `snapshot.title`:
   - Edit → `Editing <path>`
   - Delete → `Deleting <path>`
   - Move → `Moving <from> → <to>`

### Affected Code

- `tui/src/chatwidget/event_handlers.rs:handle_client_tool_snapshot_now` — routing logic
- `tui/src/client_tool_cell.rs` — in-progress edit rendering

---

## Spec 06: Artifact Text Output Cleanup

### Problem

1. **Code fence markers** (```` ```console ```` / ```` ``` ````) appear literally — Claude wraps output in markdown code blocks in the `content` field.
2. **`Output:` prefix** always shown, even for short single-line results.
3. **No `(no output)` label** when stdout is empty.
4. **Redundant `Command:` line** repeats the command already in the header.

Current:
```
• Tool [completed]: date --utc +"%Y-%m-%d %H:%M:%S %Z" (execute)
  └ Command: date --utc +"%Y-%m-%d %H:%M:%S %Z"
    Output:
    ```console
    2026-03-30 05:45:34 UTC
    ```
```

Expected:
```
• Ran date --utc +"%Y-%m-%d %H:%M:%S"
  └ 2026-03-30 05:47:15
```

### Required Changes

1. **Strip code fences**: Detect and remove leading ```` ```lang ```` and trailing ```` ``` ```` from text artifacts. Heuristic: first line matches `^```\w*$`, last line matches `^```$` → strip both.

2. **Prefer `rawOutput` for execute tools**: In `artifacts_from_tool_call`, when the tool kind is Execute and `rawOutput` has clean text, prefer it over fenced `content` text. Alternatively, always strip fences in post-processing.

3. **Handle empty output**: When an execute tool completes with no text artifacts or empty text, render `(no output)` in dim text.

4. **Remove redundant Command line**: When the `Invocation::Command` text matches the title text, omit the invocation detail line.

5. **Simplify single-line output**: Render directly on the first detail line (after `└`) without `Output:` prefix. Use `Output:` only for multi-line blocks.

### Note

The `render_execute_lines()` method in `ClientToolCell` already handles code fence stripping via `strip_code_fences()`, empty output via `(no output)`, and prefers `raw_output.stdout`. This spec's changes primarily affect the **generic rendering path** (`render_generic_lines()` / `format_artifacts()`), which non-Execute tools still use. However, the title sanitization and redundant-command-line removal apply to Execute rendering too.

### Affected Code

- `nori-protocol/src/lib.rs:artifacts_from_tool_call` — artifact priority/stripping
- `tui/src/client_event_format.rs:format_artifacts` — output formatting
- `tui/src/client_tool_cell.rs:render_generic_lines` — detail line rendering

---

## Spec 07: Diff Artifact Rendering in ClientToolCell

### Problem

`Artifact::Diff` entries are filtered out (`None`) in `format_artifacts` and never rendered. When a `ClientToolCell` carries diff artifacts (from ACP `content` array) but doesn't go through `PatchHistoryCell`, the diffs are silently lost. This primarily affects in-progress edits (spec 05 dependency) and edge cases where `snapshot_file_changes()` returns `None`.

### Expected Behavior

In-progress edit shows a diff preview:

```
⠋ Editing README.md
    1 -# Nori CLI
    1 +# Nori CLI (TEST EDIT)
```

### Required Changes

1. **Render `Artifact::Diff` entries** in `ClientToolCell` — convert to `FileChange` using existing `snapshot_file_changes` helper, then render via `create_diff_summary` from `diff_render.rs`.

2. **Scope**: Most useful for in-progress edits (spec 05) where the diff preview shows what's about to change while waiting for approval. For completed edits that already render through `PatchHistoryCell`, the diff artifacts remain redundant.

3. **Edge case**: For non-edit `ClientToolCell` instances that carry diff artifacts, render them as a secondary section after invocation/output lines.

### Affected Code

- `tui/src/client_event_format.rs:format_artifacts` — currently filters `Diff` to `None`
- `tui/src/client_tool_cell.rs` — diff rendering in detail lines
- `tui/src/diff_render.rs:create_diff_summary` — existing utility to reuse
- `tui/src/client_event_format.rs:snapshot_file_changes` — existing Diff→FileChange converter

---

## Spec 08: Gemini Empty Content Fallback

### Problem

Gemini sends completed tool calls with `content: []` (empty array) and no `rawInput`/`rawOutput`, producing `ClientToolCell`s with no detail lines — just a bare header. Additionally, Gemini embeds metadata in the title string (`[current working directory ...]`, `(description text)`).

Current:
```
• Tool [completed]: README.md (read)
```
```
• Tool [completed]: echo "..." > tmp.md [current working directory /home/...] (Create a temp file...) (execute)
```

Expected:
```
• Explored
  └ Read README.md
```
```
• Ran echo "..." > tmp.md
```

### Required Changes

1. **Fallback invocation from locations**: In `invocation_from_tool_call`, when `raw_input` is `None` but `locations` is non-empty, synthesize an invocation based on tool kind:
   - `Read` + location → `Invocation::Read { path }`
   - `Edit` + location → `Invocation::FileOperations` with path
   - `Search` + location → `Invocation::Search { path, query: None }`

2. **Title sanitization for Gemini**: Strip `[current working directory ...]` suffix and trailing `(description text)` from titles. Pattern: strip everything after the first `[` or after a trailing `(...)` when kind is Execute.

3. **Minimal completed cell**: When a completed tool cell would render with zero detail lines, show at least the locations as sub-items, or just the title without the redundant `(kind)` suffix.

### Affected Code

- `nori-protocol/src/lib.rs:invocation_from_tool_call` — location fallback
- `nori-protocol/src/lib.rs:push_session_update` — title sanitization
- `tui/src/client_tool_cell.rs:render_generic_lines` — minimal completed cell

---

## Dependency Graph

```
Spec 02 (Exploring Grouping)  ←─────────── foundational, changes ClientToolCell data model
    ↑
Spec 04 (Path Normalization)  ←─────────── threads cwd into cells, used by 02's sub-item rendering

Spec 05 (In-Progress Edits)   ←─────────── changes routing in handle_client_tool_snapshot
    ↑
Spec 07 (Diff Artifact Render) ─────────── requires in-progress edit cells from 05

Spec 06 (Artifact Cleanup)    ←─────────── independent, modifies format_artifacts + normalizer
Spec 08 (Gemini Fallback)     ←─────────── independent, modifies normalizer + minimal cell rendering
```

### Recommended Implementation Order

1. **Spec 08** — Gemini fallback (normalizer-only, minimal risk)
2. **Spec 06** — Artifact text cleanup (normalizer + format, low coupling)
3. **Spec 04** — Path normalization (threads cwd, needed by later specs)
4. **Spec 05** — In-progress edit rendering (routing change)
5. **Spec 07** — Diff artifact rendering (builds on 05)
6. **Spec 02** — Exploring cell grouping (largest change, depends on 04)

### Risk Notes

- **Spec 02** is the highest-risk change: it modifies `ClientToolCell`'s data model from single-snapshot to multi-snapshot, affecting construction, update, rendering, and the active-cell lifecycle. It should be tackled last with thorough snapshot tests.
- **Spec 04** touches rendering paths across multiple specs. Implementing it early ensures all subsequent rendering changes use normalized paths from the start.
- **Spec 05** changes the routing `match` in `handle_client_tool_snapshot_now`, which is a central dispatch point. The cell-replacement logic (spinner → PatchHistoryCell) needs careful handling of the active-cell lifecycle.

---

## Testing Strategy

All specs should have:

1. **Unit tests in `client_tool_cell.rs`** — snapshot-based rendering tests for each new rendering path (exploring, in-progress edit, stripped artifacts, normalized paths, empty content fallback).

2. **Unit tests in `nori-protocol/src/lib.rs`** — normalizer tests for artifact priority changes, location-based invocation fallback, title sanitization.

3. **Integration tests in `chatwidget/tests/`** — event routing tests verifying that snapshots arrive at the correct handler and produce the expected cell type.

4. **Snapshot tests** (`cargo insta`) — visual regression tests for rendered output of each cell variant.
