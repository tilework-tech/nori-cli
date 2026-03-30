# Spec 12: Execute Cell Completion Buffering and Output Correctness

## Summary

Fix three related problems with how ACP execute tool cells are rendered in history:

1. **Premature flush loses real output.** When multiple tool calls arrive in parallel, each new pending snapshot flushes the previous active cell to history before the completion (with stdout) arrives. The completion is then discarded by the `completed_client_tool_calls` guard. The flushed cell permanently displays the agent's description text as if it were command output.

2. **Description text rendered as command output.** Claude sends the tool `description` in the `content` array of the in-progress update (e.g., `"Print current UTC date/time with format flags"`). The `execute_output_text` function falls back to `Artifact::Text` when no `raw_output.stdout` is present yet, rendering the description as stdout.

3. **Minor rendering artifacts.** A single-snapshot Read misroutes through the execute renderer (`Ran Read File`), and Codex exploring cells show `List List /path` with a doubled label.

## Evidence

### Premature flush (Claude parallel commands)

From `screen-examples-newest/debug-acp-claude.log`, the three parallel shell commands interleave:

```
Line 50: tool_call       toolu_01A (date)     — pending, title: "Terminal"
Line 51: tool_call_update toolu_01A (date)     — title: "date ...", content: [description text]
Line 52: tool_call       toolu_01G (uptime)   — pending ← TRIGGERS flush of date cell
Line 53: tool_call_update toolu_01A (date)     — toolResponse: {stdout: "2026-03-30 ..."}
Line 54: tool_call_update toolu_01A (date)     — completed, rawOutput: "2026-03-30 ..."
Line 55: tool_call_update toolu_01G (uptime)   — title: "uptime -p", content: [description text]
Line 56: tool_call       toolu_01R (df)       — pending ← TRIGGERS flush of uptime cell
Line 57: tool_call_update toolu_01G (uptime)   — toolResponse: {stdout: "up 1 week ..."}
Line 58: tool_call_update toolu_01G (uptime)   — completed, rawOutput: "up 1 week ..."
Line 59: tool_call_update toolu_01R (df)       — title: "df -h ...", content: [description text]
Line 60: tool_call_update toolu_01R (df)       — toolResponse: {stdout: "Filesystem ..."}
Line 61: tool_call_update toolu_01R (df)       — completed, rawOutput: "Filesystem ..."
Line 62: agent_message_chunk                   — (text streaming starts)
```

Trace of what happens in the TUI:

1. **Line 51** → `handle_client_native_tool_snapshot(date)`: creates `ClientToolCell` in `active_cell`. Snapshot has `content: [{text: "Print current UTC date/time..."}]` which normalizes to `Artifact::Text`. No `raw_output` yet.

2. **Line 52** → `handle_client_native_tool_snapshot(uptime)`: different `call_id`, so hits line 1344 → `self.flush_active_cell()`. The **incomplete** date cell is sent to history. Its `call_id` is added to `completed_client_tool_calls`.

3. **Lines 53-54** → `handle_client_native_tool_snapshot(date completed)`: `completed_client_tool_calls.contains(call_id)` → **discarded** at line 1324-1329. The real stdout never reaches the cell.

4. The date cell in history permanently shows: `Running date --utc ...` → `└ Print current UTC date/time with format flags` (the description, not the output).

The same pattern repeats for `uptime` (flushed by `df` pending at line 56).

Only `df` renders correctly because it completes before the next agent_message_chunk at line 62 triggers text streaming.

### Visible result in screen output

From `screen-examples-newest/screen-capture-claude.log`:

```
• Running date --utc +"%Y-%m-%d %H:%M:%S %Z"         ← should be "Ran", with stdout
  └ Print current UTC date/time with format flags      ← description, not stdout

• Running uptime -p                                     ← should be "Ran", with stdout
  └ Show uptime in pretty/human-readable format         ← description, not stdout

• Ran df -h --type=ext4                                 ← correct (wasn't flushed early)
  └ Filesystem             Size  Used Avail Use% ...    ← real stdout
```

### Description-as-output for empty-stdout commands

From `screen-examples-newest/screen-capture-claude.log:73-74`:

