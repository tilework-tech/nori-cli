# Codex ACP Translation Gap Analysis

## Executive Summary

The Codex agent sends different message formats than Claude Code's ACP adapter. The current translator in `codex-rs/acp/src/translator.rs` was built for Claude Code's message structure and needs to be extended to handle Codex's format.

## Key Differences

### 1. **Tool Call Message Structure**

#### Claude Code (Working)
```json
{
  "sessionUpdate": "tool_call",
  "toolCallId": "toolu_01T8mtJAjBnp32dRTXntviyA",
  "rawInput": {
    "command": "git status",
    "description": "Check git status"
  },
  "status": "pending",
  "title": "`git status`",
  "kind": "execute"
}
```

#### Codex (Not Working)
```json
{
  "sessionUpdate": "tool_call",
  "toolCallId": "call_ForIWpYHApyFSjXKY7RsSLB2",
  "title": "Run git status -sb",
  "kind": "execute",
  "status": "in_progress",
  "rawInput": {
    "call_id": "call_ForIWpYHApyFSjXKY7RsSLB2",
    "command": ["bash", "-lc", "cd /home/clifford/Documents/source/nori/cli && git status -sb"],
    "cwd": "/home/clifford/Documents/source/nori/cli",
    "is_user_shell_command": false,
    "parsed_cmd": [{"cmd": "git status -sb", "type": "unknown"}]
  }
}
```

**Key Differences:**
- Codex uses `command` as an **array** instead of a string
- Codex includes shell wrapper: `["bash", "-lc", "cd ... && ..."]`
- Codex provides `parsed_cmd` array with command metadata
- Codex includes `cwd` in rawInput
- Codex uses `call_id` in rawInput (matches toolCallId)

### 2. **Tool Call Update/Completion Structure**

#### Claude Code
```json
{
  "sessionUpdate": "tool_call_update",
  "toolCallId": "toolu_01T8mtJAjBnp32dRTXntviyA",
  "status": "completed",
  "content": [{"type": "content", "content": {"type": "text", "text": "..."}}]
}
```

#### Codex
```json
{
  "sessionUpdate": "tool_call_update",
  "toolCallId": "call_5CWaocirOrt7cCXzOJKuBgE9",
  "status": "completed",
  "content": [{"type": "content", "content": {"type": "text", "text": "```sh\n...\n```\n"}}],
  "rawOutput": {
    "aggregated_output": "...",
    "call_id": "...",
    "duration": {"nanos": 44265664, "secs": 0},
    "exit_code": 0,
    "formatted_output": "...",
    "stderr": "",
    "stdout": "..."
  }
}
```

**Key Differences:**
- Codex wraps output in markdown code blocks (` ```sh\n...\n``` `)
- Codex provides rich `rawOutput` object with:
  - Separate `stdout` and `stderr` fields
  - `exit_code` for command success/failure
  - `duration` with nanosecond precision
  - Multiple output formats (`aggregated_output`, `formatted_output`)

### 3. **File Edit Operations**

#### Codex File Write
```json
{
  "sessionUpdate": "tool_call",
  "toolCallId": "call_PPGjbNSQmaGncHZMeahE7cSD",
  "title": "Edit /home/clifford/Documents/source/nori/cli/SUMMARY.md",
  "kind": "edit",
  "status": "in_progress",
  "content": [{
    "type": "diff",
    "path": "/home/clifford/Documents/source/nori/cli/SUMMARY.md",
    "oldText": null,
    "newText": "## Repository summary\n..."
  }],
  "locations": [{"path": "/home/clifford/Documents/source/nori/cli/SUMMARY.md"}],
  "rawInput": {
    "auto_approved": true,
    "call_id": "call_PPGjbNSQmaGncHZMeahE7cSD",
    "changes": {
      "/home/clifford/Documents/source/nori/cli/SUMMARY.md": {
        "add": {"content": "## Repository summary\n..."}
      }
    }
  }
}
```

