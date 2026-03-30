# ACP TUI Rendering Fixes — Progress Tracker

> See [APPLICATION-SPEC.md](./APPLICATION-SPEC.md) for full specification details.

## Implementation Order

| # | Spec | Status | Branch/PR |
|---|------|--------|-----------|
| 1 | Spec 08: Gemini Empty Content Fallback | Not Started | — |
| 2 | Spec 06: Artifact Text Output Cleanup | Not Started | — |
| 3 | Spec 04: Path Display Normalization | Not Started | — |
| 4 | Spec 05: In-Progress Edit/Delete/Move Rendering | Not Started | — |
| 5 | Spec 07: Diff Artifact Rendering in ClientToolCell | Not Started | — |
| 6 | Spec 02: Exploring Cell Grouping | Not Started | — |

## Detailed Task Breakdown

### 1. Spec 08: Gemini Empty Content Fallback

- [ ] **Normalizer: location-based invocation fallback**
  - File: `nori-protocol/src/lib.rs:invocation_from_tool_call`
  - When `raw_input` is None and `locations` is non-empty, synthesize invocation from kind + location
  - Read + location → `Invocation::Read { path }`
  - Edit + location → `Invocation::FileOperations`
  - Search + location → `Invocation::Search { path, query: None }`
- [ ] **Normalizer: Gemini title sanitization**
  - File: `nori-protocol/src/lib.rs` (title processing in `tool_snapshot_from_tool_call` or a new helper)
  - Strip `[current working directory ...]` suffix
  - Strip trailing `(description text)` when kind is Execute
- [ ] **TUI: minimal completed cell rendering**
  - File: `tui/src/client_tool_cell.rs:render_generic_lines`
  - When zero detail lines: show locations as sub-items, or title without `(kind)` suffix
- [ ] **Tests**
  - Normalizer tests for location fallback invocations
  - Normalizer tests for title sanitization
  - ClientToolCell rendering tests for minimal completed cells

---

### 2. Spec 06: Artifact Text Output Cleanup

- [ ] **Normalizer: prefer rawOutput for execute tools**
  - File: `nori-protocol/src/lib.rs:artifacts_from_tool_call`
  - When kind is Execute and `rawOutput` has clean text, prefer it over fenced `content` text
- [ ] **TUI: strip code fences in generic rendering path**
  - File: `tui/src/client_event_format.rs:format_artifacts`
  - Detect/strip ```` ```lang ```` / ```` ``` ```` from text artifacts
  - (Note: `render_execute_lines` already has `strip_code_fences()` — ensure consistency)
- [ ] **TUI: handle empty output**
  - File: `tui/src/client_event_format.rs:format_artifacts` or `client_tool_cell.rs:render_generic_lines`
  - When execute tool completes with no/empty text artifacts, show `(no output)` dim
- [ ] **TUI: remove redundant Command line**
  - File: `tui/src/client_tool_cell.rs:render_generic_lines`
  - When `Invocation::Command` text matches title, omit the invocation detail line
- [ ] **TUI: simplify single-line output**
  - Show directly on first detail line without `Output:` prefix; use `Output:` only for multi-line
- [ ] **Tests**
  - Normalizer tests for rawOutput preference
  - Rendering tests for stripped fences, empty output, redundant command removal

---

### 3. Spec 04: Path Display Normalization

- [ ] **Thread `cwd` into ClientToolCell**
  - File: `tui/src/client_tool_cell.rs` — add `cwd: PathBuf` field or config ref
  - File: `tui/src/chatwidget/event_handlers.rs` — pass cwd at construction
- [ ] **Normalize paths in format_tool_header**
  - File: `tui/src/client_event_format.rs:format_tool_header`
  - Strip/replace absolute paths within `snapshot.title` using cwd
- [ ] **Normalize paths in format_invocation**
  - File: `tui/src/client_event_format.rs:format_invocation`
  - Relativize paths in Read, Search, ListFiles, FileChanges, FileOperations
