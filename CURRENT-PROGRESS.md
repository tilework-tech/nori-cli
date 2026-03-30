# ACP TUI Rendering Fixes — Current Progress

## Completed Specs

### Spec 08: Gemini Empty Content Fallback
- **Commit:** `12f3fae5`
- Location-based invocation fallback when `raw_input` is None
- Title sanitization for Gemini `[current working directory ...]` suffixes
- Minimal completed cell renders locations when no other details available

### Spec 06: Artifact Text Output Cleanup
- **Commit:** `771bca1a`
- Code fences stripped from generic artifact text via shared `strip_code_fences()`
- `Output:` prefix removed from artifact rendering
- Redundant invocation lines suppressed via `is_invocation_redundant()`
- `strip_code_fences` shared between execute and generic rendering paths

### Spec 04: Path Display Normalization
- **Commit:** `f4320a7e`
- `cwd` field threaded into `ClientToolCell` struct and constructor
- `relativize_paths_in_text()` helper strips cwd prefix from title and invocation strings
- Paths inside cwd render as relative; execute commands unmodified
- Call site in `event_handlers.rs` updated to pass `self.config.cwd`

### Spec 05: In-Progress Edit/Delete/Move Rendering
- **Commit:** `94268dc0`
- Non-completed Edit/Delete/Move routed to `handle_client_native_tool_snapshot`
- Users see spinner during edits instead of silent drop
- Completed edits discard (not flush) matching spinner cell before adding PatchHistoryCell
- Prevents duplicate cells in history during spinner → diff transition

### Spec 07: Diff Artifact Rendering in ClientToolCell
- **Commit:** `7e7e9f96`
- `Artifact::Diff` entries extracted and converted to `codex_core::protocol::FileChange`
- Rendered via `create_diff_summary` from `diff_render.rs`
- Shows inline diff previews for in-progress edits

### Spec 02: Exploring Cell Grouping
- **Commit:** `2a482c09`
- `ClientToolCell` extended with `exploring_snapshots: Vec<ToolSnapshot>`
- `mark_exploring()` and `merge_exploring()` methods for grouping lifecycle
- `render_exploring_lines()` with compact Explored/Exploring header
- Consecutive reads grouped by filename: `Read file1.rs, file2.rs`
- Search/List sub-items with labels and compact arguments
- Event handler merge logic: exploring snapshots merge into active exploring cell

## Implementation Order

All specs implemented in the recommended order from APPLICATION-SPEC.md:
1. Spec 08 (Gemini fallback) → 2. Spec 06 (Artifact cleanup) → 3. Spec 04 (Path normalization) → 4. Spec 05 (In-progress edits) → 5. Spec 07 (Diff rendering) → 6. Spec 02 (Exploring grouping)

## Test Coverage

- 19 new tests in `client_tool_cell.rs` (unit tests for all rendering paths)
- 4 new tests in `chatwidget/tests/part3.rs` (integration tests for routing and merge)
- All 1123 existing tests continue to pass
