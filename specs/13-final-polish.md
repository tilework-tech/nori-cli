# Spec 13: Final Polish — Title Sanitization, Deduplication, and Path Relativization

## Summary

Three cosmetic rendering issues survived the core spec work (specs 01–12 plus the spec 11 step 4 follow-up). This spec fixes them in a single pass:

1. **Gemini title sanitization for exec-like tools.** Gemini embeds `[current working directory ...]` and trailing `(description text)` in its titles. Spec 08 added `sanitize_gemini_title` / `format_edit_tool_header` for ClientToolCell headers, but exec-like tools (Read, Search, ListFiles, and generic Execute without `Invocation::Command`) route through `exec_begin_event_from_client_snapshot`, which passes `snapshot.title` raw into `ExecCommandBeginEvent.command`. The user sees long, noisy titles in the TUI.

2. **Duplicate nested cell title for single-file edits.** When an Edit/Delete/Move tool completes with a single file change, the outer header says `● Edited README.md` and the nested `DiffSummary` repeats `● Edited README.md (+1 -1)`. Both come from `render_edit_lines` (client_tool_cell.rs) calling `format_edit_tool_header` for the outer header, then `create_diff_summary` (diff_render.rs) which generates its own verb+path header. The intent was for the outer header to cover the "what tool ran" context while `DiffSummary` shows the diff detail, but for single-file edits they render identically except for the line counts.

3. **Approval titles and decision cells show absolute paths.** `approval_request_from_client_event` passes `approval.title` raw to `ApprovalRequest::AcpTool`. The overlay prompt ("Would you like to allow edit: Edit /home/.../README.md?") and the decision history cell ("✔ You approved edit: Edit /home/.../README.md this time") both show full absolute paths, while every other cell in the chat uses cwd-relative paths.

## Affected Files

| File | Issue |
|------|-------|
| `chatwidget/event_handlers.rs` | (1) `exec_begin_event_from_client_snapshot` passes raw titles |
| `bottom_pane/approval_overlay.rs` | (3) AcpTool prompt title not relativized; (3) DiffSummary `cwd` is hardcoded `"."` |
| `history_cell/mod.rs` | (3) `new_acp_approval_decision_cell` title not relativized |
| `client_tool_cell.rs` | (2) `render_edit_lines` produces outer header + nested DiffSummary with duplicate content |
| `diff_render.rs` | (2) `render_changes_block` generates its own verb+path header for single files |

## Steps

### Step 1: Sanitize exec-like tool titles

In `exec_begin_event_from_client_snapshot` (event_handlers.rs), apply the existing title cleanup to the `snapshot.title` before using it. Specifically:

- Strip `[current working directory ...]` bracket patterns (Gemini embeds these).
- Strip trailing `(description text)` parenthetical metadata (Gemini appends these).
- Apply `relativize_paths_in_text` to the result.

The cleanest approach is to extract a `sanitize_tool_title(title: &str, cwd: &Path) -> String` function in `client_event_format.rs` that applies these transforms in sequence, then call it anywhere `snapshot.title` is used as a display string in exec-like paths.

The `Invocation::Command` branch already has good handling via `formatted_client_tool_command_text` — this fix targets the Read, Search, ListFiles, Tool, RawJson, and fallback Execute branches that currently do `snapshot.title.clone()` verbatim.

**Tests:** Add a unit test with a Gemini-style title like `"Read README.md [current working directory /home/user/project] (Read the contents of README.md)"` and assert it becomes `"Read README.md"`.

### Step 2: Deduplicate single-file edit header

In `render_edit_lines` (client_tool_cell.rs), when `create_diff_summary` returns content for a single file, suppress the outer `format_edit_tool_header` text and let DiffSummary's built-in header be the only header. The outer bullet (green/red/spinner) is still needed.

Concretely: when the diff changes have exactly 1 file, replace the current rendering:

```
● Edited README.md           ← outer header (format_edit_tool_header)
    ● Edited README.md (+1 -1)   ← DiffSummary's own header
        1 -old line
        1 +new line
```

with:

```
● Edited README.md (+1 -1)   ← DiffSummary header promoted to outer position
      1 -old line
      1 +new line
```

The simplest approach: when diff changes exist and there's exactly 1 file, use the first line from `create_diff_summary` (which contains the verb+path+counts) as the outer header line — prepended with the bullet — and skip the standalone `format_edit_tool_header` call. For multi-file edits, keep the outer header as-is since the `DiffSummary` shows per-file sub-headers.

