//! Translation between ACP types and codex-protocol types
//!
//! This module provides conversion functions to bridge between the ACP
//! (Agent Client Protocol) data types and the codex internal data types.

use std::collections::HashMap;
use std::path::PathBuf;

use agent_client_protocol as acp;
use codex_protocol::approvals::ApplyPatchApprovalRequestEvent;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::FileChange;

/// Convert codex ResponseItems to ACP ContentBlocks for prompting.
///
/// This extracts text content from user messages and other response items
/// to create a list of ACP content blocks that can be sent to an agent.
pub fn response_items_to_content_blocks(items: &[ResponseItem]) -> Vec<acp::ContentBlock> {
    let mut blocks = Vec::new();

    for item in items {
        match item {
            ResponseItem::Message { role, content, .. } if role == "user" => {
                // Extract text from user messages
                for content_item in content {
                    if let ContentItem::InputText { text } = content_item {
                        blocks.push(acp::ContentBlock::Text(acp::TextContent::new(text)));
                    }
                }
            }
            ResponseItem::Message { role, content, .. } if role == "assistant" => {
                // Include assistant messages for context
                for content_item in content {
                    if let ContentItem::OutputText { text } = content_item {
                        blocks.push(acp::ContentBlock::Text(acp::TextContent::new(text)));
                    }
                }
            }
            // Other item types are typically tool results which are handled separately
            _ => {}
        }
    }

    blocks
}

/// Translate a single text string to an ACP ContentBlock.
pub fn text_to_content_block(text: &str) -> acp::ContentBlock {
    acp::ContentBlock::Text(acp::TextContent::new(text))
}

/// Represents an event translated from an ACP SessionUpdate.
#[derive(Debug)]
pub enum TranslatedEvent {
    /// Text content from the agent
    TextDelta(String),
    /// Agent completed the message with a stop reason
    Completed(acp::StopReason),
}

/// Translate an ACP SessionUpdate to a list of TranslatedEvents.
///
/// Some SessionUpdate variants may produce multiple events (e.g., tool calls),
/// while others may produce none (e.g., internal state updates).
pub fn translate_session_update(update: acp::SessionUpdate) -> Vec<TranslatedEvent> {
    match update {
        acp::SessionUpdate::AgentMessageChunk(chunk) => {
            // Extract text from the content block
            match chunk.content {
                acp::ContentBlock::Text(text) => {
                    vec![TranslatedEvent::TextDelta(text.text)]
                }
                _ => {
                    // Non-text content types are not yet supported in the TUI
                    vec![]
                }
            }
        }
        acp::SessionUpdate::AgentThoughtChunk(chunk) => {
            // Thoughts are reasoning content - we could expose this as reasoning deltas
            match chunk.content {
                acp::ContentBlock::Text(text) => {
                    // For now, just treat thoughts as regular text
                    vec![TranslatedEvent::TextDelta(text.text)]
                }
                _ => {
                    // Non-text content in thoughts is not supported
                    vec![]
                }
            }
        }
        acp::SessionUpdate::ToolCall(_tool_call) => {
            // Tool calls are complex - for now, we just note them
            // The agent will send updates about tool execution via ToolCallUpdate
            vec![]
        }
        acp::SessionUpdate::ToolCallUpdate(_update) => {
            // Tool call results - could be mapped to function call outputs
            vec![]
        }
        acp::SessionUpdate::Plan(_plan) => {
            // Plans are agent-internal state
            vec![]
        }
        acp::SessionUpdate::UserMessageChunk(_) => {
            // Echo of user message - typically ignored
            vec![]
        }
        acp::SessionUpdate::CurrentModeUpdate(_) => {
            // Mode changes are internal state
            vec![]
        }
        acp::SessionUpdate::AvailableCommandsUpdate(_) => {
            // Command updates are internal state
            vec![]
        }
        _ => {
            // Handle any new update types added in future versions
            vec![]
        }
    }
}

/// Convert a text delta to a ResponseItem::Message for codex.
pub fn text_to_message_response_item(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: text.to_string(),
        }],
    }
}

/// Translate an ACP permission request to a Codex ExecApprovalRequestEvent.
///
/// This bridges ACP's permission model (multiple options) to Codex's approval model
/// (approve/deny). The translation extracts the tool call details and presents them
/// as a command for approval.
pub fn permission_request_to_approval_event(
    request: &acp::RequestPermissionRequest,
    cwd: &std::path::Path,
) -> codex_protocol::approvals::ExecApprovalRequestEvent {
    // Extract command details from the tool call
    let command = extract_command_from_tool_call(&request.tool_call);
    let reason = extract_reason_from_tool_call(&request.tool_call);

    codex_protocol::approvals::ExecApprovalRequestEvent {
        call_id: request.tool_call.tool_call_id.to_string(),
        turn_id: String::new(), // ACP doesn't have turn IDs
        command,
        cwd: cwd.to_path_buf(),
        reason,
        risk: None, // ACP doesn't provide risk assessment
        parsed_cmd: vec![],
    }
}

