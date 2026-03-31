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
  ├─ Edit/Delete/Move + Completed + file_changes → PatchHistoryCell
  ├─ Execute                                     → ClientToolCell (render_execute_lines)
  ├─ Read/Search/Fetch/Think/Other pending       → ClientToolCell (exploring merge / exec-like begin)
  ├─ Read/Search/Fetch/Think/Other completed     → ClientToolCell (exploring merge / exec-like complete)
  └─ Non-completed Edit/Delete/Move              → ClientToolCell (spinner + diff preview)
```

### Key Types

| Type | Crate | Role |
|------|-------|------|
| `ToolSnapshot` | `nori-protocol` | Normalized tool call state: call_id, title, kind, phase, locations, invocation, artifacts, raw_input/output |
| `ToolKind` | `nori-protocol` | Read, Search, Execute, Edit, Delete, Move, Fetch, Think, Other(String) |
| `ToolPhase` | `nori-protocol` | Pending, PendingApproval, InProgress, Completed, Failed |
| `Invocation` | `nori-protocol` | Structured input: Command, Read, Search, ListFiles, FileChanges, FileOperations, Tool, RawJson |
| `Artifact` | `nori-protocol` | Output data: Text { text }, Diff(FileChange) |
| `ClientToolCell` | `nori-tui` | ACP tool rendering: single-snapshot or multi-snapshot exploring cell |
| `ExecCell` | `nori-tui` | Legacy multi-call cell (non-ACP path) |
| `PatchHistoryCell` | `nori-tui` | Completed edit diff rendering (bridge to legacy FileChange model) |

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

## Completed Work (specs 01–08, 10, 12)

Ten specs were implemented across commits `512c505e`..HEAD. Summary:

| Spec | What it delivered | Commit |
|------|-------------------|--------|
| 01 — Execute Native Rendering | `render_execute_lines`: semantic verbs (`Ran`/`Running`), bash highlighting, green/red exit-code bullet, output under `└` with truncation, `(no output)`, word-wrapping | `512c505e` |
| 02 — Exploring Cell Grouping | `exploring_snapshots: Vec<ToolSnapshot>`, `render_exploring_lines` with `Explored`/`Exploring` header, grouped reads, Search/List sub-items | `2a482c09` |
| 03 — Codex Command Array Extraction | Codex `rawInput.command` array normalized to `Invocation::Command` | `cc12bf6b` |
| 04 — Path Display Normalization | `cwd` threaded into `ClientToolCell`, `relativize_paths_in_text()` strips cwd prefix | `f4320a7e` |
| 05 — In-Progress Edit Rendering | Non-completed Edit/Delete/Move routed to `ClientToolCell` with spinner; completed edits discard spinner before adding PatchHistoryCell | `94268dc0` |
| 06 — Artifact Text Cleanup | Code fences stripped via `strip_code_fences()`, `Output:` prefix removed, redundant invocation suppressed | `771bca1a` |
| 07 — Diff Artifact Rendering | `Artifact::Diff` converted to `FileChange` and rendered via `create_diff_summary` for in-progress edit previews | `7e7e9f96` |
| 08 — Gemini Empty Content Fallback | Location-based invocation fallback, Gemini title sanitization, minimal completed cell rendering | `12f3fae5` |
| 10 — Failed Edit Tool Visibility | `format_edit_tool_header()`: semantic verb headers for Edit/Delete/Move; red bullet for Failed; error text fallback from `raw_output`; duplicate-cell prevention | *pending commit* |
| 12 — Execute Cell Completion Buffering | Parallel execute buffering, description text filtering, List dedup | `c23b3af4` |

Test coverage: 32 unit tests in `client_tool_cell.rs`, 9 integration tests in `chatwidget/tests/part3.rs` and `part5.rs`.

### Learnings from Spec 10

- **ChatWidget doesn't hold history cells.** History cells are sent via `AppEvent::InsertHistoryCell` to the main app event loop. The ChatWidget cannot scan or remove previously-flushed cells. Duplicate-cell prevention must be done proactively via `completed_client_tool_calls` tracking rather than reactively scanning history.
- **Semantic verb headers reuse the path from `locations[0]`.** The `format_edit_tool_header()` function extracts the path from the first location entry, falling back to parsing it from the title string. This is more reliable than relying on the title alone, since title formats vary across providers.

---

## Remaining Work (specs 09, 11)

Two specs remain. Each has a detailed document in [`./specs/`](./specs/).

### Spec 09: ACP-Native Approval Rendering
**File:** [`specs/09-acp-native-approval-rendering.md`](specs/09-acp-native-approval-rendering.md)

All ACP approval requests are force-fit into legacy `ApprovalRequest::Exec` or `ApplyPatch`. This produces broken history text (`✔ You approved Nori to runrm /path...` — missing space, raw command concatenated) and wrong overlay content for non-execute tools. Adds `ApprovalRequest::AcpTool` variant with native protocol fields, new overlay renderer, and correct decision history cells.

### Spec 11: Delete File Operation Bridge
**File:** [`specs/11-delete-file-operation-bridge.md`](specs/11-delete-file-operation-bridge.md)

Removes the compatibility bridge converting `nori_protocol` file types back to `codex_core::protocol::FileChange`. Adds `render_edit_lines` to `ClientToolCell`, unifies all Edit/Delete/Move phases through `handle_client_native_tool_snapshot`, deletes bridge functions. **Depends on** spec 10 (✅ done); approval bridge persists until spec 09 lands.

---

## Dependency Graph and Recommended Order

```
Spec 12 (Completion Buffering)   ✅ done
Spec 10 (Failed Edit Visibility) ✅ done
Spec 09 (Approval Rendering)     ← independent, next priority
Spec 11 (Delete File Bridge)     ← depends on 10 ✅; approval bridge waits for 09
```

1. **Spec 09** — unblocks full spec 11 bridge deletion
2. **Spec 11** — final cleanup