For failed edits (which show `"Edit failed: path"` + error text but no diff), and for edits with no diff data, the outer `format_edit_tool_header` continues as the sole header — no change needed there.

**Tests:** Update the existing snapshot test `approval_modal_patch_from_client_event` if its output changes. Add a unit test rendering a single-file edit ClientToolCell and asserting the verb+path+counts appear exactly once.

### Step 3: Relativize approval titles and decision cells

Three places need `relativize_paths_in_text`:

a. **Approval overlay prompt** (approval_overlay.rs): In the `set_current` → `build_options_and_prompt` path, the AcpTool prompt string is `format!("Would you like to allow {kind_str}: {title}?")`. Apply `relativize_paths_in_text(&title, &cwd)` before formatting. This requires threading `cwd` into the approval overlay — either pass it when constructing the overlay, or store it from the `ApprovalRequest::AcpTool { snapshot }` which has locations.

b. **Approval overlay DiffSummary cwd** (approval_overlay.rs): The AcpTool edit-like branch uses `PathBuf::from(".")` as `cwd` for the DiffSummary. Change this to use the actual working directory. The `cwd` can be derived from `snapshot.locations[0].path.parent()` or passed through the ApprovalRequest.

c. **Decision history cell** (history_cell/mod.rs): `new_acp_approval_decision_cell(title, kind, decision)` uses `title` raw. Add a `cwd: &Path` parameter and apply `relativize_paths_in_text`.

The most practical approach is to add `cwd: PathBuf` to `ApprovalRequest::AcpTool` (populated from `self.config.cwd` in `handle_client_approval_request`) and propagate it to the overlay and decision cell.

**Tests:** Add a test constructing an AcpTool approval with an absolute path title and asserting the rendered overlay text contains only the relative path.

## Out of Scope — Provider Inconsistencies

The following rendering differences between agent providers are inherent to each provider's ACP implementation and cannot be fixed in the TUI layer. They are documented here for awareness.

### Claude Agent

- **Edits require approval individually.** Claude sends separate `ApprovalRequest` events for each edit, producing one approval overlay per file. Codex agents auto-approve edits in `acceptEdits` mode without protocol-level approval events.
- **Read tools render with code-fence content preview.** Claude's `Read` tool returns file content in artifacts, rendered as ``` blocks. Other providers don't include content in read artifacts.
- **Parallel tool calls complete out of order.** Claude runs tools concurrently and returns results in completion order, not submission order. The TUI renders cells as they arrive, so the visual order may not match the agent's narrative.

### Codex (OpenAI) Agent

- **Edit/Delete/Move uses generic `Edit` tool kind.** Codex does not distinguish between Edit, Delete, and Move at the protocol level — all file mutations arrive as `ToolKind::Edit` with different `FileChange` payloads. The verb in the header ("Edited"/"Deleted"/"Moved") comes from the `FileChange` variant, not the tool kind.
- **Exploring tools show "Read file" without filename.** Codex's `Read` invocations sometimes lack the `path` field, falling back to the generic title. This is an agent-side omission.
- **Shell commands use `rm` for file deletion** instead of the protocol's Delete tool. The TUI shows these as `Ran rm path` execute cells rather than native edit cells with diff output.
- **Exec-like tools sometimes show `Running Reading /path`** with a redundant "Running" prefix on the verb. This is because the in-progress phase prepends "Running" to the title, and Codex titles already start with the verb.

### Gemini Agent

- **Titles embed `[current working directory ...]` metadata.** Gemini appends the cwd path in brackets to every tool title. Spec 08 strips this for ClientToolCell headers, and this spec (step 1) extends the sanitization to exec-like tool paths.
- **File write tool fails with internal error.** Gemini's `write_file` tool sometimes fails and the agent falls back to `echo "content" > file` via shell. This produces an Execute cell instead of an Edit cell. The TUI renders it correctly as a shell command, but the user sees a less polished experience compared to native Edit rendering.
- **No diff artifacts on edits.** Gemini's Edit tool does not emit `Artifact::Diff` entries. The TUI falls back to `changes_from_invocation()` to extract diff data from the `Invocation::FileChanges` payload. This works but means diff rendering depends entirely on invocation data quality.
- **Approval retries.** Gemini sometimes triggers multiple parallel tool calls that all need approval. When the user approves some and rejects others, Gemini retries the rejected ones individually. This produces multiple rounds of approval overlays for what was a single logical action. This is agent-side behavior.
- **Plan updates appear in the transcript.** Gemini sends plan snapshots that render as "Updated Plan" cells with checkboxes. Claude and Codex do not send plan updates through the ACP protocol.
