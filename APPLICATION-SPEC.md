# ACP TUI Rendering — Application Specification

## Goal

Make ACP tool calls render as compactly, cleanly, and informatively as the legacy ExecCell path — without relying on the synthetic `ExecCommandBeginEvent`/`ExecCommandEndEvent` translation layer.

## Architecture Context

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
  ├─ Execute                                     → ClientToolCell (render_execute_lines)
  ├─ Edit/Delete/Move (all phases)               → ClientToolCell (render_edit_lines)
  ├─ Read/Search/Fetch/Think/Other pending       → ClientToolCell (exploring merge / exec-like begin)
  ��─ Read/Search/Fetch/Think/Other completed     → ClientToolCell (exploring merge / exec-like complete)
  └─ Approval requests                           → ApprovalOverlay (three-way: ApplyPatch/Exec/AcpTool)
```

### Key Types

| Type | Crate | Role |
|------|-------|------|
| `ToolSnapshot` | `nori-protocol` | Normalized tool call state: call_id, title, kind, phase, locations, invocation, artifacts, raw_input/output |
| `ToolKind` | `nori-protocol` | Read, Search, Execute, Edit, Delete, Move, Fetch, Think, Other(String) |
| `ToolPhase` | `nori-protocol` | Pending, PendingApproval, InProgress, Completed, Failed |
| `Invocation` | `nori-protocol` | Structured input: Command, Read, Search, ListFiles, FileChanges, FileOperations, Tool, RawJson |
| `Artifact` | `nori-protocol` | Output data: Text { text }, Diff(FileChange) |
| `ClientToolCell` | `nori-tui` | ACP tool rendering: four-way dispatch (exploring, execute, edit, generic) |
| `ExecCell` | `nori-tui` | Legacy multi-call cell (non-ACP path) |
| `PatchHistoryCell` | `nori-tui` | Legacy non-ACP diff rendering only (no longer used for ACP) |

### Key Files

| File | Purpose |
|------|---------|
| `tui/src/chatwidget/event_handlers.rs` | ToolSnapshot routing dispatch |
| `tui/src/client_tool_cell.rs` | ClientToolCell struct, rendering, lifecycle |
| `tui/src/client_event_format.rs` | Format helpers: tool headers, invocations, artifacts, exploring classification |
| `tui/src/diff_render.rs` | Diff summary rendering, `display_path_for()` path normalization |
| `tui/src/exec_cell/render.rs` | Legacy ExecCell rendering including `exploring_display_lines()` |
| `nori-protocol/src/lib.rs` | ClientEventNormalizer, ToolSnapshot construction, artifact/invocation extraction |

---

## Completed Work (specs 01–12)

All twelve specs were implemented across commits `512c505e`..HEAD. Summary:

| Spec | What it delivered | Commit |
|------|-------------------|--------|
| 01 ��� Execute Native Rendering | `render_execute_lines`: semantic verbs (`Ran`/`Running`), bash highlighting, green/red exit-code bullet, output under `└` with truncation, `(no output)`, word-wrapping | `512c505e` |
| 02 — Exploring Cell Grouping | `exploring_snapshots: Vec<ToolSnapshot>`, `render_exploring_lines` with `Explored`/`Exploring` header, grouped reads, Search/List sub-items | `2a482c09` |
| 03 — Codex Command Array Extraction | Codex `rawInput.command` array normalized to `Invocation::Command` | `cc12bf6b` |
| 04 �� Path Display Normalization | `cwd` threaded into `ClientToolCell`, `relativize_paths_in_text()` strips cwd prefix | `f4320a7e` |
| 05 — In-Progress Edit Rendering | Non-completed Edit/Delete/Move routed to `ClientToolCell` with spinner; completed edits update in-place | `94268dc0` |
| 06 — Artifact Text Cleanup | Code fences stripped via `strip_code_fences()`, `Output:` prefix removed, redundant invocation suppressed | `771bca1a` |
| 07 ��� Diff Artifact Rendering | `Artifact::Diff` converted to `FileChange` and rendered via `create_diff_summary` for in-progress edit previews | `7e7e9f96` |
| 08 — Gemini Empty Content Fallback | Location-based invocation fallback, Gemini title sanitization, minimal completed cell rendering | `12f3fae5` |
| 09 — ACP-Native Approval Rendering | `ApprovalRequest::AcpTool` variant with three-way routing (Edit/Delete/Move→ApplyPatch, Execute+Command→Exec, everything else→AcpTool); native overlay/history/fullscreen for non-exec ACP tools | `2801dd03` |
| 10 — Failed Edit Tool Visibility | `format_edit_tool_header()`: semantic verb headers for Edit/Delete/Move; red bullet for Failed; error text fallback from `raw_output`; duplicate-cell prevention | `bd51a208` |
| 11 — Delete File Operation Bridge | `render_edit_lines`: dedicated edit/delete/move renderer with green/red bullet; unified routing through `handle_client_native_tool_snapshot`; `handle_client_edit_tool_snapshot` deleted; `PatchHistoryCell` no longer used for ACP | *pending commit* |
| 12 — Execute Cell Completion Buffering | Parallel execute buffering, description text filtering, List dedup | `c23b3af4` |
| 13 — Final Polish | `sanitize_tool_title()` for Gemini exec-like tools, single-file edit header dedup, `cwd` in `AcpTool` approval for path relativization | *pending commit* |

Test coverage: 57 unit tests in `client_tool_cell.rs`, `approval_overlay.rs`, and `client_event_format.rs`, 10 integration tests in `chatwidget/tests/part3.rs` and `part5.rs`.

### Learnings from Spec 13

- **Gemini title sanitization is simple string ops.** `[current working directory ...]` is always a single bracket pair and trailing `(description text)` is always at the end. No regex needed — `find`/`rfind` is sufficient.
- **Single-file DiffSummary header promotion avoids modifying diff_render.rs.** Rather than changing the shared `render_changes_block` to conditionally suppress its header, `render_edit_lines` in `client_tool_cell.rs` takes the first line from `create_diff_summary` and uses it as the outer header. This keeps the dedup logic in the consumer, not the shared renderer.
- **Move tools need the outer header preserved.** DiffSummary always says "Edited" for `FileChange::Update`, but Move tools need the verb "Moved". The dedup skips `ToolKind::Move` to keep the correct verb.
- **`cwd` threading through `ApprovalRequest::AcpTool` makes relativization consistent.** Previously the DiffSummary in the approval overlay used `PathBuf::from(".")` as cwd, which didn't relativize absolute paths. Now the real cwd is threaded from `handle_client_approval_request` through the enum variant.

### Learnings from Spec 11

- **`render_edit_lines` needs two diff sources.** Diff artifacts (`Artifact::Diff`) are the primary source, but some completed edits only have `Invocation::FileOperations` or `Invocation::FileChanges` without artifacts. The renderer tries artifacts first, then falls back to `changes_from_invocation()`.
- **Bridge functions removed.** The bridge functions (`file_changes_from_snapshot`, `file_change_from_nori_operation`, `file_change_from_nori_change`) that converted `nori_protocol` types to `codex_core::protocol::FileChange` for the approval overlay have been deleted. Edit/Delete/Move approvals now route through `ApprovalRequest::AcpTool` and reuse the same `pub(crate)` diff extraction helpers (`diff_changes_from_artifacts()`, `changes_from_invocation()`) from `client_tool_cell.rs`.
- **`observe_directories_from_paths` extracts paths from snapshot locations.** The old `handle_client_edit_tool_snapshot` observed directories from the changes HashMap keys. The new path uses `snapshot.locations` which already contain the file paths. Added `observe_directories_from_paths()` as a lighter-weight companion to `observe_directories_from_changes()`.
- **Completed edit cells get green bullets, matching Execute.** This is a visible UX improvement — previously completed edits went through `PatchHistoryCell` which had no bullet/header. Now they show `● Edited path` with green bullet and diff summary below.

### Learnings from Spec 10

- **ChatWidget doesn't hold history cells.** History cells are sent via `AppEvent::InsertHistoryCell` to the main app event loop. The ChatWidget cannot scan or remove previously-flushed cells. Duplicate-cell prevention must be done proactively via `completed_client_tool_calls` tracking rather than reactively scanning history.
- **Semantic verb headers reuse the path from `locations[0]`.** The `format_edit_tool_header()` function extracts the path from the first location entry, falling back to parsing it from the title string. This is more reliable than relying on the title alone, since title formats vary across providers.

### Learnings from Spec 09

- **ACP backend treats ExecApproval and PatchApproval identically.** Both call `handle_exec_approval` in `acp/src/backend/submit_and_ops.rs`. The `AcpTool` variant reuses `Op::ExecApproval` rather than introducing a new Op.
- **Two-way routing (updated from three-way).** Execute tools with `Invocation::Command` route to `ApprovalRequest::Exec` for bash syntax highlighting. Everything else (including Edit/Delete/Move) routes to `ApprovalRequest::AcpTool`, which renders diffs via `DiffSummary` for edit-like tools and falls back to text rendering for others. `ApplyPatch` is only used by the legacy non-ACP codex backend.
- **Protocol approval options not mapped to TUI options.** The `nori_protocol::ApprovalOption` entries carry option text and kind, but TUI approval options need keyboard shortcuts and agent display names that aren't in the protocol. The `AcpTool` variant generates its own options via `acp_tool_options()` with hardcoded Yes/Always/No choices and y/a/n keyboard shortcuts.
- **ToolSnapshot must be boxed in the enum.** The `ToolSnapshot` struct is 440+ bytes, triggering clippy's `large_enum_variant` lint. Boxing it as `Box<nori_protocol::ToolSnapshot>` keeps the `ApprovalRequest` enum size reasonable.

---

## Remaining Work

None. All thirteen specs are complete.

---

## Dependency Graph (Final)

```
Spec 12 (Completion Buffering)   ��� done
Spec 10 (Failed Edit Visibility) ✅ done
Spec 09 (Approval Rendering)     ✅ done
Spec 11 (Delete File Bridge)     ✅ done
Spec 13 (Final Polish)           ✅ done — all specs complete
```
