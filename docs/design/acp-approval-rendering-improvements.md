# ACP Approval Request Rendering Improvements

## Problem Statement

Currently, ACP tool approval requests render poorly in the TUI:

**Before approval (current):**
```
Would you like to run the following command?

Reason: ACP agent requests permission to use: unknown tool

$ toolu_01Hmtbur4ZGyevLqpoSvnfrk
  "{\"file_path\":\"/home/user/project/src/file.rs\",\"old_string\":\"...\",\"new_string\":\"...\"}"
```

**After approval (current):**
```
• Ran Edit
  └ (no output)
```

The issues are:
1. Tool ID shown instead of meaningful command name
2. Raw JSON parameters displayed instead of human-readable format
3. Reason shows "unknown tool" instead of descriptive context
4. No diff preview for edit operations
5. Post-approval message lacks file path and change statistics

## Design Constraint

**All changes must be contained within `codex-rs/acp/`**. No modifications to:
- `codex-rs/protocol/` types (e.g., `ExecApprovalRequestEvent`)
- `codex-rs/tui/` types (e.g., `ApprovalRequest` enum variants)

The ACP module must format data to fit existing TUI expectations.

## Proposed Solution

### Target Experience

**Before approval (proposed):**
```
Would you like to run the following command?

Reason: Edit src/chatwidget.rs: replace 5 lines with 6 lines

$ Edit src/chatwidget.rs
  --- old (5 lines)
  } else {
      // Cell is fully completed - flush it to history immediately.
      self.flush_active_cell();
  }
  +++ new (6 lines)
  } else {
      // Cell is fully completed - clear separator flag before flushing
      self.needs_final_message_separator = false;
      self.flush_active_cell();
  }
```

**After approval (proposed):**
```
• Edited src/chatwidget.rs (+6 -5)
```

### Architecture: ACP Module Formatting

The existing TUI renders approval requests using:
- `command: Vec<String>` → displayed as shell command with `$ ` prefix
- `reason: Option<String>` → displayed as "Reason: {reason}"
- `aggregated_output: String` → displayed under the command after execution

The solution is to have the ACP module populate these fields with well-formatted content.

#### 1. Enhanced Command Extraction (`translator.rs`)

Replace the current `extract_command_from_tool_call` function:

```rust
// CURRENT (produces raw JSON):
fn extract_command_from_tool_call(tool_call: &acp::ToolCallUpdate) -> Vec<String> {
    let mut cmd = Vec::new();
    if let Some(title) = &tool_call.fields.title {
        cmd.push(title.to_string());
    } else {
        cmd.push(tool_call.tool_call_id.to_string());
    }
    if let Some(input) = &tool_call.fields.raw_input {
        cmd.push(serde_json::to_string(input).unwrap_or_default());
    }
    cmd
}

// PROPOSED (produces readable command + formatted preview):
fn extract_command_from_tool_call(tool_call: &acp::ToolCallUpdate) -> Vec<String> {
    let title = tool_call.fields.title.as_deref().unwrap_or("Tool");
    let kind = tool_call.fields.kind.as_ref();
    let raw_input = tool_call.fields.raw_input.as_ref();

    match kind {
        Some(acp::ToolKind::Edit) => format_edit_command(title, raw_input),
        Some(acp::ToolKind::Write) => format_write_command(title, raw_input),
        Some(acp::ToolKind::Delete) => format_delete_command(title, raw_input),
        Some(acp::ToolKind::Execute) => format_execute_command(title, raw_input),
        _ => format_generic_command(title, raw_input),
    }
}

fn format_edit_command(title: &str, raw_input: Option<&serde_json::Value>) -> Vec<String> {
    let Some(input) = raw_input else {
        return vec![title.to_string()];
    };

    let file_path = input
        .get("file_path")
        .or_else(|| input.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or("file");

    let old_string = input.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
    let new_string = input.get("new_string").and_then(|v| v.as_str()).unwrap_or("");

    let short_path = shorten_path(file_path);
    let old_lines = old_string.lines().count();
    let new_lines = new_string.lines().count();

    // Build a readable diff preview
    let mut preview = String::new();
    preview.push_str(&format!("--- old ({} lines)\n", old_lines));
    for line in old_string.lines().take(10) {
        preview.push_str(line);
        preview.push('\n');
    }
    if old_lines > 10 {
        preview.push_str(&format!("... ({} more lines)\n", old_lines - 10));
    }
    preview.push_str(&format!("+++ new ({} lines)\n", new_lines));
    for line in new_string.lines().take(10) {
        preview.push_str(line);
        preview.push('\n');
    }
    if new_lines > 10 {
        preview.push_str(&format!("... ({} more lines)\n", new_lines - 10));
    }

    vec![format!("Edit {}", short_path), preview]
}

fn format_write_command(title: &str, raw_input: Option<&serde_json::Value>) -> Vec<String> {
    let file_path = raw_input
        .and_then(|i| i.get("path").or_else(|| i.get("file_path")))
        .and_then(|v| v.as_str())
        .unwrap_or("file");

    let content = raw_input
        .and_then(|i| i.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let line_count = content.lines().count();
    let preview = content.lines().take(5).collect::<Vec<_>>().join("\n");

    let mut result = vec![format!("Write {} ({} lines)", shorten_path(file_path), line_count)];
    if !preview.is_empty() {
        result.push(format!("{}\n...", preview));
    }
    result
}

fn format_execute_command(title: &str, raw_input: Option<&serde_json::Value>) -> Vec<String> {
    let command = raw_input
        .and_then(|i| i.get("command").or_else(|| i.get("cmd")))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if command.is_empty() {
        vec![title.to_string()]
    } else {
        vec![command.to_string()]
    }
}

fn format_delete_command(title: &str, raw_input: Option<&serde_json::Value>) -> Vec<String> {
    let file_path = raw_input
        .and_then(|i| i.get("path").or_else(|| i.get("file_path")))
        .and_then(|v| v.as_str())
        .unwrap_or("file");

    vec![format!("Delete {}", shorten_path(file_path))]
}

fn format_generic_command(title: &str, raw_input: Option<&serde_json::Value>) -> Vec<String> {
    // Fall back to existing format_tool_call_command logic
    vec![format_tool_call_command(title, raw_input)]
}
```