// ==================== Helper Functions ====================

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

/// Shorten a file path to just the filename for display.
fn shorten_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Calculate added/removed line counts using set difference.
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

/// Truncate a string to a maximum length, adding "..." if truncated.
/// Used for display purposes to keep output readable.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

// ==================== Command Extraction Functions ====================

/// Extract command string from either array or string format.
///
/// Codex sends commands as arrays: `["bash", "-lc", "cd /path && command"]`
/// Claude Code sends commands as strings: `"git status"`
///
/// This function handles both formats and extracts the actual command.
fn extract_command_string(raw_input: Option<&serde_json::Value>) -> Option<String> {
    raw_input.and_then(|input| {
        // Try array format first (Codex style)
        if let Some(cmd_array) = input.get("command").and_then(|v| v.as_array()) {
            // Check for bash wrapper: ["bash", "-lc", "cd ... && command"]
            if cmd_array.len() == 3
                && cmd_array.first().and_then(|v| v.as_str()) == Some("bash")
                && cmd_array.get(1).and_then(|v| v.as_str()) == Some("-lc")
            {
                // Extract the actual command from the shell wrapper
                if let Some(shell_cmd) = cmd_array.get(2).and_then(|v| v.as_str()) {
                    return extract_command_from_shell_wrapper(shell_cmd);
                }
            }
            // Fallback: join array elements
            return Some(
                cmd_array
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }

        // Try string format (Claude Code style)
        input
            .get("command")
            .and_then(|v| v.as_str())
            .map(String::from)
    })
}

/// Extract actual command from "cd /path && command" format.
///
/// Codex wraps commands in shell wrappers like:
/// `bash -lc "cd /home/user/project && git status"`
///
/// This function extracts just the "git status" part.
fn extract_command_from_shell_wrapper(shell_cmd: &str) -> Option<String> {
    // Look for "cd ... && command" pattern
    if let Some(pos) = shell_cmd.find(" && ") {
        Some(shell_cmd[pos + 4..].trim().to_string())
    } else {
        Some(shell_cmd.to_string())
    }
}

/// Extract parsed_cmd array from Codex rawInput for command metadata.
///
/// Codex provides a `parsed_cmd` array with command metadata:
/// ```json
/// "parsed_cmd": [{"cmd": "git status -sb", "type": "unknown"}]
/// ```
fn extract_parsed_cmd_string(raw_input: Option<&serde_json::Value>) -> Option<String> {
    raw_input.and_then(|input| {
        input
            .get("parsed_cmd")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|first| first.get("cmd"))
            .and_then(|v| v.as_str())
            .map(String::from)
    })
}

// ==================== Format Functions ====================

/// Format an Edit command with git-style summary: "Edit filename (+added -removed)"
fn format_edit_command(_title: &str, raw_input: Option<&serde_json::Value>) -> Vec<String> {
    let file_path = extract_file_path(raw_input).unwrap_or_else(|| "file".to_string());
    let short_path = shorten_path(&file_path);
    let (added, removed) = calculate_diff_stats(raw_input);

    vec![format!("Edit {} (+{} -{})", short_path, added, removed)]
}

/// Format a Write command: "Write filename (N lines)"
fn format_write_command(raw_input: Option<&serde_json::Value>) -> Vec<String> {
    let file_path = extract_file_path(raw_input).unwrap_or_else(|| "file".to_string());
    let short_path = shorten_path(&file_path);
    let line_count = raw_input
        .and_then(|i| i.get("content"))
        .and_then(|v| v.as_str())
        .map(|s| s.lines().count().max(1))
        .unwrap_or(0);

    vec![format!("Write {} ({} lines)", short_path, line_count)]
}

/// Format an Execute command: "Execute: command"
fn format_execute_command(_title: &str, raw_input: Option<&serde_json::Value>) -> Vec<String> {
    // Try parsed_cmd first (Codex metadata), then extract from command field
    let cmd = extract_parsed_cmd_string(raw_input)
        .or_else(|| extract_command_string(raw_input))
        .unwrap_or_else(|| "command".to_string());

    vec![format!("Execute: {}", cmd)]
}

/// Format a Delete command: "Delete filename"
fn format_delete_command(raw_input: Option<&serde_json::Value>) -> Vec<String> {
    let file_path = extract_file_path(raw_input).unwrap_or_else(|| "file".to_string());
    let short_path = shorten_path(&file_path);

    vec![format!("Delete {}", short_path)]
}

/// Format a Move command: "Move from → to"
fn format_move_command(raw_input: Option<&serde_json::Value>) -> Vec<String> {
    let from = raw_input
        .and_then(|i| i.get("from").or_else(|| i.get("source")))
        .and_then(|v| v.as_str())
        .map(shorten_path)
        .unwrap_or_else(|| "source".to_string());
    let to = raw_input
        .and_then(|i| i.get("to").or_else(|| i.get("destination")))
        .and_then(|v| v.as_str())
        .map(shorten_path)
        .unwrap_or_else(|| "destination".to_string());

    vec![format!("Move {} → {}", from, to)]
}

