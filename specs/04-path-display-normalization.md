# Spec 04: Path Display Normalization in Tool Cells

## Summary

Relativize absolute paths in tool snapshot titles, invocation descriptions, and sub-item labels to the working directory or home directory, matching the compact display used by the old rendering path.

## Expected Behavior (old rendering)

From `screen-examples-old/debug-acp-claude-screen.log:28-34`:

```
• Edited README.md (+1 -1)
    1 -# Nori CLI
    1 +# Nori CLI (test edit)

• Added tmp.md (+1 -0)
    1 +# Temporary file for testing
```

From `screen-examples-old/debug-acp-claude-screen.log:25-26`:

```
• Explored
  └ Read README.md
```

Paths are shown as:
- `README.md` (relative to cwd)
- `tmp.md` (relative to cwd)
- `~/.claude/CLAUDE.md` (relative to home when outside cwd)

## Actual Behavior (new rendering)

From `screen-examples-new/screen-capture-claude.log:22-25`:

```
• Tool [completed]: Read /home/clifford/Documents/source/nori/cli/.worktrees/acp-event-model-
refactor/README.md (1 - 5) (read)
  └ Read: /home/clifford/Documents/source/nori/cli/.worktrees/acp-event-model-refactor/
README.md
```

From `screen-examples-new/screen-capture-codex.log:86-89`:

```
• Tool [completed]: Run df -h . (execute)
  └ Input: {"call_id":"...","command":["/usr/bin/zsh","-lc","df -h ."],"cwd":"/home/clifford/Documents/source/nori/
cli/.worktrees/acp-event-model-refactor",...}
```

Problems:
1. Full absolute paths in tool titles (`/home/clifford/Documents/source/nori/cli/.worktrees/acp-event-model-refactor/README.md`)
2. Full absolute paths in invocation descriptions (`Read: /home/...`)
3. Full absolute paths in `ToolLocation` entries
4. These long paths wrap across multiple terminal lines, wasting vertical space

## Wire Protocol Evidence

The ACP protocol sends absolute paths in both `title` and `locations`:

`screen-examples-new/debug-acp-claude.log:21`:
```json
{
  "title": "Read /home/clifford/Documents/source/nori/cli/.worktrees/acp-event-model-refactor/README.md (1 - 5)",
  "kind": "read",
  "locations": [{"path": "/home/clifford/Documents/source/nori/cli/.worktrees/acp-event-model-refactor/README.md", "line": 1}]
}
```

The path normalization must happen at the TUI display layer, not in the protocol normalizer, because the `cwd` is a TUI-level configuration value.

## Affected Code

- **`tui/src/client_event_format.rs:63-89`** — `format_invocation` renders `Read: {path.display()}` and `Command: {command}` without normalization
- **`tui/src/client_event_format.rs:36-43`** — `format_tool_header` uses `snapshot.title` verbatim
- **`tui/src/client_tool_cell.rs:84`** — `format_tool_header(&self.snapshot)` passed directly to display
- **`tui/src/exec_command.rs`** — has existing `relativize_to_home` utility
- **`tui/src/diff_render.rs:237-243`** — has existing `display_path_for(path, cwd)` that does cwd-relative display

## Scope

1. Thread `cwd: &Path` into `ClientToolCell` (either at construction or via a config reference)
2. Apply path relativization in:
   - `format_tool_header`: strip the cwd prefix from `snapshot.title` (or regex-replace absolute paths within the title string)
   - `format_invocation`: relativize `path` in `Read`, `Search`, `ListFiles`, `FileChanges`, `FileOperations`
   - Sub-item rendering for exploring cells
3. Reuse the existing `display_path_for(path, cwd)` from `diff_render.rs` or `relativize_to_home` from `exec_command.rs`
4. Paths inside cwd → show relative (`README.md`); paths outside cwd but inside home → show `~/...`; paths outside home → show absolute