#### 2. Enhanced Reason Extraction (`translator.rs`)

Replace the current `extract_reason_from_tool_call` function:

```rust
// CURRENT (generic message):
fn extract_reason_from_tool_call(tool_call: &acp::ToolCallUpdate) -> Option<String> {
    let name = tool_call.fields.title.as_deref().unwrap_or("unknown tool");
    Some(format!("ACP agent requests permission to use: {name}"))
}

// PROPOSED (descriptive reason based on tool type):
fn extract_reason_from_tool_call(tool_call: &acp::ToolCallUpdate) -> Option<String> {
    let title = tool_call.fields.title.as_deref().unwrap_or("tool");
    let kind = tool_call.fields.kind.as_ref();
    let raw_input = tool_call.fields.raw_input.as_ref();

    let reason = match kind {
        Some(acp::ToolKind::Edit) => {
            let file_path = extract_file_path(raw_input).unwrap_or("file".to_string());
            let (old_lines, new_lines) = count_edit_lines(raw_input);
            format!("Edit {}: replace {} lines with {} lines",
                shorten_path(&file_path), old_lines, new_lines)
        }
        Some(acp::ToolKind::Write) => {
            let file_path = extract_file_path(raw_input).unwrap_or("file".to_string());
            let line_count = count_content_lines(raw_input);
            format!("Write {} ({} lines)", shorten_path(&file_path), line_count)
        }
        Some(acp::ToolKind::Delete) => {
            let file_path = extract_file_path(raw_input).unwrap_or("file".to_string());
            format!("Delete {}", shorten_path(&file_path))
        }
        Some(acp::ToolKind::Move) => {
            let from = raw_input.and_then(|i| i.get("from")).and_then(|v| v.as_str());
            let to = raw_input.and_then(|i| i.get("to")).and_then(|v| v.as_str());
            match (from, to) {
                (Some(f), Some(t)) => format!("Move {} → {}", shorten_path(f), shorten_path(t)),
                _ => format!("{} requests file move", title),
            }
        }
        Some(acp::ToolKind::Execute) => {
            let cmd = raw_input
                .and_then(|i| i.get("command"))
                .and_then(|v| v.as_str())
                .unwrap_or("command");
            format!("Execute: {}", truncate_str(cmd, 60))
        }
        _ => format!("ACP agent requests permission to use: {}", title),
    };

    Some(reason)
}

// Helper functions
fn extract_file_path(raw_input: Option<&serde_json::Value>) -> Option<String> {
    raw_input
        .and_then(|i| {
            i.get("file_path")
                .or_else(|| i.get("path"))
                .or_else(|| i.get("file"))
        })
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn count_edit_lines(raw_input: Option<&serde_json::Value>) -> (usize, usize) {
    raw_input
        .map(|input| {
            let old = input.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
            let new = input.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
            (old.lines().count().max(1), new.lines().count().max(1))
        })
        .unwrap_or((0, 0))
}

fn count_content_lines(raw_input: Option<&serde_json::Value>) -> usize {
    raw_input
        .and_then(|i| i.get("content"))
        .and_then(|v| v.as_str())
        .map(|c| c.lines().count())
        .unwrap_or(0)
}

fn shorten_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
```