/// Generic fallback for unknown tool types
fn format_generic_command(title: &str, raw_input: Option<&serde_json::Value>) -> Vec<String> {
    let args = raw_input.and_then(|input| {
        // Try common argument names
        input
            .get("path")
            .or_else(|| input.get("command"))
            .or_else(|| input.get("query"))
            .or_else(|| input.get("pattern"))
            .and_then(|v| v.as_str())
            .map(|s| truncate_str(s, 60))
    });

    match args {
        Some(arg) => vec![format!("{}({})", title, arg)],
        None => vec![title.to_string()],
    }
}

// ==================== Main Extraction Functions ====================

/// Extract a command representation from an ACP ToolCallUpdate.
fn extract_command_from_tool_call(tool_call: &acp::ToolCallUpdate) -> Vec<String> {
    let title = tool_call.fields.title.as_deref().unwrap_or("Tool");
    let kind = tool_call.fields.kind.as_ref();
    let raw_input = tool_call.fields.raw_input.as_ref();

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
            // Check title for write-like operations or use generic format
            let title_lower = title.to_lowercase();
            if title_lower.contains("write") && raw_input.and_then(|i| i.get("content")).is_some() {
                format_write_command(raw_input)
            } else if title_lower.contains("edit")
                && raw_input.and_then(|i| i.get("old_string")).is_some()
            {
                format_edit_command(title, raw_input)
            } else {
                format_generic_command(title, raw_input)
            }
        }
    }
}

/// Extract a human-readable reason from the tool call.
fn extract_reason_from_tool_call(tool_call: &acp::ToolCallUpdate) -> Option<String> {
    let title = tool_call.fields.title.as_deref().unwrap_or("tool");
    let kind = tool_call.fields.kind.as_ref();
    let raw_input = tool_call.fields.raw_input.as_ref();

    let reason = match kind {
        Some(acp::ToolKind::Edit) => {
            let file_path = extract_file_path(raw_input).unwrap_or_else(|| "file".to_string());
            let short_path = shorten_path(&file_path);
            if raw_input.and_then(|i| i.get("old_string")).is_some() {
                let (added, removed) = calculate_diff_stats(raw_input);
                format!("Edit {short_path} (+{added} -{removed})")
            } else {
                let line_count = raw_input
                    .and_then(|i| i.get("content"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.lines().count().max(1))
                    .unwrap_or(0);
                format!("Write {short_path} ({line_count} lines)")
            }
        }
        Some(acp::ToolKind::Delete) => {
            let file_path = extract_file_path(raw_input).unwrap_or_else(|| "file".to_string());
            let short_path = shorten_path(&file_path);
            format!("Delete {short_path}")
        }
        Some(acp::ToolKind::Execute) => {
            // Try parsed_cmd first (Codex metadata), then extract from command field
            let cmd = extract_parsed_cmd_string(raw_input)
                .or_else(|| extract_command_string(raw_input))
                .unwrap_or_else(|| "command".to_string());
            let truncated = truncate_str(&cmd, 60);
            format!("Execute: {truncated}")
        }
        Some(acp::ToolKind::Move) => {
            let from = raw_input
                .and_then(|i| i.get("from"))
                .and_then(|v| v.as_str())
                .map(shorten_path);
            let to = raw_input
                .and_then(|i| i.get("to"))
                .and_then(|v| v.as_str())
                .map(shorten_path);
            match (from, to) {
                (Some(f), Some(t)) => format!("Move {f} → {t}"),
                _ => title.to_string(),
            }
        }
        _ => format!("ACP agent requests permission to use: {title}"),
    };

    Some(reason)
}

/// Translate a Codex ReviewDecision to an ACP RequestPermissionOutcome.
///
/// This maps the binary approve/deny decision to ACP's option-based model.
/// Uses the PermissionOptionKind to find the appropriate option.
pub fn review_decision_to_permission_outcome(
    decision: codex_protocol::protocol::ReviewDecision,
    options: &[acp::PermissionOption],
) -> acp::RequestPermissionOutcome {
    use codex_protocol::protocol::ReviewDecision;

    // Find the appropriate option based on the decision.
    // Critically, Approved maps to AllowOnce (one-time, no persistence) while
    // ApprovedForSession maps to AllowAlways (persistent "don't ask again").
    // This distinction prevents yolo/full-access auto-approvals from polluting
    // the user's settings file with permanent command approvals.
    let option_id = match decision {
        ReviewDecision::Approved => {
            // One-time approval: prefer AllowOnce to avoid persistent storage
            options
                .iter()
                .find(|opt| matches!(opt.kind, acp::PermissionOptionKind::AllowOnce))
                .or_else(|| {
                    options
                        .iter()
                        .find(|opt| matches!(opt.kind, acp::PermissionOptionKind::AllowAlways))
                })
                .or_else(|| {
                    options.iter().find(|opt| {
                        let name_lower = opt.name.to_lowercase();
                        name_lower.contains("allow")
                            || name_lower.contains("approve")
                            || name_lower.contains("yes")
                    })
                })
                .map(|opt| opt.option_id.clone())
                .unwrap_or_else(|| {
                    // Default to first option if no clear "allow" option
                    options
                        .first()
                        .map(|opt| opt.option_id.clone())
                        .unwrap_or_else(|| acp::PermissionOptionId::from("allow".to_string()))
                })
        }
        ReviewDecision::ApprovedForSession => {
            // Session/persistent approval: prefer AllowAlways for "don't ask again"
            options
                .iter()
                .find(|opt| matches!(opt.kind, acp::PermissionOptionKind::AllowAlways))
                .or_else(|| {
                    options
                        .iter()
                        .find(|opt| matches!(opt.kind, acp::PermissionOptionKind::AllowOnce))
                })
                .or_else(|| {
                    options.iter().find(|opt| {
                        let name_lower = opt.name.to_lowercase();
                        name_lower.contains("allow")
                            || name_lower.contains("approve")
                            || name_lower.contains("yes")
                    })
                })
                .map(|opt| opt.option_id.clone())
                .unwrap_or_else(|| {
                    // Default to first option if no clear "allow" option
                    options
                        .first()
                        .map(|opt| opt.option_id.clone())
                        .unwrap_or_else(|| acp::PermissionOptionId::from("allow".to_string()))
                })
        }
        ReviewDecision::Denied | ReviewDecision::Abort => {
            // Look for a "Reject" kind option (RejectOnce or RejectAlways)
            options
                .iter()
                .find(|opt| {
                    matches!(
                        opt.kind,
                        acp::PermissionOptionKind::RejectOnce
                            | acp::PermissionOptionKind::RejectAlways
                    )
                })
                .or_else(|| {
                    options.iter().find(|opt| {
                        let name_lower = opt.name.to_lowercase();
                        name_lower.contains("deny")
                            || name_lower.contains("reject")
                            || name_lower.contains("no")
                    })
                })
                .map(|opt| opt.option_id.clone())
                .unwrap_or_else(|| {
                    // Default to last option if no clear "reject" option
                    options
                        .last()
                        .map(|opt| opt.option_id.clone())
                        .unwrap_or_else(|| acp::PermissionOptionId::from("deny".to_string()))
                })
        }
    };

    acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(option_id))
}