- [ ] **Reuse existing utilities**
  - Consolidate on `display_path_for()` from `diff_render.rs` or `relativize_to_home` from `exec_command.rs`
- [ ] **Tests**
  - Path normalization: inside cwd → relative, inside home → `~/...`, outside → absolute
  - Title path replacement with various path positions

---

### 4. Spec 05: In-Progress Edit/Delete/Move Rendering

- [ ] **Routing: handle non-completed Edit/Delete/Move**
  - File: `tui/src/chatwidget/event_handlers.rs:handle_client_tool_snapshot_now`
  - Route Pending/InProgress/PendingApproval Edit/Delete/Move to `handle_client_native_tool_snapshot`
- [ ] **Cell replacement: spinner → PatchHistoryCell**
  - When completed Edit/Delete/Move arrives, flush the in-progress ClientToolCell and create PatchHistoryCell
- [ ] **Verb-appropriate labels**
  - File: `tui/src/client_tool_cell.rs` — new rendering branch for Edit/Delete/Move kinds
  - Edit → `Editing <path>`, Delete → `Deleting <path>`, Move → `Moving <from> → <to>`
- [ ] **Failed edits**
  - Red bullet with error text from artifacts
- [ ] **Tests**
  - Routing tests: non-completed edit creates ClientToolCell
  - Rendering tests: spinner + verb + path display
  - Transition test: in-progress → completed replaces cell

---

### 5. Spec 07: Diff Artifact Rendering in ClientToolCell

- [ ] **Render Artifact::Diff entries**
  - File: `tui/src/client_tool_cell.rs` or `client_event_format.rs`
  - Convert Diff artifacts to FileChange via `snapshot_file_changes`
  - Render via `create_diff_summary` from `diff_render.rs`
- [ ] **Scope to in-progress edits**
  - Show diff preview while edit is pending/in-progress (from spec 05)
  - For completed edits through PatchHistoryCell, diffs remain handled there
- [ ] **Tests**
  - Rendering test: in-progress edit with diff artifacts shows colored diff lines
  - Edge case: non-edit cell with diff artifacts

---

### 6. Spec 02: Exploring Cell Grouping

- [ ] **Extend ClientToolCell data model**
  - File: `tui/src/client_tool_cell.rs`
  - Change from single `ToolSnapshot` to `Vec<ToolSnapshot>` (or parallel sub-items list)
  - Update `apply_snapshot`, `is_active`, `call_id` for multi-snapshot
- [ ] **Merge exploring snapshots**
  - File: `tui/src/chatwidget/event_handlers.rs:handle_client_native_tool_snapshot`
  - When active cell is exploring ClientToolCell and new snapshot is exploring, merge
- [ ] **Port sub-item rendering**
  - Reference: `tui/src/exec_cell/render.rs:exploring_display_lines`
  - Group consecutive reads: `Read file1.rs, file2.rs`
  - Show `Read`, `Search`, `List` labels with compact args
  - Header: `Exploring` (spinner) / `Explored` (dim bullet)
  - Tree prefix via `prefix_lines`
- [ ] **Omit read output content** in exploring cells
- [ ] **Tests**
  - Single exploring read → "Explored" cell with sub-item
  - Multiple reads grouped → "Explored" with comma-separated filenames
  - Mixed read + search → separate sub-items
  - Active exploring → "Exploring" with spinner
  - Non-exploring call breaks grouping

---

## Completed Work

_(None yet)_

## Open Questions

1. Should the synthetic ExecCell translation path (Read/Search → ExecCommandBeginEvent/EndEvent) be removed once spec 02 is complete, or kept as a fallback?
2. For spec 04, should path normalization happen in the normalizer (protocol layer) or purely at display time in the TUI? The specs say TUI layer since `cwd` is a TUI config value.
3. For spec 02, should `ClientToolCell` hold `Vec<ToolSnapshot>` directly, or should there be a separate `ExploringCell` type that implements `HistoryCell`?