```
• Ran rm /home/clifford/.../tmp.md
  └ Delete the temporary test file                      ← should be "(no output)"
```

The `rm` command has `stdout: ""` and `noOutputExpected: true` (debug log line 68). But the in-progress snapshot carried `content: [{text: "Delete the temporary test file"}]` (the description). The `execute_output_text` function returns the description text because `raw_output.stdout` is empty string (falsy check: `as_str` returns `Some("")` which is truthy, but the text is then treated as real output — actually, the completed snapshot at line 69 has no `content`/`rawOutput` at all, so the cell retains whichever artifacts it had from the in-progress snapshot).

Wait — re-examining: the completed snapshot (line 69) does not carry `rawOutput` or `content`. So `apply_snapshot` replaces the snapshot entirely (line 93: `self.snapshot = snapshot`), which means the completed snapshot has no artifacts. Then `execute_output_text` returns `None`, and the cell shows `(no output)`. But this only works if the completion event *reaches* the cell.

For the `rm` case specifically, the cell was NOT flushed early (it was the only active command). The cell receives the `toolResponse` update (line 68, with `stdout: ""`), then the completed update (line 69, with empty `rawOutput: ""`). After `apply_snapshot`, the snapshot's `raw_output` field has `stdout: ""`. The `execute_output_text` function finds `stdout` via `as_str()` → `Some("")` and returns `Some("")`. Line 363-369 then checks `text.is_empty()` → true → renders `(no output)`.

So the `rm` case actually works correctly **when the completion reaches the cell**. But looking at the screen output, line 74 shows `└ Delete the temporary test file` — which means this is the in-progress snapshot's description text. This suggests either:
- The `toolResponse` update (line 68) set `raw_output` on the snapshot but kept the old `content` artifacts
- Or the cell was rendered from an intermediate state

Looking more carefully at the protocol normalizer: the `toolResponse` update at line 68 has no `rawOutput`/`content`/`status` — it only has `_meta.claudeCode.toolResponse`. The normalizer may not be propagating `stdout` from the `_meta.claudeCode.toolResponse` into `snapshot.raw_output`. The separate completed update at line 69 has `rawOutput: ""` and no `content`. So after the final `apply_snapshot`, `raw_output` is `Some(json!(""))` — a bare empty string, not `{"stdout": ""}`. Then `raw_output.get("stdout")` returns `None`, and the function falls through to `Artifact::Text` from the earlier in-progress snapshot... except `apply_snapshot` replaces the entire snapshot, so the old artifacts are gone.

This needs investigation in the normalizer, but the fix is the same: don't render description-only artifacts as command output.

### Read misrouted as execute

From `screen-examples-newest/screen-capture-claude.log:11`:

```
• Ran Read File
  └ ```
       1→# Nori CLI
  … +3 lines
```

This is a Read tool that initially arrives as `pending` with `title: "Read File"` and `kind: "read"` (debug log line 12). It should render as `Explored` → `└ Read README.md`, not `Ran Read File`.

The issue: a single Read snapshot arrives as `pending` → creates a ClientToolCell → gets update with full title/content → but the `kind` is `read`, not `execute`. Looking at `display_lines` (line 500-508):

```rust
if !self.exploring_snapshots.is_empty() {
    self.render_exploring_lines(width)
} else if self.snapshot.kind == nori_protocol::ToolKind::Execute {
    self.render_execute_lines(width)
} else {
    self.render_generic_lines()
}
```

A Read snapshot with `exploring_snapshots` empty goes to `render_generic_lines`, not `render_execute_lines`. So the `Ran Read File` display must be coming from somewhere else. Let me re-examine — the screen output says `Ran Read File`, which uses the execute verb "Ran". This means either:
- The snapshot's `kind` was set to `Execute` by the normalizer
- Or the cell was an ExecCell, not a ClientToolCell

Given the debug log shows `kind: "read"`, this is likely a normalizer issue where `_meta.claudeCode.toolName: "Read"` is being mapped differently. Or — the initial pending snapshot (title: "Read File", kind: "read") was flushed to history before the update arrived, and then the update created a *second* cell that was also flushed. The `Ran` verb implies `render_execute_lines` ran, which implies `snapshot.kind == Execute`.

Regardless of root cause, this is a display path that should be fixed by the buffering approach.

### Codex `List List` duplication

From `screen-examples-newest/screen-capture-codex.log:30`:

```
• Explored
  └ List List /home/clifford/...