#### 3. Enhanced Post-Approval Display (`backend.rs`)

Modify the `translate_session_update_to_events` function to format the command field
in `ExecCommandEnd` events based on tool type:

```rust
acp::SessionUpdate::ToolCallUpdate(update) => {
    if update.fields.status == Some(acp::ToolCallStatus::Completed) {
        let title = update.fields.title.clone().unwrap_or_default();
        let kind = update.fields.kind.as_ref();
        let raw_input = update.fields.raw_input.as_ref();

        // Format command for post-approval display
        let command = format_completed_tool_command(&title, kind, raw_input);

        // Extract output with enhanced formatting
        let aggregated_output = extract_tool_output_enhanced(&update.fields);

        let parsed_cmd = classify_tool_to_parsed_command(&title, kind, raw_input);

        vec![EventMsg::ExecCommandEnd(
            codex_protocol::protocol::ExecCommandEndEvent {
                call_id: update.tool_call_id.to_string(),
                process_id: None,
                turn_id: String::new(),
                command: vec![command],
                cwd: PathBuf::new(),
                parsed_cmd,
                source: codex_protocol::protocol::ExecCommandSource::Agent,
                interaction_input: None,
                stdout: String::new(),
                stderr: String::new(),
                aggregated_output,
                exit_code: 0,
                duration: std::time::Duration::ZERO,
                formatted_output: String::new(),
            },
        )]
    } else {
        vec![]
    }
}

/// Format the command string for post-approval display (e.g., "Edited file.rs (+6 -5)")
fn format_completed_tool_command(
    title: &str,
    kind: Option<&acp::ToolKind>,
    raw_input: Option<&serde_json::Value>,
) -> String {
    match kind {
        Some(acp::ToolKind::Edit) => {
            let file_path = extract_file_path(raw_input).unwrap_or_else(|| "file".to_string());
            let (old_lines, new_lines) = count_edit_lines(raw_input);
            let added = new_lines.saturating_sub(old_lines);
            let removed = old_lines.saturating_sub(new_lines);

            // Calculate actual diff stats using line comparison
            let (actual_added, actual_removed) = calculate_diff_stats(raw_input);

            format!("Edited {} (+{} -{})",
                shorten_path(&file_path),
                actual_added.max(added),
                actual_removed.max(removed))
        }
        Some(acp::ToolKind::Write) => {
            let file_path = extract_file_path(raw_input).unwrap_or_else(|| "file".to_string());
            format!("Wrote {}", shorten_path(&file_path))
        }
        Some(acp::ToolKind::Delete) => {
            let file_path = extract_file_path(raw_input).unwrap_or_else(|| "file".to_string());
            format!("Deleted {}", shorten_path(&file_path))
        }
        Some(acp::ToolKind::Move) => {
            let from = raw_input.and_then(|i| i.get("from")).and_then(|v| v.as_str());
            let to = raw_input.and_then(|i| i.get("to")).and_then(|v| v.as_str());
            match (from, to) {
                (Some(f), Some(t)) => format!("Moved {} → {}", shorten_path(f), shorten_path(t)),
                _ => format!("Ran {}", title),
            }
        }
        Some(acp::ToolKind::Execute) => {
            let cmd = raw_input
                .and_then(|i| i.get("command"))
                .and_then(|v| v.as_str())
                .map(|c| truncate_str(c, 50))
                .unwrap_or_else(|| title.to_string());
            format!("Ran {}", cmd)
        }
        _ => format_tool_call_command(title, raw_input),
    }
}

/// Calculate actual added/removed lines using simple diff
fn calculate_diff_stats(raw_input: Option<&serde_json::Value>) -> (usize, usize) {
    raw_input
        .and_then(|input| {
            let old = input.get("old_string")?.as_str()?;
            let new = input.get("new_string")?.as_str()?;

            let old_lines: std::collections::HashSet<_> = old.lines().collect();
            let new_lines: std::collections::HashSet<_> = new.lines().collect();

            let added = new_lines.difference(&old_lines).count();
            let removed = old_lines.difference(&new_lines).count();

            Some((added, removed))
        })
        .unwrap_or((0, 0))
}

/// Enhanced tool output extraction
fn extract_tool_output_enhanced(fields: &acp::ToolCallUpdateFields) -> String {
    // First try the existing extraction
    let base_output = extract_tool_output(fields);
    if !base_output.is_empty() {
        return base_output;
    }

    // For edit operations with no output, provide a summary
    if let Some(acp::ToolKind::Edit) = fields.kind.as_ref() {
        let file_path = extract_file_path(fields.raw_input.as_ref())
            .unwrap_or_else(|| "file".to_string());
        return format!("Applied edit to {}", shorten_path(&file_path));
    }

    String::new()
}
```