// ==================== Patch Event Translation ====================

/// Check if a tool operation should use patch events instead of exec events.
///
/// Edit, Write (via Edit kind with content), and Delete operations are
/// rendered more elegantly using ApplyPatchApprovalRequest and PatchApplyBegin/End
/// events in the TUI.
pub fn is_patch_operation(
    kind: Option<&acp::ToolKind>,
    _title: &str,
    raw_input: Option<&serde_json::Value>,
) -> bool {
    match kind {
        Some(acp::ToolKind::Edit) => true,
        Some(acp::ToolKind::Delete) => true,
        // Fallback: check raw_input for edit/write patterns when kind is not set
        None | Some(acp::ToolKind::Other) => {
            if let Some(input) = raw_input {
                // Write operation: has content field
                if input.get("content").is_some() && extract_file_path(Some(input)).is_some() {
                    return true;
                }
                // Edit operation: has old_string and new_string
                if input.get("old_string").is_some() && input.get("new_string").is_some() {
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

/// Convert ACP tool call raw_input to a FileChange.
///
/// Supports three operation types:
/// - Edit: `old_string` + `new_string` → `FileChange::Update` with unified diff
/// - Write: `content` → `FileChange::Add`
/// - Delete: `ToolKind::Delete` → `FileChange::Delete`
///
/// Returns `None` if the raw_input doesn't contain recognizable file operation data.
pub fn tool_call_to_file_change(
    kind: Option<&acp::ToolKind>,
    raw_input: Option<&serde_json::Value>,
) -> Option<(PathBuf, FileChange)> {
    let input = raw_input?;
    let file_path = extract_file_path(Some(input))?;
    let path = PathBuf::from(&file_path);

    // Check for Delete operation
    if matches!(kind, Some(acp::ToolKind::Delete)) {
        // For delete, we may have content or may need to indicate deletion
        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Some((path, FileChange::Delete { content }));
    }

    // Check for Write operation (new file creation) - content without old_string
    if let Some(content) = input.get("content").and_then(|v| v.as_str())
        && input.get("old_string").is_none()
    {
        return Some((
            path,
            FileChange::Add {
                content: content.to_string(),
            },
        ));
    }

    // Check for Edit operation (string replacement)
    let old_string = input.get("old_string").and_then(|v| v.as_str())?;
    let new_string = input.get("new_string").and_then(|v| v.as_str())?;

    // Generate unified diff using diffy
    let unified_diff = diffy::create_patch(old_string, new_string).to_string();

    Some((
        path,
        FileChange::Update {
            unified_diff,
            move_path: None,
        },
    ))
}

/// Translate an ACP permission request to a Codex ApplyPatchApprovalRequestEvent.
///
/// This is used for Edit/Write/Delete operations to get the more elegant
/// patch approval UI in the TUI instead of the generic exec approval.
pub fn permission_request_to_patch_approval_event(
    request: &acp::RequestPermissionRequest,
) -> Option<ApplyPatchApprovalRequestEvent> {
    let kind = request.tool_call.fields.kind.as_ref();
    let raw_input = request.tool_call.fields.raw_input.as_ref();

    // Only convert if this is a patch operation
    if !is_patch_operation(kind, "", raw_input) {
        return None;
    }

    let (path, change) = tool_call_to_file_change(kind, raw_input)?;

    let mut changes = HashMap::new();
    changes.insert(path, change);

    // Generate a human-readable reason
    let reason = extract_reason_from_tool_call(&request.tool_call);

    Some(ApplyPatchApprovalRequestEvent {
        call_id: request.tool_call.tool_call_id.to_string(),
        turn_id: String::new(), // ACP doesn't have turn IDs
        changes,
        reason,
        grant_root: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::protocol::ReviewDecision;

    // ==================== New Approval Rendering Tests ====================

    #[test]
    fn test_extract_file_path_from_file_path_field() {
        let input = serde_json::json!({"file_path": "/home/user/src/main.rs"});
        let path = extract_file_path(Some(&input));
        assert_eq!(path, Some("/home/user/src/main.rs".to_string()));
    }

    #[test]
    fn test_extract_file_path_from_path_field() {
        let input = serde_json::json!({"path": "/home/user/src/lib.rs"});
        let path = extract_file_path(Some(&input));
        assert_eq!(path, Some("/home/user/src/lib.rs".to_string()));
    }

    #[test]
    fn test_extract_file_path_none_when_missing() {
        let input = serde_json::json!({"command": "ls -la"});
        let path = extract_file_path(Some(&input));
        assert_eq!(path, None);
    }

    #[test]
    fn test_shorten_path_extracts_filename() {
        assert_eq!(shorten_path("/home/user/project/src/main.rs"), "main.rs");
        assert_eq!(shorten_path("src/lib.rs"), "lib.rs");
        assert_eq!(shorten_path("file.txt"), "file.txt");
    }

    #[test]
    fn test_calculate_diff_stats_counts_changes() {
        let input = serde_json::json!({
            "old_string": "line1\nline2\nline3",
            "new_string": "line1\nmodified\nline3\nline4"
        });
        let (added, removed) = calculate_diff_stats(Some(&input));
        // "line2" removed, "modified" and "line4" added
        assert_eq!(added, 2);
        assert_eq!(removed, 1);
    }

    #[test]
    fn test_calculate_diff_stats_empty_input() {
        let (added, removed) = calculate_diff_stats(None);
        assert_eq!(added, 0);
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_truncate_str_long_string() {
        assert_eq!(truncate_str("hello world this is long", 10), "hello w...");
    }

    #[test]
    fn test_format_edit_command_produces_git_style_summary() {
        let input = serde_json::json!({
            "file_path": "/home/user/src/main.rs",
            "old_string": "fn old() {}\nfn other() {}",
            "new_string": "fn new() {\n    println!(\"hello\");\n}\nfn other() {}"
        });

        let cmd = format_edit_command("Edit", Some(&input));
        // Should produce something like "Edit main.rs (+2 -1)"
        assert_eq!(cmd.len(), 1);
        assert!(cmd[0].contains("main.rs"), "Should contain filename");
        assert!(cmd[0].contains("+"), "Should contain added count");
        assert!(cmd[0].contains("-"), "Should contain removed count");
    }

    #[test]
    fn test_format_write_command_shows_file_and_lines() {
        let input = serde_json::json!({
            "file_path": "/home/user/new_file.rs",
            "content": "line1\nline2\nline3"
        });

        let cmd = format_write_command(Some(&input));
        assert_eq!(cmd.len(), 1);
        assert!(cmd[0].contains("new_file.rs"), "Should contain filename");
        assert!(cmd[0].contains("3"), "Should contain line count");
    }

    #[test]
    fn test_format_execute_command_shows_shell_command() {
        let input = serde_json::json!({
            "command": "git status"
        });

        let cmd = format_execute_command("Terminal", Some(&input));
        assert_eq!(cmd.len(), 1);
        assert!(cmd[0].contains("git status"), "Should contain the command");
    }

    #[test]
    fn test_extract_reason_for_edit_tool() {
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
        assert!(reason.is_some());
        let reason = reason.unwrap();
        // Should mention editing, not generic "requests permission to use"
        assert!(
            !reason.contains("requests permission to use"),
            "Should not use generic reason"
        );
        assert!(
            reason.to_lowercase().contains("edit"),
            "Should mention edit"
        );
    }

    #[test]
    fn test_extract_reason_for_execute_tool() {
        let tool_call = acp::ToolCallUpdate::new(
            acp::ToolCallId::from("call-2".to_string()),
            acp::ToolCallUpdateFields::new()
                .title("Terminal")
                .kind(acp::ToolKind::Execute)
                .raw_input(serde_json::json!({
                    "command": "npm install"
                })),
        );

        let reason = extract_reason_from_tool_call(&tool_call);
        assert!(reason.is_some());
        let reason = reason.unwrap();
        assert!(
            reason.contains("npm install") || reason.to_lowercase().contains("execute"),
            "Should mention the command or execute"
        );
    }

    #[test]
    fn test_extract_command_for_edit_uses_git_style() {
        let tool_call = acp::ToolCallUpdate::new(
            acp::ToolCallId::from("call-edit".to_string()),
            acp::ToolCallUpdateFields::new()
                .title("Edit")
                .kind(acp::ToolKind::Edit)
                .raw_input(serde_json::json!({
                    "file_path": "/home/user/src/lib.rs",
                    "old_string": "fn foo() {}",
                    "new_string": "fn foo() {\n    bar();\n}"
                })),
        );

        let cmd = extract_command_from_tool_call(&tool_call);
        assert!(!cmd.is_empty());
        // Should NOT be raw JSON
        let cmd_str = cmd.join(" ");
        assert!(
            !cmd_str.contains("old_string"),
            "Should not contain raw JSON field names"
        );
        assert!(
            !cmd_str.contains("new_string"),
            "Should not contain raw JSON field names"
        );
        // Should contain filename
        assert!(cmd_str.contains("lib.rs"), "Should contain filename");
    }

    // ==================== Original Tests ====================

    #[test]
    fn test_permission_request_to_approval_event() {
        let tool_call = acp::ToolCallUpdate::new(
            acp::ToolCallId::from("call-123".to_string()),
            acp::ToolCallUpdateFields::new()
                .status(acp::ToolCallStatus::InProgress)
                .title("shell")
                .raw_input(serde_json::json!({"command": "ls -la"})),
        );

        let request = acp::RequestPermissionRequest::new(
            acp::SessionId::from("session-1".to_string()),
            tool_call,
            vec![],
        );

        let cwd = std::path::Path::new("/home/user/project");
        let event = permission_request_to_approval_event(&request, cwd);

        assert_eq!(event.call_id, "call-123");
        assert_eq!(event.cwd, cwd.to_path_buf());
        // Command should now be formatted with arguments
        let cmd_str = event.command.join(" ");
        assert!(
            cmd_str.contains("shell") || cmd_str.contains("ls -la"),
            "Command should contain tool name or arguments: {cmd_str}"
        );
        assert!(event.reason.is_some());
    }

    /// Extract the selected option_id from a RequestPermissionOutcome.
    fn selected_option_id(outcome: acp::RequestPermissionOutcome) -> String {
        match outcome {
            acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome {
                option_id,
                ..
            }) => option_id.0.as_ref().to_string(),
            other => panic!("Expected Selected outcome, got {other:?}"),
        }
    }

    #[test]
    fn test_approved_prefers_allow_once_over_allow_always() {
        // When both AllowAlways and AllowOnce are available, Approved should
        // pick AllowOnce to avoid persisting a permanent approval.
        let options = vec![
            acp::PermissionOption::new(
                acp::PermissionOptionId::from("always".to_string()),
                "Allow Always",
                acp::PermissionOptionKind::AllowAlways,
            ),
            acp::PermissionOption::new(
                acp::PermissionOptionId::from("once".to_string()),
                "Allow Once",
                acp::PermissionOptionKind::AllowOnce,
            ),
            acp::PermissionOption::new(
                acp::PermissionOptionId::from("deny".to_string()),
                "Deny",
                acp::PermissionOptionKind::RejectOnce,
            ),
        ];

        let outcome = review_decision_to_permission_outcome(ReviewDecision::Approved, &options);
        assert_eq!(selected_option_id(outcome), "once");
    }

    #[test]
    fn test_approved_for_session_prefers_allow_always() {
        // When both AllowAlways and AllowOnce are available,
        // ApprovedForSession should pick AllowAlways for "don't ask again".
        let options = vec![
            acp::PermissionOption::new(
                acp::PermissionOptionId::from("once".to_string()),
                "Allow Once",
                acp::PermissionOptionKind::AllowOnce,
            ),
            acp::PermissionOption::new(
                acp::PermissionOptionId::from("always".to_string()),
                "Allow Always",
                acp::PermissionOptionKind::AllowAlways,
            ),
            acp::PermissionOption::new(
                acp::PermissionOptionId::from("deny".to_string()),
                "Deny",
                acp::PermissionOptionKind::RejectOnce,
            ),
        ];

        let outcome =
            review_decision_to_permission_outcome(ReviewDecision::ApprovedForSession, &options);
        assert_eq!(selected_option_id(outcome), "always");
    }

    #[test]
    fn test_approved_falls_back_to_allow_always_when_no_allow_once() {
        let options = vec![
            acp::PermissionOption::new(
                acp::PermissionOptionId::from("always".to_string()),
                "Allow Always",
                acp::PermissionOptionKind::AllowAlways,
            ),
            acp::PermissionOption::new(
                acp::PermissionOptionId::from("deny".to_string()),
                "Deny",
                acp::PermissionOptionKind::RejectOnce,
            ),
        ];

        let outcome = review_decision_to_permission_outcome(ReviewDecision::Approved, &options);
        assert_eq!(selected_option_id(outcome), "always");
    }

    #[test]
    fn test_approved_for_session_falls_back_to_allow_once_when_no_allow_always() {
        let options = vec![
            acp::PermissionOption::new(
                acp::PermissionOptionId::from("once".to_string()),
                "Allow Once",
                acp::PermissionOptionKind::AllowOnce,
            ),
            acp::PermissionOption::new(
                acp::PermissionOptionId::from("deny".to_string()),
                "Deny",
                acp::PermissionOptionKind::RejectOnce,
            ),
        ];

        let outcome =
            review_decision_to_permission_outcome(ReviewDecision::ApprovedForSession, &options);
        assert_eq!(selected_option_id(outcome), "once");
    }

    #[test]
    fn test_denied_selects_reject_option() {
        let options = vec![
            acp::PermissionOption::new(
                acp::PermissionOptionId::from("allow".to_string()),
                "Allow",
                acp::PermissionOptionKind::AllowOnce,
            ),
            acp::PermissionOption::new(
                acp::PermissionOptionId::from("deny".to_string()),
                "Deny",
                acp::PermissionOptionKind::RejectOnce,
            ),
        ];

        let outcome = review_decision_to_permission_outcome(ReviewDecision::Denied, &options);
        assert_eq!(selected_option_id(outcome), "deny");
    }

    #[test]
    fn test_text_to_content_block() {
        let block = text_to_content_block("Hello, world!");
        match block {
            acp::ContentBlock::Text(text) => {
                assert_eq!(text.text, "Hello, world!");
            }
            _ => panic!("Expected text block"),
        }
    }

    #[test]
    fn test_translate_agent_message_chunk() {
        let update = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
            acp::ContentBlock::Text(acp::TextContent::new("Test response")),
        ));

        let events = translate_session_update(update);
        assert_eq!(events.len(), 1);
        match &events[0] {
            TranslatedEvent::TextDelta(text) => {
                assert_eq!(text, "Test response");
            }
            _ => panic!("Expected TextDelta"),
        }
    }

    #[test]
    fn test_response_items_to_content_blocks() {
        let items = vec![
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "Hello".to_string(),
                }],
            },
            ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "Hi there".to_string(),
                }],
            },
        ];

        let blocks = response_items_to_content_blocks(&items);
        assert_eq!(blocks.len(), 2);

        match &blocks[0] {
            acp::ContentBlock::Text(text) => assert_eq!(text.text, "Hello"),
            _ => panic!("Expected text block"),
        }

        match &blocks[1] {
            acp::ContentBlock::Text(text) => assert_eq!(text.text, "Hi there"),
            _ => panic!("Expected text block"),
        }
    }

    // ==================== Patch Event Translation Tests ====================

    #[test]
    fn test_is_patch_operation_edit_kind() {
        assert!(is_patch_operation(Some(&acp::ToolKind::Edit), "Edit", None));
    }

    #[test]
    fn test_is_patch_operation_delete_kind() {
        assert!(is_patch_operation(
            Some(&acp::ToolKind::Delete),
            "Delete",
            None
        ));
    }

    #[test]
    fn test_is_patch_operation_execute_kind_is_false() {
        assert!(!is_patch_operation(
            Some(&acp::ToolKind::Execute),
            "Terminal",
            Some(&serde_json::json!({"command": "ls"}))
        ));
    }

    #[test]
    fn test_is_patch_operation_fallback_with_content() {
        let input = serde_json::json!({
            "file_path": "/path/to/file.txt",
            "content": "new file content"
        });
        assert!(is_patch_operation(None, "Write", Some(&input)));
    }

    #[test]
    fn test_is_patch_operation_fallback_with_old_new_string() {
        let input = serde_json::json!({
            "file_path": "/path/to/file.txt",
            "old_string": "old",
            "new_string": "new"
        });
        assert!(is_patch_operation(None, "Edit", Some(&input)));
    }

    #[test]
    fn test_tool_call_to_file_change_edit() {
        let input = serde_json::json!({
            "file_path": "/src/main.rs",
            "old_string": "fn old() {}",
            "new_string": "fn new() {\n    println!(\"hello\");\n}"
        });

        let result = tool_call_to_file_change(Some(&acp::ToolKind::Edit), Some(&input));
        assert!(result.is_some());

        let (path, change) = result.unwrap();
        assert_eq!(path, PathBuf::from("/src/main.rs"));

        match change {
            FileChange::Update {
                unified_diff,
                move_path,
            } => {
                // The diff should contain the changes
                assert!(unified_diff.contains("-fn old() {}"));
                assert!(unified_diff.contains("+fn new() {"));
                assert!(unified_diff.contains("+    println!(\"hello\");"));
                assert!(move_path.is_none());
            }
            _ => panic!("Expected FileChange::Update"),
        }
    }

    #[test]
    fn test_tool_call_to_file_change_write() {
        let input = serde_json::json!({
            "file_path": "/src/new_file.rs",
            "content": "// New file\nfn main() {}\n"
        });

        let result = tool_call_to_file_change(Some(&acp::ToolKind::Edit), Some(&input));
        assert!(result.is_some());

        let (path, change) = result.unwrap();
        assert_eq!(path, PathBuf::from("/src/new_file.rs"));

        match change {
            FileChange::Add { content } => {
                assert_eq!(content, "// New file\nfn main() {}\n");
            }
            _ => panic!("Expected FileChange::Add"),
        }
    }

    #[test]
    fn test_tool_call_to_file_change_delete() {
        let input = serde_json::json!({
            "file_path": "/src/old_file.rs",
            "content": "// File to delete\n"
        });

        let result = tool_call_to_file_change(Some(&acp::ToolKind::Delete), Some(&input));
        assert!(result.is_some());

        let (path, change) = result.unwrap();
        assert_eq!(path, PathBuf::from("/src/old_file.rs"));

        match change {
            FileChange::Delete { content } => {
                assert_eq!(content, "// File to delete\n");
            }
            _ => panic!("Expected FileChange::Delete"),
        }
    }

    #[test]
    fn test_tool_call_to_file_change_missing_path_returns_none() {
        let input = serde_json::json!({
            "content": "some content"
        });

        let result = tool_call_to_file_change(Some(&acp::ToolKind::Edit), Some(&input));
        assert!(result.is_none());
    }

    #[test]
    fn test_permission_request_to_patch_approval_event_edit() {
        let tool_call = acp::ToolCallUpdate::new(
            acp::ToolCallId::from("call-edit".to_string()),
            acp::ToolCallUpdateFields::new()
                .title("Edit")
                .kind(acp::ToolKind::Edit)
                .raw_input(serde_json::json!({
                    "file_path": "/src/lib.rs",
                    "old_string": "fn foo() {}",
                    "new_string": "fn foo() {\n    bar();\n}"
                })),
        );

        let request = acp::RequestPermissionRequest::new(
            acp::SessionId::from("session-1".to_string()),
            tool_call,
            vec![],
        );

        let event = permission_request_to_patch_approval_event(&request);
        assert!(event.is_some());

        let event = event.unwrap();
        assert_eq!(event.call_id, "call-edit");
        assert_eq!(event.changes.len(), 1);
        assert!(event.changes.contains_key(&PathBuf::from("/src/lib.rs")));

        match event.changes.get(&PathBuf::from("/src/lib.rs")).unwrap() {
            FileChange::Update { unified_diff, .. } => {
                assert!(unified_diff.contains("-fn foo() {}"));
                assert!(unified_diff.contains("+fn foo() {"));
            }
            _ => panic!("Expected FileChange::Update"),
        }
    }

    #[test]
    fn test_permission_request_to_patch_approval_event_execute_returns_none() {
        let tool_call = acp::ToolCallUpdate::new(
            acp::ToolCallId::from("call-exec".to_string()),
            acp::ToolCallUpdateFields::new()
                .title("Terminal")
                .kind(acp::ToolKind::Execute)
                .raw_input(serde_json::json!({
                    "command": "ls -la"
                })),
        );

        let request = acp::RequestPermissionRequest::new(
            acp::SessionId::from("session-1".to_string()),
            tool_call,
            vec![],
        );

        let event = permission_request_to_patch_approval_event(&request);
        assert!(event.is_none());
    }

    #[test]
    fn test_permission_request_to_patch_approval_event_delete() {
        let tool_call = acp::ToolCallUpdate::new(
            acp::ToolCallId::from("call-delete".to_string()),
            acp::ToolCallUpdateFields::new()
                .title("Delete")
                .kind(acp::ToolKind::Delete)
                .raw_input(serde_json::json!({
                    "file_path": "/tmp/old.txt",
                    "content": "old content"
                })),
        );

        let request = acp::RequestPermissionRequest::new(
            acp::SessionId::from("session-1".to_string()),
            tool_call,
            vec![],
        );

        let event = permission_request_to_patch_approval_event(&request);
        assert!(event.is_some());

        let event = event.unwrap();
        assert_eq!(event.call_id, "call-delete");

        match event.changes.get(&PathBuf::from("/tmp/old.txt")).unwrap() {
            FileChange::Delete { content } => {
                assert_eq!(content, "old content");
            }
            _ => panic!("Expected FileChange::Delete"),
        }
    }
}
