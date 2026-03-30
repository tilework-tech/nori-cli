# Spec 02: Exploring Cell Grouping for Read/Search/List Snapshots

## Summary

Group consecutive `ToolKind::Read`, `ToolKind::Search`, and `Invocation::ListFiles` snapshots into a single composite "Explored" cell with compact sub-items, matching the old ExecCell exploring mode.

## Expected Behavior (old rendering)

From `screen-examples-old/debug-acp-claude-screen.log:25-26`:

```
• Explored
  └ Read README.md
```

From `screen-examples-old/debug-acp-codex-screen.log:28-30`:

```
• Explored
  └ Read file
    Search List /home/clifford/Documents/source/nori/cli
```

Key design elements:
- **Single composite cell**: multiple consecutive reads/searches are merged into one "Explored" cell
- **Active state**: `Exploring` (bold) with spinner while any sub-call is pending
- **Completed state**: `Explored` (bold) with dim `•`
- **Sub-items grouped by type**: `Read filename1, filename2` (reads on one line, comma-separated), `Search query in path`, `List path`
- **Consecutive read merging**: adjacent read-only calls are merged so `Read a.rs, b.rs, c.rs` appears as a single sub-item
- **Tree prefix**: `└` for first sub-item, spaces for subsequent (via `prefix_lines`)

## Actual Behavior (new rendering)

From `screen-examples-new/screen-capture-claude.log:22-37`:

```
• Tool [completed]: Read /home/clifford/Documents/source/nori/cli/.worktrees/acp-event-model-
refactor/README.md (1 - 5) (read)
  └ Read: /home/clifford/Documents/source/nori/cli/.worktrees/acp-event-model-refactor/
README.md
    Output:
    ```
         1→# Nori CLI
         2→
         3→[![CI](...)]
         ...
    ```
```

From `screen-examples-new/screen-capture-gemini.log:20-21`:

```
• Tool [completed]: README.md (read)
```

Problems:
1. Each read is a separate cell — no grouping into "Explored"
2. Verbose full absolute paths instead of short filenames
3. Full file content dumped as output — the old rendering intentionally omitted read output in the history cell
4. No `Exploring`/`Explored` verb — uses generic `Tool [status]:` header
5. Gemini's read has no detail lines at all (empty content on completed)

## Wire Protocol Evidence

Claude emits separate `tool_call` + `tool_call_update` pairs for each read. They arrive in sequence and can be grouped by the TUI:

`screen-examples-new/debug-acp-claude.log:18-23` — Read tool lifecycle:
```
Line 18: tool_call (status: pending, title: "Read File", kind: read)
Line 21: tool_call_update (title: "Read .../README.md (1 - 5)", kind: read, locations: [{path: ".../README.md"}])
Line 23: tool_call_update (status: completed, content: [{text: "```\n  1→# Nori CLI\n...```"}])
```

Gemini emits minimal read events:

`screen-examples-new/debug-acp-gemini.log:13-14`:
```
Line 13: tool_call (status: in_progress, title: "README.md", kind: read, locations: [{path: ".../README.md"}])
Line 14: tool_call_update (status: completed, content: [])
```

## Affected Code

- **`tui/src/chatwidget/event_handlers.rs:1338-1381`** — `handle_client_native_tool_snapshot` creates a new `ClientToolCell` per snapshot; needs to merge into an existing exploring cell when the previous active cell is also exploring
- **`tui/src/client_tool_cell.rs`** — `ClientToolCell` holds a single `ToolSnapshot`; needs to hold multiple snapshots for exploring mode (similar to `ExecCell` holding a `Vec<ExecCall>`)
- **`tui/src/client_event_format.rs:53-61`** — `is_exploring_snapshot` exists but is only used to decide whether to flush; it doesn't trigger grouping
- **`tui/src/exec_cell/render.rs:244-346`** — `exploring_display_lines()` has the full grouping/rendering logic to port

## Scope

- Extend `ClientToolCell` to hold a `Vec<ToolSnapshot>` (or a parallel list of sub-items) for exploring mode
- In `handle_client_native_tool_snapshot`, when the active cell is an exploring `ClientToolCell` and the new snapshot is also exploring, merge the new snapshot into the existing cell instead of creating a new one
- Port the sub-item rendering from `ExecCell::exploring_display_lines`: group reads by filename, show `Read`, `Search`, `List` labels with compact arguments
- Omit read output content from the exploring display (reads are informational; content is noise in history)
- Show `Exploring` with spinner while any sub-snapshot is active; `Explored` when all are completed