### Implementation Plan

#### Phase 1: Enhanced Command Formatting (Low Risk)
1. Add helper functions to `translator.rs`:
   - `shorten_path`, `truncate_str`, `extract_file_path`
   - `count_edit_lines`, `count_content_lines`
2. Update `extract_command_from_tool_call` with tool-specific formatting
3. Update `extract_reason_from_tool_call` with descriptive reasons

#### Phase 2: Enhanced Post-Approval Display (Low Risk)
1. Add `format_completed_tool_command` to `backend.rs`
2. Add `calculate_diff_stats` for accurate +/- counting
3. Update `translate_session_update_to_events` to use new formatting
4. Add `extract_tool_output_enhanced` for better output strings

#### Phase 3: Testing & Polish
1. Add unit tests for all formatting functions
2. Add E2E test with mock ACP agent
3. Tune truncation limits and preview lengths

### File Changes Summary

| File | Changes |
|------|---------|
| `codex-rs/acp/src/translator.rs` | Enhanced command/reason extraction functions |
| `codex-rs/acp/src/backend.rs` | Enhanced post-approval command formatting, diff stats |

**No changes to:**
- `codex-rs/protocol/src/approvals.rs`
- `codex-rs/tui/src/bottom_pane/approval_overlay.rs`
- Any other TUI or protocol files

### Testing Strategy

1. **Unit tests** for:
   - `format_edit_command` with various JSON inputs
   - `format_completed_tool_command` output strings
   - `calculate_diff_stats` accuracy
   - `shorten_path` and `truncate_str` edge cases

2. **Integration tests**:
   - `permission_request_to_approval_event` produces readable output
   - `translate_session_update_to_events` produces correct ExecCommandEnd

3. **E2E tests** (mock ACP agent):
   - Edit tool approval shows readable command + diff preview
   - Post-approval shows "Edited file.rs (+X -Y)"
   - Execute tool shows actual command

### Example Transformations

#### Edit Tool

**Input (raw_input JSON):**
```json
{
  "file_path": "/home/user/project/src/chatwidget.rs",
  "old_string": "    } else {\n        self.flush_active_cell();\n    }",
  "new_string": "    } else {\n        self.needs_final_message_separator = false;\n        self.flush_active_cell();\n    }"
}
```

**Approval command (proposed):**
```
Edit chatwidget.rs
--- old (3 lines)
    } else {
        self.flush_active_cell();
    }
+++ new (4 lines)
    } else {
        self.needs_final_message_separator = false;
        self.flush_active_cell();
    }
```

**Approval reason (proposed):**
```
Edit chatwidget.rs: replace 3 lines with 4 lines
```

**Post-approval display (proposed):**
```
• Edited chatwidget.rs (+1 -0)
```

#### Execute Tool

**Input:**
```json
{"command": "cargo test --release"}
```

**Approval command:** `cargo test --release`
**Approval reason:** `Execute: cargo test --release`
**Post-approval:** `Ran cargo test --release`

### Success Metrics

- Approval dialog shows file name (not raw JSON)
- Approval dialog shows diff preview for edit operations
- Reason describes the operation clearly
- Post-approval shows file name and change stats
- No regressions for shell command approvals
- All changes contained within `codex-rs/acp/`