```

The exploring cell sub-item renderer prefixes the tool kind as a label (`List`), but the `title` field from Codex already starts with `"List"`, producing `List List /path`.

## Root Cause Analysis

The core problem is that `handle_client_native_tool_snapshot` eagerly creates and flushes `ClientToolCell` instances:

1. **Line 1344**: `self.flush_active_cell()` — before creating a new cell, the current active cell is unconditionally flushed to history, regardless of whether it has received its completion.

2. **Lines 17-21 in user_input.rs**: When flushing an active `ClientToolCell`, its `call_id` is added to `completed_client_tool_calls`.

3. **Lines 1324-1329**: Later completion events for that `call_id` are **silently discarded** because the `call_id` is already in the set.

This design was intentional for chronological ordering (tool cells appear before subsequent agent text), but it means parallel ACP tool calls produce permanently incomplete cells in history.

The secondary problem is that `execute_output_text` (line 458-476) doesn't distinguish between tool descriptions and command output. Claude's `content` array on in-progress execute updates contains the agent's `description` field, not stdout. The function should not return this text as command output.

## Required Changes

### 1. Buffer pending execute cells instead of flushing them incomplete

Replace the unconditional `flush_active_cell()` in `handle_client_native_tool_snapshot` (line 1344) with a buffering strategy for execute tool cells:

Introduce a `pending_client_tool_cells: HashMap<String, ClientToolCell>` field on `ChatWidget` (analogous to `pending_exec_cells` for legacy ExecCells). When a new tool snapshot arrives and would displace an active incomplete ClientToolCell:

- If the active cell is an **incomplete execute** ClientToolCell, move it to `pending_client_tool_cells` keyed by call_id instead of flushing to history.
- The cell stays in the buffer, invisible to the user, until its completion event arrives.
- When a completion event arrives for a buffered call_id, update the cell with `apply_snapshot`, then flush it to history in its correct position.

For non-execute cells (exploring, generic) and for already-completed cells, the current behavior is fine — flush normally.

### 2. Drain buffered cells at turn boundaries

At turn completion (`on_agent_message`, `on_task_complete`), drain all remaining cells from `pending_client_tool_cells`:

- **Cells that received their completion** (should not happen — they'd already be flushed): flush to history.
- **Cells still incomplete**: do NOT flush to history. Instead, discard them silently. An execute cell that never completed within the turn is an orphan — flushing it with partial/wrong content (description text, no real output) pollutes the scrollback. The agent's final summary text already describes what happened.

This is a deliberate choice: **silence is better than wrong output**. A cell showing `Running date ...` → `└ Print current UTC date/time...` is actively misleading because it looks like the command's output was the description text. Omitting the cell entirely is less harmful — the completed command's output appears in the agent's summary message anyway.

### 3. Update completion handling to check the buffer

In `handle_client_native_tool_snapshot`, after the `completed_client_tool_calls` guard (line 1324), add a check for `pending_client_tool_cells`:

```rust
// Check if this completion is for a buffered cell
if let Some(mut buffered_cell) = self.pending_client_tool_cells.remove(&tool_snapshot.call_id) {
    buffered_cell.apply_snapshot(tool_snapshot);
    self.add_boxed_history(Box::new(buffered_cell));
    return;
}
```

This allows the completion to reach the cell even though it's no longer in `active_cell`.

### 4. Don't add buffered cells to `completed_client_tool_calls`

Currently, `flush_active_cell` (user_input.rs:17-21) adds active ClientToolCell call_ids to `completed_client_tool_calls`. When moving a cell to the buffer instead of flushing, do NOT add the call_id to the completed set — the cell is still waiting for its completion.

### 5. Filter description-only artifacts from execute output

In `execute_output_text` (client_tool_cell.rs:458-476), add a guard to avoid returning text that is purely description content:

- Check if the snapshot has `raw_output` with a `stdout` key (even if empty string). If `raw_output.stdout` exists, use it exclusively — don't fall back to artifacts.
- If no `raw_output` exists at all (in-progress state), return `None` instead of falling back to `Artifact::Text`. The description text in `content` is not stdout.

More precisely: for Execute tool kinds, the artifact text fallback should only activate when the snapshot is `Completed` or `Failed` and has no `raw_output`. During `Pending`/`InProgress`, artifact text for execute tools is always the description, never stdout.

### 6. Fix Read snapshot misroute

The pending Read snapshot with `title: "Read File"` should not be rendered with `Ran` verb. Two fixes:

- In `handle_client_native_tool_snapshot`, when a pending Read arrives as the first snapshot (no prior exploring cell), mark it as exploring immediately via `cell.mark_exploring()` — this already happens at line 1356-1358, so the issue may be that the pending snapshot isn't classified as exploring because `is_exploring_snapshot` checks `snapshot.kind` and `snapshot.invocation`.

- Check `is_exploring_snapshot`: it matches on `ToolKind::Read` and `ToolKind::Search` (line 45-53 in client_event_format.rs), so a pending Read *should* be classified as exploring. If the cell is being flushed before the exploring flag is set, the buffering fix (change 1) would also resolve this.

Trace: the pending Read (debug line 12) has `kind: "read"` → `is_exploring_snapshot` returns `true` → line 1356-1358 calls `cell.mark_exploring()` → but then line 1360 checks `should_flush` which is `false` for exploring → cell stays in `active_cell`. Then the update (debug line 13) has the same call_id → line 1308-1320 applies the snapshot and since it's exploring, doesn't flush. Then the completion (debug line 15) applies the snapshot → `!is_active() && !is_exploring()` — wait, `is_exploring()` checks `!exploring_snapshots.is_empty()`, but a single read doesn't get merged (it's the primary snapshot, not in `exploring_snapshots`). So `!is_exploring()` is `true` → cell gets flushed.

The root issue: a single-read ClientToolCell has `exploring_snapshots` empty (it's just the primary `snapshot`), so `is_exploring()` returns `false`, and `display_lines` doesn't enter `render_exploring_lines`. It falls to `render_generic_lines`. The `Ran Read File` text must be coming from `render_generic_lines` which shows `"Tool [completed]: Read File (read)"` — not `Ran`. So the screen output `Ran Read File` is inconsistent with my analysis.

This needs direct investigation during implementation. The fix should ensure single Read snapshots render as `Explored` → `└ Read {filename}` regardless of whether they're in `exploring_snapshots` or are the primary snapshot. The simplest fix: in `display_lines`, also enter `render_exploring_lines` when `self.snapshot.kind` is `Read`/`Search` or `self.snapshot.invocation` is `ListFiles`, even if `exploring_snapshots` is empty. Treat the primary snapshot as the single exploring sub-item.

### 7. Fix `List List` duplication in exploring cells

In `render_exploring_lines`, when rendering a sub-item for a `ListFiles` snapshot, the label is prefixed with `"List"`, but the snapshot `title` from Codex already starts with `"List ..."`. Detect and strip the redundant prefix: if the title starts with the label text (case-insensitive), use the title directly instead of prepending the label.

## Scope

- Add `pending_client_tool_cells: HashMap<String, ClientToolCell>` to `ChatWidget`
- Modify `handle_client_native_tool_snapshot` to buffer incomplete execute cells instead of flushing
- Add completion-from-buffer path for deferred completions
- Drain and discard orphan cells at turn boundaries (not flush them)
- Filter description-only artifacts from `execute_output_text` for in-progress execute snapshots
- Fix single-read exploring display path
- Fix `List List` duplication
- Add tests: parallel execute cells buffer and complete, orphan cells discarded, description text not shown as output, single read renders as explored

## Non-Goals

- This spec does not change the legacy ExecCell / `pending_exec_cells` buffering (that path works for its use cases)
- This spec does not change the interrupt queue / `defer_or_handle` mechanism (that handles text-streaming interleaving, a different concern)
- This spec does not address the `rm` empty-stdout description issue if it's a normalizer bug (the buffering fix may resolve it; if not, a separate normalizer fix is needed)

## Ordering

This spec is independent of specs 09–11. It can be implemented first because it fixes the most user-visible regressions (wrong output text in every parallel execute batch).
