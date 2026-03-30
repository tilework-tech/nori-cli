# Spec 06: Artifact Text Output Cleanup

## Summary

Clean up the text content rendered from `Artifact::Text` entries: strip markdown code fence markers, handle empty output with a `(no output)` label, and remove the redundant `Output:` prefix when the output is a single short line.

## Expected Behavior (old rendering)

From `screen-examples-old/debug-acp-claude-screen.log:39-50`:

```
• Ran date --utc +"%Y-%m-%d %H:%M:%S"
  └ 2026-03-24 23:47:15

• Ran rm /home/clifford/Documents/source/nori/cli/tmp.md
  └ (no output)
```

- Single-line output is shown directly after `└`, no label
- Multi-line output is shown under `└` with continuation indent
- Empty output shows `(no output)` in dim text
- No code fence markers

## Actual Behavior (new rendering)

From `screen-examples-new/screen-capture-claude.log:72-78`:

```
• Tool [completed]: date --utc +"%Y-%m-%d %H:%M:%S %Z" (execute)
  └ Command: date --utc +"%Y-%m-%d %H:%M:%S %Z"
    Output:
    ```console
    2026-03-30 05:45:34 UTC
    ```
```

From `screen-examples-new/screen-capture-claude.log:101-105`:

```
• Tool [completed]: rm .../tmp.md (execute)
  └ Command: rm .../tmp.md
    Output: Delete the temporary tmp.md file
```

Problems:
1. Code fence markers (```` ```console ```` and ```` ``` ````) appear literally in the display — they come from the ACP `content` field where Claude wraps output in markdown code blocks
2. `Output:` prefix is always shown, even for short single-line results where the output could go directly on the detail line
3. No `(no output)` label when stdout is empty (the `rm` example shows the `description` as output instead — "Delete the temporary tmp.md file" is Claude's tool description, not actual command output)
4. The `Command:` invocation line redundantly repeats the command already shown in the header

## Wire Protocol Evidence

Claude wraps shell output in code fences within the `content` array:

`screen-examples-new/debug-acp-claude.log:55`:
```json
{
  "status": "completed",
  "rawOutput": "2026-03-30 05:45:34 UTC",
  "content": [{"type": "content", "content": {"type": "text", "text": "```console\n2026-03-30 05:45:34 UTC\n```"}}]
}
```

Note that `rawOutput` has the clean text, while `content[].text` has the fenced version. The normalizer currently uses `content` (via `artifacts_from_tool_call`) first, falling back to `rawOutput` only when no text artifacts exist.

For the `rm` command with empty stdout:

`screen-examples-new/debug-acp-claude.log:70-71`:
```json
Line 70: {"_meta": {"claudeCode": {"toolResponse": {"stdout": "", "stderr": "", "noOutputExpected": true}}}}
Line 71: {"status": "completed", "rawOutput": ""}
```

No `content` array is provided (or it's empty), and `rawOutput` is empty string. The current rendering silently omits the output section, but doesn't show `(no output)`.

## Affected Code

- **`nori-protocol/src/lib.rs:421-449`** — `artifacts_from_tool_call` prefers `content` text (fenced) over `rawOutput` (clean). The priority should be reversed for execute tools, or the fences should be stripped.
- **`tui/src/client_event_format.rs:92-104`** — `format_artifacts` adds `Output:` prefix and passes text through unmodified
- **`tui/src/client_tool_cell.rs:88-97`** — artifact detail lines rendered without any post-processing

## Scope

1. **Strip code fences**: In `format_artifacts` (or in the normalizer), detect and remove leading/trailing ```` ```lang ```` and ```` ``` ```` lines from text artifacts. A simple heuristic: if the first line matches `^```\w*$` and the last line matches `^```$`, strip both.

2. **Prefer rawOutput for execute tools**: In `artifacts_from_tool_call`, when the tool kind is `Execute` and `rawOutput` has clean text, prefer it over fenced `content` text. Alternatively, always strip fences in post-processing.

3. **Handle empty output**: When an execute tool completes with no text artifacts (or empty text), render `(no output)` in dim text as a detail line.

4. **Remove redundant Command line**: When spec 01 (execute native rendering) is implemented, the command goes in the header (`Ran <command>`), making the `Command:` invocation line redundant. Until then, if the `Invocation::Command` text equals the title text, omit the invocation detail line.

5. **Simplify single-line output**: When the output is a single short line, render it directly on the first detail line (after `└`) without an `Output:` prefix. Use the `Output:` prefix only for multi-line output blocks.
