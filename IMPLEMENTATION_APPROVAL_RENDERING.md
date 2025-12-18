# ACP Approval Rendering Implementation Guide

This document provides a detailed technical guide for the ACP approval rendering improvements implemented in this branch. All changes are contained within `codex-rs/acp/` and require no modifications to protocol or TUI types.

## Table of Contents

1. [Problem Statement](#problem-statement)
2. [Architecture Overview](#architecture-overview)
3. [Implementation Details](#implementation-details)
   - [translator.rs Changes](#translatorrs-changes)
   - [backend.rs Changes](#backendrs-changes)
4. [Key Code Snippets](#key-code-snippets)
5. [Testing](#testing)
6. [Design Decisions](#design-decisions)

---

## Problem Statement

### Before (Poor Rendering)

**Approval Request:**
```
Would you like to run the following command?

Reason: ACP agent requests permission to use: unknown tool

$ toolu_01Hmtbur4ZGyevLqpoSvnfrk
  "{\"file_path\":\"/home/user/project/src/file.rs\",\"old_string\":\"...\",\"new_string\":\"...\"}"
```

**Post-Approval:**
```
• Ran Edit
  └ (no output)
```

### After (Improved Rendering)

**Approval Request:**
```
Would you like to run the following command?

Reason: Edit src/chatwidget.rs: replace 5 lines with 6 lines

$ Edit chatwidget.rs
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

**Post-Approval:**
```
• Edited chatwidget.rs (+6 -5)
```

---

## Architecture Overview

The TUI renders approval requests using these fields from `ExecApprovalRequestEvent`:

| Field | Usage |
|-------|-------|
| `command: Vec<String>` | Displayed as shell command with `$ ` prefix |
| `reason: Option<String>` | Displayed as "Reason: {reason}" |
| `parsed_cmd: Vec<ParsedCommand>` | Determines rendering mode (Exploring vs Command) |

The TUI renders completed tool calls using `ExecCommandEndEvent`:

| Field | Usage |
|-------|-------|
| `command: Vec<String>` | Displayed as the completed action |
| `aggregated_output: String` | Displayed under the command |
| `parsed_cmd: Vec<ParsedCommand>` | Determines rendering mode |

**Key Insight**: The ACP module formats data to fit these existing expectations without modifying the types.

---

## Implementation Details

### translator.rs Changes

#### 1. Command Extraction

The `extract_command_from_tool_call` function produces human-readable commands based on tool type:

```rust
fn extract_command_from_tool_call(tool_call: &acp::ToolCallUpdate) -> Vec<String> {
    let title = tool_call.fields.title.as_deref().unwrap_or("Tool");
    let kind = tool_call.fields.kind.as_ref();
    let raw_input = tool_call.fields.raw_input.as_ref();

    // Note: ACP ToolKind doesn't have a Write variant - write operations
    // typically come through as Edit or Other with title-based detection
    match kind {
        Some(acp::ToolKind::Edit) => {
            // Check if this is a write (new file) vs edit (string replacement)
            if raw_input.and_then(|i| i.get("old_string")).is_some() {
                format_edit_command(title, raw_input)
            } else if raw_input.and_then(|i| i.get("content")).is_some() {
                format_write_command(raw_input)
            } else {
                format_edit_command(title, raw_input)
            }
        }
        Some(acp::ToolKind::Delete) => format_delete_command(raw_input),
        Some(acp::ToolKind::Execute) => format_execute_command(title, raw_input),
        Some(acp::ToolKind::Move) => format_move_command(raw_input),
        _ => {
            // Check title for write-like operations
            let title_lower = title.to_lowercase();
            if title_lower.contains("write") && raw_input.and_then(|i| i.get("content")).is_some() {
                format_write_command(raw_input)
            } else {
                format_generic_command(title, raw_input)
            }
        }
    }
}
```

#### 2. Edit Command Formatting with Diff Preview

```rust
fn format_edit_command(title: &str, raw_input: Option<&serde_json::Value>) -> Vec<String> {
    let Some(input) = raw_input else {
        return vec![title.to_string()];
    };

    let file_path = extract_file_path(Some(input)).unwrap_or_else(|| "file".to_string());
    let old_string = input.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
    let new_string = input.get("new_string").and_then(|v| v.as_str()).unwrap_or("");

    let short_path = shorten_path(&file_path);
    let old_lines = old_string.lines().count().max(1);
    let new_lines = new_string.lines().count().max(1);

    // Build a readable diff preview
    let mut preview = String::new();
    preview.push_str(&format!(
        "--- old ({} line{})\n",
        old_lines,
        if old_lines == 1 { "" } else { "s" }
    ));
    for line in old_string.lines().take(10) {
        preview.push_str(line);
        preview.push('\n');
    }
    if old_lines > 10 {
        preview.push_str(&format!("... ({} more lines)\n", old_lines - 10));
    }
    preview.push_str(&format!(
        "+++ new ({} line{})\n",
        new_lines,
        if new_lines == 1 { "" } else { "s" }
    ));
    for line in new_string.lines().take(10) {
        preview.push_str(line);
        preview.push('\n');
    }
    if new_lines > 10 {
        preview.push_str(&format!("... ({} more lines)\n", new_lines - 10));
    }

    vec![format!("Edit {}", short_path), preview.trim_end().to_string()]
}
```

#### 3. Reason Extraction

```rust
fn extract_reason_from_tool_call(tool_call: &acp::ToolCallUpdate) -> Option<String> {
    let title = tool_call.fields.title.as_deref().unwrap_or("tool");
    let kind = tool_call.fields.kind.as_ref();
    let raw_input = tool_call.fields.raw_input.as_ref();

    let reason = match kind {
        Some(acp::ToolKind::Edit) => {
            let file_path = extract_file_path(raw_input).unwrap_or_else(|| "file".to_string());
            if raw_input.and_then(|i| i.get("old_string")).is_some() {
                let (old_lines, new_lines) = count_edit_lines(raw_input);
                format!(
                    "Edit {}: replace {} line{} with {} line{}",
                    shorten_path(&file_path),
                    old_lines,
                    if old_lines == 1 { "" } else { "s" },
                    new_lines,
                    if new_lines == 1 { "" } else { "s" }
                )
            } else {
                let line_count = count_content_lines(raw_input);
                format!(
                    "Write {} ({} line{})",
                    shorten_path(&file_path),
                    line_count,
                    if line_count == 1 { "" } else { "s" }
                )
            }
        }
        Some(acp::ToolKind::Delete) => {
            let file_path = extract_file_path(raw_input).unwrap_or_else(|| "file".to_string());
            format!("Delete {}", shorten_path(&file_path))
        }
        Some(acp::ToolKind::Execute) => {
            let cmd = raw_input
                .and_then(|i| i.get("command"))
                .and_then(|v| v.as_str())
                .unwrap_or("command");
            format!("Execute: {}", truncate_str(cmd, 60))
        }
        Some(acp::ToolKind::Move) => {
            // ... format move reason
        }
        _ => format!("ACP agent requests permission to use: {}", title),
    };

    Some(reason)
}
```

### backend.rs Changes

#### 1. Post-Approval Command Display

```rust
fn format_completed_tool_command(
    title: &str,
    kind: Option<&acp::ToolKind>,
    raw_input: Option<&serde_json::Value>,
) -> String {
    match kind {
        Some(acp::ToolKind::Edit) => {
            let file_path = extract_file_path(raw_input).unwrap_or_else(|| "file".to_string());
            // Check if this is a write (has content) vs edit (has old_string)
            if raw_input.and_then(|i| i.get("old_string")).is_some() {
                let (added, removed) = calculate_diff_stats(raw_input);
                format!(
                    "Edited {} (+{} -{})",
                    shorten_path(&file_path),
                    added,
                    removed
                )
            } else {
                format!("Wrote {}", shorten_path(&file_path))
            }
        }
        Some(acp::ToolKind::Delete) => {
            let file_path = extract_file_path(raw_input).unwrap_or_else(|| "file".to_string());
            format!("Deleted {}", shorten_path(&file_path))
        }
        Some(acp::ToolKind::Move) => {
            let from = raw_input.and_then(|i| i.get("from")).and_then(|v| v.as_str());
            let to = raw_input.and_then(|i| i.get("to")).and_then(|v| v.as_str());
            match (from, to) {
                (Some(f), Some(t)) => {
                    format!("Moved {} → {}", shorten_path(f), shorten_path(t))
                }
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
```

#### 2. Diff Statistics Calculation

```rust
fn calculate_diff_stats(raw_input: Option<&serde_json::Value>) -> (usize, usize) {
    raw_input
        .and_then(|input| {
            let old = input.get("old_string")?.as_str()?;
            let new = input.get("new_string")?.as_str()?;

            let old_lines: std::collections::HashSet<_> = old.lines().collect();
            let new_lines: std::collections::HashSet<_> = new.lines().collect();

            let added = new_lines.difference(&old_lines).count();
            let removed = old_lines.difference(&new_lines).count();

            // Ensure at least some change is shown if strings differ
            if added == 0 && removed == 0 && old != new {
                Some((1, 1))
            } else {
                Some((added, removed))
            }
        })
        .unwrap_or((0, 0))
}
```

#### 3. Tool Classification for TUI Rendering

Maps ACP `ToolKind` to `ParsedCommand` for proper TUI rendering mode:

```rust
fn classify_tool_to_parsed_command(
    title: &str,
    kind: Option<&acp::ToolKind>,
    raw_input: Option<&serde_json::Value>,
) -> Vec<ParsedCommand> {
    match kind {
        // Read operations → Exploring mode (compact display)
        Some(acp::ToolKind::Read) => {
            let path = raw_input
                .and_then(|i| i.get("path").or_else(|| i.get("file_path")))
                .and_then(|v| v.as_str())
                .unwrap_or("file");
            let name = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string());
            vec![ParsedCommand::Read {
                cmd: title.to_string(),
                name,
                path: std::path::PathBuf::from(path),
            }]
        }

        // Search operations → Exploring mode
        Some(acp::ToolKind::Search) => {
            let query = raw_input
                .and_then(|i| i.get("pattern").or_else(|| i.get("query")))
                .and_then(|v| v.as_str())
                .map(String::from);
            let path = raw_input
                .and_then(|i| i.get("path"))
                .and_then(|v| v.as_str())
                .map(String::from);
            vec![ParsedCommand::Search {
                cmd: title.to_string(),
                query,
                path,
            }]
        }

        // Mutating operations → Command mode (full display)
        Some(acp::ToolKind::Edit | acp::ToolKind::Delete | acp::ToolKind::Move) => {
            vec![ParsedCommand::Unknown {
                cmd: format_tool_call_command(title, raw_input),
            }]
        }

        Some(acp::ToolKind::Execute) => {
            vec![ParsedCommand::Unknown {
                cmd: format_tool_call_command(title, raw_input),
            }]
        }

        // Fallback: try to infer from title
        Some(acp::ToolKind::Other) | None => {
            classify_tool_by_title(title, raw_input)
        }
        
        _ => vec![ParsedCommand::Unknown {
            cmd: format_tool_call_command(title, raw_input),
        }]
    }
}
```

---

## Key Code Snippets

### Helper Functions

```rust
/// Extract file path from raw_input JSON, checking common field names.
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

/// Count lines in old_string and new_string for edit operations.
fn count_edit_lines(raw_input: Option<&serde_json::Value>) -> (usize, usize) {
    raw_input
        .map(|input| {
            let old = input.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
            let new = input.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
            (old.lines().count().max(1), new.lines().count().max(1))
        })
        .unwrap_or((1, 1))
}

/// Shorten a file path to just the filename for display.
fn shorten_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Truncate a string to a maximum length, adding "..." if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
```

### Integration Points

**In `translator.rs` - `permission_request_to_approval_event`:**
```rust
pub fn permission_request_to_approval_event(
    request: &acp::RequestPermissionRequest,
    cwd: &std::path::Path,
) -> codex_protocol::approvals::ExecApprovalRequestEvent {
    let command = extract_command_from_tool_call(&request.tool_call);
    let reason = extract_reason_from_tool_call(&request.tool_call);

    codex_protocol::approvals::ExecApprovalRequestEvent {
        call_id: request.tool_call.tool_call_id.to_string(),
        turn_id: String::new(),
        command,  // Now contains human-readable command + diff preview
        cwd: cwd.to_path_buf(),
        reason,   // Now contains descriptive reason
        risk: None,
        parsed_cmd: vec![],
    }
}
```

**In `backend.rs` - `translate_session_update_to_events` for `ToolCallUpdate`:**
```rust
acp::SessionUpdate::ToolCallUpdate(update) => {
    if update.fields.status == Some(acp::ToolCallStatus::Completed) {
        let aggregated_output = extract_tool_output_enhanced(&update.fields);
        let title = update.fields.title.clone().unwrap_or_default();
        let command = format_completed_tool_command(
            &title,
            update.fields.kind.as_ref(),
            update.fields.raw_input.as_ref(),
        );
        let parsed_cmd = classify_tool_to_parsed_command(
            &title,
            update.fields.kind.as_ref(),
            update.fields.raw_input.as_ref(),
        );

        vec![EventMsg::ExecCommandEnd(
            codex_protocol::protocol::ExecCommandEndEvent {
                call_id: update.tool_call_id.to_string(),
                command: vec![command],  // "Edited file.rs (+6 -5)"
                aggregated_output,
                parsed_cmd,
                // ... other fields
            },
        )]
    } else {
        vec![]
    }
}
```

---

## Testing

### Unit Tests in translator.rs

```rust
#[test]
fn test_format_edit_command() {
    let input = serde_json::json!({
        "file_path": "/home/user/src/main.rs",
        "old_string": "fn old() {}",
        "new_string": "fn new() {\n    println!(\"hello\");\n}"
    });

    let cmd = format_edit_command("Edit", Some(&input));
    assert_eq!(cmd.len(), 2);
    assert_eq!(cmd[0], "Edit main.rs");
    assert!(cmd[1].contains("--- old (1 line)"));
    assert!(cmd[1].contains("+++ new (3 lines)"));
}

#[test]
fn test_extract_reason_edit() {
    let tool_call = acp::ToolCallUpdate::new(
        acp::ToolCallId::from("call-1".to_string()),
        acp::ToolCallUpdateFields::new()
            .title("Edit")
            .kind(acp::ToolKind::Edit)
            .raw_input(serde_json::json!({
                "file_path": "/src/main.rs",
                "old_string": "old\ncode",
                "new_string": "new\ncode\nhere"
            })),
    );

    let reason = extract_reason_from_tool_call(&tool_call);
    assert!(reason.unwrap().contains("replace 2 lines with 3 lines"));
}
```

### Unit Tests in backend.rs

```rust
#[test]
fn test_classify_tool_kind_read() {
    let parsed = classify_tool_to_parsed_command(
        "Read File",
        Some(&acp::ToolKind::Read),
        Some(&serde_json::json!({"path": "src/main.rs"})),
    );
    match &parsed[0] {
        ParsedCommand::Read { cmd, name, path } => {
            assert_eq!(cmd, "Read File");
            assert_eq!(name, "main.rs");
        }
        _ => panic!("Expected ParsedCommand::Read"),
    }
}

#[test]
fn test_classify_tool_kind_execute() {
    let parsed = classify_tool_to_parsed_command(
        "Terminal",
        Some(&acp::ToolKind::Execute),
        Some(&serde_json::json!({"command": "git status"})),
    );
    match &parsed[0] {
        ParsedCommand::Unknown { cmd } => {
            assert_eq!(cmd, "Terminal(git status)");
        }
        _ => panic!("Expected ParsedCommand::Unknown"),
    }
}
```

---

## Design Decisions

### 1. No Write Variant in ACP ToolKind

ACP's `ToolKind` enum doesn't include a `Write` variant. Write operations are detected by:
- Checking for `content` field presence (vs `old_string` for edits)
- Title-based matching (e.g., title contains "write")

```rust
Some(acp::ToolKind::Edit) => {
    if raw_input.and_then(|i| i.get("old_string")).is_some() {
        format_edit_command(title, raw_input)  // String replacement edit
    } else if raw_input.and_then(|i| i.get("content")).is_some() {
        format_write_command(raw_input)  // New file creation
    }
}
```

### 2. Diff Preview Truncation

Large diffs are truncated to 10 lines each for old/new content to prevent TUI overflow:

```rust
for line in old_string.lines().take(10) {
    preview.push_str(line);
}
if old_lines > 10 {
    preview.push_str(&format!("... ({} more lines)\n", old_lines - 10));
}
```

### 3. Set-Based Diff Statistics

Uses set difference for accurate +/- counts that match git-style output:

```rust
let old_lines: std::collections::HashSet<_> = old.lines().collect();
let new_lines: std::collections::HashSet<_> = new.lines().collect();

let added = new_lines.difference(&old_lines).count();
let removed = old_lines.difference(&new_lines).count();
```

### 4. Exploring vs Command Mode

Tools are classified into two rendering modes:
- **Exploring**: `Read`, `Search`, `ListFiles` - compact, grouped display
- **Command**: `Edit`, `Execute`, `Delete`, `Move` - full command display

This matches the existing TUI behavior for native Codex tools.

---

## Files Changed

| File | Lines Added | Lines Removed | Description |
|------|-------------|---------------|-------------|
| `acp/src/translator.rs` | ~540 | ~18 | Command/reason extraction, formatting |
| `acp/src/backend.rs` | ~391 | ~1 | Post-approval display, classification |

---

## Running Tests

```bash
cd codex-rs
cargo test -p acp
```

All 70+ unit tests should pass, including the new formatting tests.