**Key Differences:**
- Codex includes `content` array with `type: "diff"` and inline diff data
- Codex provides `locations` array with file paths
- Codex uses nested `changes` object with operation type (`add`, `update`, `delete`)
- Codex has `auto_approved` flag in rawInput

### 4. **Client Request for File Write**

```json
← {"jsonrpc":"2.0","id":0,"method":"fs/write_text_file","params":{
  "sessionId":"019bbda0-0591-7ae2-9d59-29782b78aabc",
  "path":"/home/clifford/Documents/source/nori/cli/SUMMARY.md",
  "content":"## Repository summary\n..."
}}
→ {"jsonrpc":"2.0","id":0,"result":{}}
```

**This is a bidirectional RPC call** - the agent calls back to the client to perform the file write.

## Missing Translation Support

The current translator (`codex-rs/acp/src/translator.rs`) needs to handle:

### 1. **Command Extraction from Array Format**
Current code expects `command` as a string, but Codex sends an array. Need to:
- Extract the actual command from the shell wrapper
- Parse `parsed_cmd` array for command metadata
- Handle both string and array formats

### 2. **Output Formatting**
Codex wraps output in markdown code blocks. Need to:
- Strip markdown formatting (` ```sh\n...\n``` `) from content
- Use `rawOutput.stdout` and `rawOutput.stderr` separately
- Use `rawOutput.exit_code` for success/failure indication

### 3. **File Edit Diffs**
Codex provides diff information in `content` field with:
- `type: "diff"` indicator
- `oldText` and `newText` for the change
- `locations` array for affected files
Need to:
- Extract diff from `content` array instead of only `rawInput`
- Handle the nested `changes` structure
- Support `auto_approved` flag

