# Spec 08: Gemini Empty Content Fallback

## Summary

When Gemini sends a completed tool call with `content: []` (empty array), the `ClientToolCell` renders with no detail lines — just a bare header. The normalizer should fall back to title and locations to produce a meaningful display, and the TUI should handle the case where a completed tool has no artifacts or invocation.

## Expected Behavior

A completed read with no content should still show the file path:

```
• Explored
  └ Read README.md
```

A completed execute with no content but a meaningful title should show:

```
• Ran echo "This is a temporary file..." > tmp.md
```

A completed tool with no content and no parseable invocation should at minimum show:

```
• Tool completed: README.md (read)
```

...not a bare headerless cell.

## Actual Behavior

From `screen-examples-new/screen-capture-gemini.log:20-21`:

```
• Tool [completed]: README.md (read)
```

This is the entire cell for a completed read — no sub-items, no path detail, no output. Compare to Claude's rendering of the same operation which at least shows the path and content.

For Gemini's shell commands, the rendering shows the full approval text as a title but with no output:

`screen-examples-new/screen-capture-gemini.log:43-45`:
```
• Tool [completed]: echo "This is a temporary file for testing." > tmp.md [current working
directory /home/clifford/Documents/source/nori/cli/.worktrees/acp-event-model-refactor] (Create
a temporary file using echo as write_file failed.) (execute)
```

The `[current working directory ...]` and `(Create a temporary file...)` are part of the raw title from Gemini — they should not appear in the display.

## Wire Protocol Evidence

Gemini's completed read — `screen-examples-new/debug-acp-gemini.log:13-14`:

```json
Line 13 (tool_call): {
  "sessionUpdate": "tool_call",
  "toolCallId": "read_file-1774849791482",
  "status": "in_progress",
  "title": "README.md",
  "content": [],
  "locations": [{"path": "/home/.../README.md"}],
  "kind": "read"
}
Line 14 (tool_call_update): {
  "sessionUpdate": "tool_call_update",
  "toolCallId": "read_file-1774849791482",
  "status": "completed",
  "content": []
}
```

Key observations:
- The initial `tool_call` has `content: []` and `locations` with the path
- The completed `tool_call_update` also has `content: []` — no artifacts
- There is no `rawInput` or `rawOutput` provided
- The `title` is just `"README.md"` (short, not a full path)

Gemini's shell commands — `screen-examples-new/debug-acp-gemini.log` (lines for execute tool calls):

The Gemini ACP agent embeds the cwd and description directly in the title string:
```
"title": "echo \"This is a temporary file for testing.\" > tmp.md [current working directory /home/...] (Create a temporary file using echo as write_file failed.)"
```

This polluted title propagates directly to the display.

## Affected Code

- **`nori-protocol/src/lib.rs:250-261`** — `push_session_update` for `ToolCall`: when Gemini sends `content: []` and no `rawInput`, the resulting `ToolSnapshot` has `invocation: None` and `artifacts: []`
- **`nori-protocol/src/lib.rs:396-418`** — `invocation_from_tool_call`: returns `None` when there are no diff artifacts and `raw_input` is `None`
- **`tui/src/client_tool_cell.rs:88-102`** — `render_lines`: when `invocation` is `None` and `artifacts` is empty, no detail lines are produced

## Scope

1. **Fallback invocation from locations**: In `invocation_from_tool_call`, when `raw_input` is `None` but `locations` is non-empty, synthesize an invocation based on the tool kind:
   - `Read` + location → `Invocation::Read { path: locations[0].path }`
   - `Edit` + location → `Invocation::FileOperations` with path from location
   - `Search` + location → `Invocation::Search { path: Some(locations[0].path), query: None }`

2. **Title sanitization for Gemini**: Detect and strip the `[current working directory ...]` suffix and `(description text)` suffix from titles. These patterns are Gemini-specific metadata that shouldn't appear in display. A simple approach: strip everything after the first `[` or after a trailing `(...)` when the kind is `execute`.

3. **Minimal completed cell**: When a completed tool cell would render with zero detail lines (no invocation, no artifacts), show at least the locations as sub-items:
   ```
   • Read completed: README.md
     └ /path/to/README.md
   ```
   Or if even locations are empty, show just the title without the redundant `(kind)` suffix.
