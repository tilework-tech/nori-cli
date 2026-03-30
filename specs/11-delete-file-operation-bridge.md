# Spec 11: Delete File Operation Compatibility Bridge

## Summary

Remove the compatibility bridge that converts `nori_protocol` file operation types back into `codex_core::protocol::FileChange` for ACP edit/delete/move tool rendering. After stash@{0} specs 05 and 07, `ClientToolCell` can render in-progress edits with spinners and diff artifacts directly from `nori_protocol` types. This spec completes the migration by routing completed file operations through `ClientToolCell` as well, eliminating the `PatchHistoryCell` detour and the bridge code.

## Current Bridge Code

### Conversion functions (event_handlers.rs:1667-1777)

Three functions convert `nori_protocol` types to `codex_core::protocol::FileChange`:

- `file_changes_from_snapshot` (line 1667) — dispatches on `Invocation::FileChanges` vs `Invocation::FileOperations`
- `file_change_from_nori_operation` (line 1719) — converts `FileOperation::{Create,Update,Delete,Move}` to `FileChange::{Add,Update,Delete}`
- `file_change_from_nori_change` (line 1765) — converts `nori_protocol::FileChange` (old_text/new_text pair) to `codex_core::protocol::FileChange`

### Duplicate conversion in ClientToolCell (client_tool_cell.rs:478-497)

`diff_changes_from_artifacts` performs the same `nori_protocol::FileChange` → `codex_core::protocol::FileChange` conversion, but operating on `Artifact::Diff` entries rather than `Invocation` data. This was added in spec 07 for rendering diff artifacts in the generic path.

### How the bridge is consumed

1. **Completed edit routing** (event_handlers.rs:1254-1262):
   ```
   Edit|Delete|Move + Completed + file_changes_from_snapshot().is_some()
     → handle_client_edit_tool_snapshot()
   ```

2. **`handle_client_edit_tool_snapshot`** (event_handlers.rs:1387-1410):
   - Calls `file_changes_from_snapshot` to get `HashMap<PathBuf, FileChange>`
   - Passes to `history_cell::new_patch_event(changes, cwd)` → `PatchHistoryCell`

3. **Approval overlay** (event_handlers.rs:1644-1657):
   ```
   Edit/Delete/Move with file_changes → ApprovalRequest::ApplyPatch { changes }
   ```

4. **Fullscreen preview** (app/event_handling.rs:491-497):
   ```
   ApplyPatch → DiffSummary::new(changes, cwd) in "P A T C H" overlay
   ```

## Why the bridge can go

`ClientToolCell` already renders:
- **Diff artifacts** via `diff_changes_from_artifacts` → `create_diff_summary` (spec 07)
- **In-progress edit spinners** with file path and verb headers (spec 05)
- **Path relativization** for all tool titles and invocations (spec 04)

The only thing `PatchHistoryCell` provides that `ClientToolCell` doesn't is the `"Edited README.md (+1 -1)"` header with add/remove line counts. That's addressed by spec 10's semantic verb headers for edit-like tools.

## Required Changes

### 1. Enhance `ClientToolCell` completed-edit rendering

Add a dedicated `render_edit_lines` method to `ClientToolCell` (alongside `render_execute_lines` and `render_exploring_lines`) that:

- Shows a verb header: `"Edited {path} (+N -M)"` / `"Added {path}"` / `"Deleted {path}"` / `"Moved {from} → {to}"`
- Renders diff content from `Artifact::Diff` entries using `create_diff_summary` (already working from spec 07)
- Falls back to `Invocation::FileChanges` / `Invocation::FileOperations` when no diff artifacts are present, converting to display lines directly from the `nori_protocol` types without going through `codex_core::protocol::FileChange`
- Uses green bullet for completed, red for failed

### 2. Route completed edits to ClientToolCell

In `handle_client_tool_snapshot_now` (event_handlers.rs:1254-1299), remove the special-case routing:

```
// Before:
Edit|Delete|Move + Completed + has_file_changes → handle_client_edit_tool_snapshot

// After:
Edit|Delete|Move → handle_client_native_tool_snapshot  (all phases)
```

`handle_client_native_tool_snapshot` already handles the full lifecycle (pending → in-progress → completed/failed) with `apply_snapshot`. The completed edit will now render through `render_edit_lines`.

### 3. Delete bridge functions

Remove from event_handlers.rs:
- `file_changes_from_snapshot` (line 1667)
- `file_change_from_nori_operation` (line 1719)
- `file_change_from_nori_change` (line 1765)
- `handle_client_edit_tool_snapshot` (line 1387)

### 4. Update approval overlay (depends on spec 09)

After spec 09 introduces `ApprovalRequest::AcpTool`, the approval overlay for edit approvals can use the `AcpTool` variant instead of `ApplyPatch`. The `ApplyPatch` variant can then be scoped to legacy non-ACP approval requests only.

If spec 09 is not yet implemented, keep `ApplyPatch` as-is for now — the bridge functions used by the approval path can remain temporarily until spec 09 lands.

### 5. Update or remove PatchHistoryCell usage

If all ACP file operations now render through `ClientToolCell`, `PatchHistoryCell` is only needed for legacy non-ACP `ApplyPatchEvent` handling. Check whether any non-ACP code paths still produce `PatchHistoryCell`; if so, keep it. If not, remove `PatchHistoryCell` and `new_patch_event` from history_cell/mod.rs.

### 6. Replace `diff_changes_from_artifacts` bridge

The `diff_changes_from_artifacts` function in client_tool_cell.rs:478-497 also converts `nori_protocol::FileChange` to `codex_core::protocol::FileChange`. Replace it with a direct rendering function that takes `&[nori_protocol::Artifact]` and produces diff display lines without the intermediate `codex_core` type. This can wrap `create_diff_summary` with a thin adapter or replace it with a native renderer that works directly on `nori_protocol::FileChange`.

## Scope

- Add `render_edit_lines` to `ClientToolCell`
- Unify Edit/Delete/Move routing to always use `handle_client_native_tool_snapshot`
- Delete bridge conversion functions
- Eliminate or reduce dependency on `codex_core::protocol::FileChange` in the ACP rendering path
- Add tests: completed edit through ClientToolCell, completed delete, completed move with both source and destination, add/remove line count in header

## Non-Goals

- This spec does not remove `PatchHistoryCell` if it's still used by legacy non-ACP code paths
- This spec does not change the approval rendering (that's spec 09)
- This spec does not change diff rendering internals (`create_diff_summary` stays)

## Ordering

This spec depends on spec 10 (semantic verb headers for edit-like tools) being complete or implemented simultaneously. It is independent of spec 09 (approval rendering) — the approval bridge can remain as a temporary compatibility layer.
