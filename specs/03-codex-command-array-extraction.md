# Spec 03: Codex Command Array Extraction in Normalizer

## Summary

Fix `structured_invocation_from_tool_call` to handle Codex's `rawInput.command` field when it is a JSON array (e.g., `["/usr/bin/zsh", "-lc", "actual command"]`) instead of a plain string. Currently this falls through to `Invocation::RawJson`, causing raw JSON to render in the TUI.

## Expected Behavior

When Codex sends an execute tool call with `rawInput.command` as an array, the normalizer should extract the human-readable command string (the last element after the shell invocation prefix) and produce `Invocation::Command { command: "df -h ." }`.

For Codex read tool calls with `rawInput.parsed_cmd`, the normalizer should recognize the parsed command type and produce the appropriate `Invocation::Read`/`Invocation::Search`/`Invocation::ListFiles`.

## Actual Behavior

From `screen-examples-new/screen-capture-codex.log:86-94`:

```
• Tool [completed]: Run df -h . (execute)
  └ Input: {"call_id":"call_V1aQXvSI1XSNFFXtMCNZZHNT","command":["/usr/
bin/zsh","-lc","df -h ."],"cwd":"/home/clifford/Documents/source/nori/
cli/.worktrees/acp-event-model-refactor","parsed_cmd":[{"cmd":"df
-h .","type":"unknown"}],"process_id":"78060","source":"unified_exec_startup","turn_id":"1"}
```

The raw JSON of the entire `rawInput` object is displayed as the invocation detail.

## Wire Protocol Evidence

Codex execute tool calls — `screen-examples-new/debug-acp-codex.log:90`:

```json
{
  "sessionUpdate": "tool_call",
  "toolCallId": "call_6CPQbgZ4wlkISOXdP50Ayork",
  "title": "Read SKILL.md",
  "kind": "read",
  "status": "in_progress",
  "rawInput": {
    "call_id": "call_6CPQbgZ4wlkISOXdP50Ayork",
    "command": ["/usr/bin/zsh", "-lc", "sed -n '1,220p' /home/clifford/.codex/skills/using-skills/SKILL.md"],
    "cwd": "/home/clifford/Documents/source/nori/cli/.worktrees/acp-event-model-refactor",
    "parsed_cmd": [{"cmd": "sed -n '1,220p' ...", "name": "SKILL.md", "path": "/home/clifford/.codex/skills/using-skills/SKILL.md", "type": "read"}],
    "process_id": "12510",
    "source": "unified_exec_startup",
    "turn_id": "1"
  }
}
```

Key structural differences from Claude:
- `rawInput.command` is `["/usr/bin/zsh", "-lc", "actual command"]` (array), not a string
- `rawInput.parsed_cmd` is an array of `{cmd, type, name?, path?}` objects from the Codex backend's command parser
- `rawInput.cwd`, `rawInput.call_id`, `rawInput.process_id`, `rawInput.source`, `rawInput.turn_id` are extra metadata not present in Claude

## Root Cause

`nori-protocol/src/lib.rs:467-474`:

```rust
acp::ToolKind::Execute => {
    let command = raw_input
        .get("command")
        .or_else(|| raw_input.get("cmd"))
        .and_then(serde_json::Value::as_str)?;  // <-- returns None for arrays
    Some(Invocation::Command {
        command: command.to_string(),
    })
}
```

`as_str()` returns `None` when the value is a JSON array, so the entire `Execute` branch returns `None`. Control falls to `lib.rs:418`: `tool_call.raw_input.clone().map(Invocation::RawJson)`.

Similarly for `Read` (`lib.rs:476-484`), Codex doesn't have `path`/`file_path`/`file` keys at the top level — the path is nested inside `parsed_cmd[0].path`.

## Scope

In `structured_invocation_from_tool_call` (`nori-protocol/src/lib.rs`):

1. **Execute**: After trying `as_str()`, try `as_array()` on the `command` field. If it's an array like `[shell, "-lc", cmd]` or `[shell, "-c", cmd]`, extract the last element as the command string. Also try `parsed_cmd[0].cmd` as a fallback.

2. **Read**: After the existing path extraction fails, try `parsed_cmd[0].path` (when `parsed_cmd[0].type == "read"`).

3. **Search**: After the existing extraction fails, try `parsed_cmd[0]` fields (when `type` is `"list_files"` or contains search indicators).

4. Add unit tests with Codex-shaped `rawInput` payloads to verify extraction works for all three kinds.
