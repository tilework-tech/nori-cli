# Spec 01: Native Rendering for Execute Tool Calls

## Summary

Replace the generic `Tool [status]: title (execute)` rendering for `ToolKind::Execute` snapshots with ExecCell-parity rendering: semantic verbs, bash syntax highlighting, exit-code bullet coloring, output formatting with truncation, duration display, and `(no output)` handling.

## Expected Behavior (old rendering)

From `screen-examples-old/debug-acp-claude-screen.log:39-51`:

```
• Ran date --utc +"%Y-%m-%d %H:%M:%S"
  └ 2026-03-24 23:47:15

• Ran df -h --type=ext4
  └ Filesystem             Size  Used Avail Use% Mounted on
    /dev/mapper/data-root  1.8T  999G  733G  58% /

• Ran uptime -p
  └ up 1 day, 20 hours, 54 minutes

• Ran rm /home/clifford/Documents/source/nori/cli/tmp.md
  └ (no output)
```

Key design elements:
- **Verb**: `Ran` (completed), `Running` (in-progress), `You ran` (user shell)
- **Bash highlighting**: command text is syntax-highlighted via `highlight_bash_to_lines`
- **Exit-code bullet**: green `•` on success (`exit_code == 0`), red `•` on failure, spinner while running
- **Output**: directly under `└` prefix, word-wrapped to terminal width
- **Empty output**: shows `(no output)` in dim text
- **Truncation**: middle-truncation with `…(N lines omitted)` for long output (max 5 lines for agent, 50 for user shell)
- **Duration**: not shown in the old ACP path but present in the legacy ExecCell (`(1s)`)

## Actual Behavior (new rendering)

From `screen-examples-new/screen-capture-claude.log:72-106`:

```
• Tool [completed]: date --utc +"%Y-%m-%d %H:%M:%S %Z" (execute)
  └ Command: date --utc +"%Y-%m-%d %H:%M:%S %Z"
    Output:
    ```console
    2026-03-30 05:45:34 UTC
    ```

• Tool [completed]: uptime -p (execute)
  └ Command: uptime -p
    Output:
    ```console
    up 1 week, 2 hours, 53 minutes
    ```

• Tool [completed]: rm .../tmp.md (execute)
  └ Command: rm .../tmp.md
    Output: Delete the temporary tmp.md file
```

Problems:
1. Generic `Tool [completed]:` header instead of semantic `Ran`
2. No bash syntax highlighting on the command
3. Dim `•` bullet for all states — no green/red exit code coloring
4. Redundant `Command:` line repeating the title
5. Code fence markers (```` ```console ````) rendered literally in the output
6. No `(no output)` for empty shell results
7. No output truncation logic
8. No duration display
9. No word-wrapping of output lines

## Wire Protocol Evidence

Claude's execute tool calls provide the command string in both `title` and `rawInput.command` (a string):

`screen-examples-new/debug-acp-claude.log:52`:
```json
{
  "toolCallId": "toolu_016FWsmp1M6pxaBHpqHkWEFc",
  "sessionUpdate": "tool_call_update",
  "rawInput": {"command": "date --utc +\"%Y-%m-%d %H:%M:%S %Z\"", "description": "Print current date in UTC format"},
  "title": "date --utc +\"%Y-%m-%d %H:%M:%S %Z\"",
  "kind": "execute",
  "content": [{"type": "content", "content": {"type": "text", "text": "Print current date in UTC format"}}]
}
```

The `rawOutput` (in `_meta.claudeCode.toolResponse`) carries `stdout`, `stderr`, `interrupted`, and `noOutputExpected`:

`screen-examples-new/debug-acp-claude.log:54`:
```json
{"_meta": {"claudeCode": {"toolResponse": {"stdout": "2026-03-30 05:45:34 UTC", "stderr": "", "interrupted": false, "isImage": false, "noOutputExpected": false}}}}
```

## Affected Code

- **`tui/src/client_tool_cell.rs:74-105`** — `render_lines()` uses generic `format_tool_header` for all kinds
- **`tui/src/client_event_format.rs:36-43`** — `format_tool_header` produces `Tool [status]: title (kind)`
- **`tui/src/exec_cell/render.rs:348-482`** — `command_display_lines()` has the full ExecCell rendering logic to reuse or port

## Scope

- Dispatch `ToolKind::Execute` snapshots in `ClientToolCell::render_lines()` to a dedicated method
- Port the verb logic (`Ran`/`Running`), bash highlighting, exit-code bullet coloring, output rendering (with `└` prefix, truncation, `(no output)`), and word-wrapping from `ExecCell::command_display_lines`
- Extract the command string from `Invocation::Command { command }` for the header; fall back to `snapshot.title`
- Extract output text from `artifacts` (strip code fences) or from `raw_output.stdout`
- Infer exit code from `snapshot.phase` (Completed → success, Failed → failure) or from `raw_output` if available