### 4. **Tool Call Status Mapping**
Codex uses different status values:
- `in_progress` (vs Claude Code's `pending`)
- Need to map both to appropriate TUI states

## Recommended Implementation Changes

### File: `codex-rs/acp/src/translator.rs`

#### 1. Add Command Array Parsing
```rust
/// Extract command from either string or array format
fn extract_command_string(raw_input: Option<&serde_json::Value>) -> Option<String> {
    raw_input.and_then(|input| {
        // Try array format first (Codex style)
        if let Some(cmd_array) = input.get("command").and_then(|v| v.as_array()) {
            // Check for bash wrapper: ["bash", "-lc", "cd ... && command"]
            if cmd_array.len() == 3 
                && cmd_array[0].as_str() == Some("bash") 
                && cmd_array[1].as_str() == Some("-lc") {
                // Extract the actual command from the shell wrapper
                if let Some(shell_cmd) = cmd_array[2].as_str() {
                    // Parse "cd /path && actual_command" to get "actual_command"
                    return extract_command_from_shell_wrapper(shell_cmd);
                }
            }
            // Fallback: join array elements
            return Some(cmd_array.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(" "));
        }
        
        // Try string format (Claude Code style)
        input.get("command").and_then(|v| v.as_str()).map(String::from)
    })
}

/// Extract actual command from "cd /path && command" format
fn extract_command_from_shell_wrapper(shell_cmd: &str) -> Option<String> {
    // Look for "cd ... && command" pattern
    if let Some(pos) = shell_cmd.find(" && ") {
        Some(shell_cmd[pos + 4..].trim().to_string())
    } else {
        Some(shell_cmd.to_string())
    }
}
```

#### 2. Add Markdown Stripping for Output
```rust
/// Strip markdown code block formatting from output
fn strip_markdown_code_blocks(text: &str) -> String {
    // Remove ```language\n ... \n``` wrapping
    let re = regex::Regex::new(r"```[a-z]*\n(.*?)\n```").unwrap();
    if let Some(captures) = re.captures(text) {
        captures.get(1).map(|m| m.as_str().to_string()).unwrap_or_else(|| text.to_string())
    } else {
        text.to_string()
    }
}
```

#### 3. Enhance `extract_tool_output` to Use rawOutput
```rust
fn extract_tool_output(fields: &acp::ToolCallUpdateFields) -> String {
    // Try rawOutput first (Codex provides structured output here)
    if let Some(raw_output) = &fields.raw_output {
        if let Some(stdout) = raw_output.get("stdout").and_then(|v| v.as_str()) {
            return stdout.to_string();
        }
        if let Some(formatted) = raw_output.get("formatted_output").and_then(|v| v.as_str()) {
            return formatted.to_string();
        }
        if let Some(aggregated) = raw_output.get("aggregated_output").and_then(|v| v.as_str()) {
            return aggregated.to_string();
        }
    }
    
    // Fallback to content field (existing behavior)
    let mut output_parts: Vec<String> = Vec::new();
    if let Some(content) = &fields.content {
        for item in content {
            if let acp::ToolCallContent::Content(c) = item
                && let acp::ContentBlock::Text(text) = &c.content
                && !text.text.is_empty()
            {
                // Strip markdown formatting
                output_parts.push(strip_markdown_code_blocks(&text.text));
            }
        }
    }
    output_parts.join("\n")
}
```

#### 4. Add Support for Diff Content Field
```rust
/// Extract file changes from Codex-style content field
fn extract_file_changes_from_content(
    content: &[acp::ToolCallContent],
) -> Option<HashMap<PathBuf, FileChange>> {
    let mut changes = HashMap::new();
    
    for item in content {
        // Look for diff-type content
        if let Some(obj) = item.as_object() {
            if obj.get("type").and_then(|v| v.as_str()) == Some("diff") {
                let path = obj.get("path").and_then(|v| v.as_str())?;
                let old_text = obj.get("oldText").and_then(|v| v.as_str());
                let new_text = obj.get("newText").and_then(|v| v.as_str())?;
                
                if old_text.is_none() {
                    // New file (add operation)
                    changes.insert(
                        PathBuf::from(path),
                        FileChange::Add { content: new_text.to_string() }
                    );
                } else {
                    // File update (edit operation)
                    let diff = diffy::create_patch(old_text.unwrap(), new_text).to_string();
                    changes.insert(
                        PathBuf::from(path),
                        FileChange::Update { unified_diff: diff, move_path: None }
                    );
                }
            }
        }
    }
    
    if changes.is_empty() { None } else { Some(changes) }
}
```

### File: `codex-rs/acp/src/backend.rs`

#### Update `translate_session_update_to_events`
```rust
fn translate_session_update_to_events(
    update: &acp::SessionUpdate,
    pending_patch_changes: &mut HashMap<String, HashMap<PathBuf, FileChange>>,
) -> Vec<EventMsg> {
    match update {
        acp::SessionUpdate::ToolCall(tool_call) => {
            // Check for file changes in content field (Codex style)
            if let Some(content) = &tool_call.content {
                if let Some(changes) = extract_file_changes_from_content(content) {
                    pending_patch_changes.insert(
                        tool_call.tool_call_id.to_string(),
                        changes
                    );
                    return vec![]; // Don't emit Begin yet, wait for approval
                }
            }
            
            // Existing ToolCall handling...
        }
        // ... rest of match arms
    }
}
```

## Testing Checklist

After implementing these changes, test with Codex:

- [ ] Shell command execution shows correct command (not bash wrapper)
- [ ] Command output displays without markdown code blocks
- [ ] File edits show proper diffs in TUI
- [ ] File writes complete successfully
- [ ] Exit codes are properly captured and displayed
- [ ] Duration information is preserved
- [ ] `parsed_cmd` metadata is used for command classification
- [ ] `auto_approved` flag is respected

## References

- Codex log: `sacp-tee-2026-01-14-codex.log`
- Claude Code log: `sacp-tee-2026-01-14-claude.log`
- Current translator: `codex-rs/acp/src/translator.rs`
- Backend adapter: `codex-rs/acp/src/backend.rs`
