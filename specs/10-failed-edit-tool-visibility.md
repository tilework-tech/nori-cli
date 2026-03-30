# Spec 10: Failed Edit/Delete/Move Tool Visibility

## Summary

Verify and harden the rendering of failed Edit/Delete/Move ACP tool snapshots. After stash@{0} spec 05, non-completed edit-like tools are routed to `ClientToolCell` with a spinner. This spec ensures that the *failed* terminal state renders correctly: red bullet, error artifact text, and any partial diff artifacts are preserved — and that the completed-edit path correctly discards the in-progress spinner cell without leaving visual artifacts.

## Current State

### Routing (event_handlers.rs:1254-1299)

The match in `handle_client_tool_snapshot_now` routes:

```
Edit|Delete|Move + Completed + has_file_changes
  → handle_client_edit_tool_snapshot()  (PatchHistoryCell)

Edit|Delete|Move (all other phases, including Failed and Completed-without-file-changes)
  → handle_client_native_tool_snapshot()  (ClientToolCell)
```

This means failed edits *do* reach `ClientToolCell`. The question is whether they render well once there.

### ClientToolCell generic rendering (client_tool_cell.rs:250-299)

A failed Edit snapshot hits `render_generic_lines()` which:

1. Shows a dim `•` bullet (not red — the generic path doesn't use exit-code coloring)
2. Shows `format_tool_header` → `"Tool [failed]: Edit README.md (edit)"`
3. Shows `format_invocation` detail if not redundant with title
4. Shows `format_artifacts` text (code-fence-stripped)
5. Shows `diff_changes_from_artifacts` → `create_diff_summary` for any `Artifact::Diff` entries

### What's missing

1. **No red bullet for failed edits.** The generic renderer uses `spinner()` for active and `"•".dim()` for terminal, regardless of success/failure. Only `render_execute_lines` has green/red exit-code coloring.

2. **Failed edits with no artifacts render bare.** If the provider sends a failed edit with no text artifacts and no diff artifacts (just a phase transition to Failed), the cell renders as:
   ```
   • Tool [failed]: Edit README.md (edit)
   ```
   No error detail, no indication of *why* it failed.

3. **Completed-without-file-changes fallthrough.** If an edit completes but `file_changes_from_snapshot` returns `None` (e.g., the invocation type doesn't match `FileChanges` or `FileOperations`), it falls through to `ClientToolCell` generic rendering instead of `PatchHistoryCell`. This can happen when providers send diffs only in `artifacts` (as `Artifact::Diff`) rather than in the invocation. The diff artifact renderer (spec 07) covers this case visually, but the cell header still says `"Tool [completed]"` instead of the patch-style `"Edited README.md (+1 -1)"` header.

4. **Spinner-to-patch transition.** When a completed Edit arrives with file changes, `handle_client_edit_tool_snapshot` (event_handlers.rs:1387-1410) discards the active ClientToolCell spinner and replaces it with a PatchHistoryCell. This works, but if the active cell has already been flushed to history (due to interleaved text), both the spinner cell and the patch cell can end up in history.

## Required Changes

### 1. Red bullet for failed generic tools

In `render_generic_lines` (client_tool_cell.rs:250-258), replace the unconditional dim bullet with phase-aware coloring:

```rust
let bullet = if self.is_active() {
    spinner(self.start_time, self.animations_enabled)
} else if self.snapshot.phase == nori_protocol::ToolPhase::Failed {
    "•".red().bold()
} else {
    "•".dim()
};
```

This benefits all failed generic tool cells, not just edits.

### 2. Semantic header for edit-like tools

When `snapshot.kind` is Edit/Delete/Move, the generic header should use a verb instead of the generic `"Tool [phase]"` format:

- Pending/InProgress Edit → `"Editing {path}"` (already shown via spinner)
- Completed Edit (fallthrough) → `"Edited {path}"`
- Failed Edit → `"Edit failed: {path}"`
- Similarly for Delete → `"Deleting"` / `"Deleted"` / `"Delete failed"`
- Similarly for Move → `"Moving"` / `"Moved"` / `"Move failed"`

Extract the path from `snapshot.locations[0].path` or parse from `snapshot.title`.

### 3. Error text from failed tools

When a tool is in `Failed` phase and has no text artifacts, check for error information in:
- `snapshot.raw_output` — some providers include error text here
- `snapshot.title` — some providers embed the error in the title update

If no error text is available, render `"(failed)"` in dim red as a detail line, so the cell is never completely silent about the failure.

### 4. Prevent duplicate cells on flush-then-complete

In `handle_client_edit_tool_snapshot` (event_handlers.rs:1387-1410), the current code discards the active cell if it matches the call_id. But if the cell was already flushed to history (because interleaved text triggered `flush_active_cell`), the spinner cell persists in history and the patch cell is added separately. Add a scan of recent history cells to remove a matching ClientToolCell before inserting the PatchHistoryCell, or track the call_id in `completed_client_tool_calls` earlier to prevent the spinner cell from flushing in the first place.

## Scope

- Add phase-aware bullet coloring to `render_generic_lines`
- Add edit/delete/move verb headers to `render_generic_lines`
- Add error fallback text for failed tools with no artifacts
- Harden the spinner-to-patch transition to prevent duplicate cells
- Add tests: failed edit red bullet, failed edit with error artifact, failed edit with diff artifact, completed edit without file_changes fallthrough

## Non-Goals

- This spec does not change the routing logic (the stash@{0} spec 05 routing is correct)
- This spec does not add failed-tool rendering to the transcript pager (out of scope per user direction)
