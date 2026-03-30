# Spec 05: In-Progress Edit/Delete/Move Rendering

## Summary

Show pending and in-progress edit/delete/move tool snapshots with a spinner and file path, instead of silently dropping them. Currently, only `Completed` edits with available file changes are rendered; all other phases are discarded.

## Expected Behavior

When an edit tool call arrives with `phase: Pending` or `phase: InProgress`:

```
⠋ Editing README.md
```

When it completes, it transitions to the existing `PatchHistoryCell` diff rendering:

```
• Edited README.md (+1 -1)
    1 -# Nori CLI
    1 +# Nori CLI (TEST EDIT)
```

For delete:
```
⠋ Deleting tmp.md
```

For move:
```
⠋ Moving old.rs → new.rs
```

## Actual Behavior

From `screen-examples-new/debug-acp-claude.log:29-34`, the edit lifecycle is:

```
Line 29: tool_call       — status: pending, title: "Edit", kind: edit
Line 30: tool_call_update — title: "Edit .../README.md", rawInput: {...}, content: [diff]
Line 31: request_permission — approval request with diff
          (user approves)
Line 33: tool_call_update — content: [diff with full context], status still in_progress
Line 34: tool_call_update — status: completed
```

The pending and in-progress snapshots (lines 29-30, 33) hit the `_ => {}` arm in `handle_client_tool_snapshot_now` and are silently dropped. Only the completed snapshot (line 34) triggers `handle_client_edit_tool_snapshot`. This means there is no visual feedback while the edit is being applied.

## Root Cause

`tui/src/chatwidget/event_handlers.rs:1299-1320`:

```rust
fn handle_client_tool_snapshot_now(&mut self, tool_snapshot: nori_protocol::ToolSnapshot) {
    match tool_snapshot.kind {
        nori_protocol::ToolKind::Edit
        | nori_protocol::ToolKind::Delete
        | nori_protocol::ToolKind::Move
            if tool_snapshot.phase == nori_protocol::ToolPhase::Completed
                && crate::client_event_format::snapshot_file_changes(&tool_snapshot)
                    .is_some() =>
        {
            self.handle_client_edit_tool_snapshot(tool_snapshot);
        }
        nori_protocol::ToolKind::Execute
        | nori_protocol::ToolKind::Read
        // ...
        => {
            self.handle_client_native_tool_snapshot(tool_snapshot);
        }
        _ => {}  // <-- Edit/Delete/Move with phase != Completed land here
    }
}
```

The guard `if tool_snapshot.phase == Completed && snapshot_file_changes().is_some()` means:
- Pending/InProgress/PendingApproval edits → `_ => {}` (dropped)
- Completed edits without parseable file changes → `_ => {}` (dropped)
- Failed edits → `_ => {}` (dropped)

## Scope

1. Route non-completed Edit/Delete/Move snapshots to `handle_client_native_tool_snapshot` so they render as a `ClientToolCell` with a spinner
2. When a completed Edit/Delete/Move arrives, replace the in-progress `ClientToolCell` in the active cell slot with the `PatchHistoryCell` (flush the spinner cell, add the diff cell)
3. For failed edits, show a red bullet with the error from artifacts
4. The in-progress rendering should extract the file path from `snapshot.locations[0].path` or `snapshot.title` and show a verb-appropriate label:
   - Edit → `Editing <path>`
   - Delete → `Deleting <path>`
   - Move → `Moving <from> → <to>`
