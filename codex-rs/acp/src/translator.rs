//! Translation between ACP types and codex-protocol types
//!
//! This module provides conversion functions to bridge between the ACP
//! (Agent Client Protocol) data types and the codex internal data types.

use agent_client_protocol as acp;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;

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

/// Extract a command representation from an ACP ToolCallUpdate.
///
/// Formats the command based on tool type to produce human-readable output
/// instead of raw JSON. For edit operations, includes a diff preview.
fn extract_command_from_tool_call(tool_call: &acp::ToolCallUpdate) -> Vec<String> {
    let title = tool_call.fields.title.as_deref().unwrap_or("Tool");
    let kind = tool_call.fields.kind.as_ref();
    let raw_input = tool_call.fields.raw_input.as_ref();

    // Note: ACP ToolKind doesn't have a Write variant - write operations
    // typically come through as Edit or Other with title-based detection
    match kind {
        Some(acp::ToolKind::Edit) => {
            // Check if this is a write (new file) vs edit (string replacement)
            if raw_input
                .and_then(|i| i.get("old_string"))
                .and_then(|v| v.as_str())
                .is_some()
            {
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

/// Format an edit command with a diff preview.
fn format_edit_command(title: &str, raw_input: Option<&serde_json::Value>) -> Vec<String> {
    let Some(input) = raw_input else {
        return vec![title.to_string()];
    };

    let file_path = extract_file_path(Some(input)).unwrap_or_else(|| "file".to_string());
    let old_string = input
        .get("old_string")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let new_string = input
        .get("new_string")
        .and_then(|v| v.as_str())
        .unwrap_or("");

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

/// Format a write command showing file path and line count.
fn format_write_command(raw_input: Option<&serde_json::Value>) -> Vec<String> {
    let file_path =
        extract_file_path(raw_input).unwrap_or_else(|| "file".to_string());
    let content = raw_input
        .and_then(|i| i.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let line_count = content.lines().count().max(1);
    let preview: String = content.lines().take(5).collect::<Vec<_>>().join("\n");

    let mut result = vec![format!(
        "Write {} ({} line{})",
        shorten_path(&file_path),
        line_count,
        if line_count == 1 { "" } else { "s" }
    )];
    if !preview.is_empty() {
        if line_count > 5 {
            result.push(format!("{}\n...", preview));
        } else {
            result.push(preview);
        }
    }
    result
}

/// Format an execute/shell command.
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

/// Format a delete command.
fn format_delete_command(raw_input: Option<&serde_json::Value>) -> Vec<String> {
    let file_path =
        extract_file_path(raw_input).unwrap_or_else(|| "file".to_string());
    vec![format!("Delete {}", shorten_path(&file_path))]
}

/// Format a move command.
fn format_move_command(raw_input: Option<&serde_json::Value>) -> Vec<String> {
    let from = raw_input
        .and_then(|i| i.get("from").or_else(|| i.get("source")))
        .and_then(|v| v.as_str());
    let to = raw_input
        .and_then(|i| i.get("to").or_else(|| i.get("destination")))
        .and_then(|v| v.as_str());

    match (from, to) {
        (Some(f), Some(t)) => {
            vec![format!("Move {} → {}", shorten_path(f), shorten_path(t))]
        }
        _ => vec!["Move file".to_string()],
    }
}

/// Format a generic command using the existing format_tool_call_command logic.
fn format_generic_command(title: &str, raw_input: Option<&serde_json::Value>) -> Vec<String> {
    let args = raw_input
        .and_then(|input| extract_display_args(title, input))
        .unwrap_or_default();

    if args.is_empty() {
        vec![title.to_string()]
    } else {
        vec![format!("{title}({args})")]
    }
}

/// Extract display-friendly arguments from raw_input based on tool type.
fn extract_display_args(title: &str, input: &serde_json::Value) -> Option<String> {
    let title_lower = title.to_lowercase();

    if title_lower.contains("search")
        || title_lower.contains("find")
        || title_lower.contains("grep")
    {
        let pattern = input
            .get("pattern")
            .or_else(|| input.get("query"))
            .or_else(|| input.get("glob"))
            .and_then(|v| v.as_str());
        let path = input.get("path").and_then(|v| v.as_str());

        match (pattern, path) {
            (Some(p), Some(dir)) => Some(format!("{p} in {dir}")),
            (Some(p), None) => Some(p.to_string()),
            (None, Some(dir)) => Some(dir.to_string()),
            (None, None) => None,
        }
    } else if title_lower.contains("read") || title_lower.contains("file") {
        extract_file_path(Some(input))
    } else {
        input
            .get("path")
            .or_else(|| input.get("command"))
            .or_else(|| input.get("query"))
            .or_else(|| input.get("name"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }
}

/// Extract a human-readable reason from the tool call.
///
/// Provides descriptive reasons based on tool type instead of generic messages.
fn extract_reason_from_tool_call(tool_call: &acp::ToolCallUpdate) -> Option<String> {
    let title = tool_call.fields.title.as_deref().unwrap_or("tool");
    let kind = tool_call.fields.kind.as_ref();
    let raw_input = tool_call.fields.raw_input.as_ref();

    let reason = match kind {
        Some(acp::ToolKind::Edit) => {
            let file_path =
                extract_file_path(raw_input).unwrap_or_else(|| "file".to_string());
            // Check if this is a write (has content) or edit (has old_string)
            if raw_input
                .and_then(|i| i.get("old_string"))
                .and_then(|v| v.as_str())
                .is_some()
            {
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
            let file_path =
                extract_file_path(raw_input).unwrap_or_else(|| "file".to_string());
            format!("Delete {}", shorten_path(&file_path))
        }
        Some(acp::ToolKind::Move) => {
            let from = raw_input
                .and_then(|i| i.get("from").or_else(|| i.get("source")))
                .and_then(|v| v.as_str());
            let to = raw_input
                .and_then(|i| i.get("to").or_else(|| i.get("destination")))
                .and_then(|v| v.as_str());
            match (from, to) {
                (Some(f), Some(t)) => {
                    format!("Move {} → {}", shorten_path(f), shorten_path(t))
                }
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
        _ => {
            // Check title for write-like operations
            let title_lower = title.to_lowercase();
            if title_lower.contains("write") && raw_input.and_then(|i| i.get("content")).is_some() {
                let file_path =
                    extract_file_path(raw_input).unwrap_or_else(|| "file".to_string());
                let line_count = count_content_lines(raw_input);
                format!(
                    "Write {} ({} line{})",
                    shorten_path(&file_path),
                    line_count,
                    if line_count == 1 { "" } else { "s" }
                )
            } else {
                format!("ACP agent requests permission to use: {}", title)
            }
        }
    };

    Some(reason)
}

// ============================================================================
// Helper functions for formatting
// ============================================================================

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
            let old = input
                .get("old_string")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let new = input
                .get("new_string")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            (old.lines().count().max(1), new.lines().count().max(1))
        })
        .unwrap_or((1, 1))
}

/// Count lines in content field for write operations.
fn count_content_lines(raw_input: Option<&serde_json::Value>) -> usize {
    raw_input
        .and_then(|i| i.get("content"))
        .and_then(|v| v.as_str())
        .map(|c| c.lines().count().max(1))
        .unwrap_or(1)
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

/// Translate a Codex ReviewDecision to an ACP RequestPermissionOutcome.
///
/// This maps the binary approve/deny decision to ACP's option-based model.
/// Uses the PermissionOptionKind to find the appropriate option.
pub fn review_decision_to_permission_outcome(
    decision: codex_protocol::protocol::ReviewDecision,
    options: &[acp::PermissionOption],
) -> acp::RequestPermissionOutcome {
    use codex_protocol::protocol::ReviewDecision;

    // Find the appropriate option based on the decision
    let option_id = match decision {
        ReviewDecision::Approved | ReviewDecision::ApprovedForSession => {
            // Look for an "Allow" kind option (AllowOnce or AllowAlways)
            options
                .iter()
                .find(|opt| {
                    matches!(
                        opt.kind,
                        acp::PermissionOptionKind::AllowOnce
                            | acp::PermissionOptionKind::AllowAlways
                    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::protocol::ReviewDecision;

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
        // Command should be formatted with the command arg (e.g., "shell(ls -la)")
        assert!(
            event.command[0].contains("shell") || event.command[0].contains("ls -la"),
            "Expected command to contain shell or ls -la, got: {:?}",
            event.command
        );
        assert!(event.reason.is_some());
    }

    #[test]
    fn test_review_decision_to_permission_outcome_approved() {
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

        let outcome = review_decision_to_permission_outcome(ReviewDecision::Approved, &options);
        assert!(matches!(
            outcome,
            acp::RequestPermissionOutcome::Selected { .. }
        ));
    }

    #[test]
    fn test_review_decision_to_permission_outcome_denied() {
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
        assert!(matches!(
            outcome,
            acp::RequestPermissionOutcome::Selected { .. }
        ));
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

    // ==================== Formatting Function Tests ====================

    #[test]
    fn test_shorten_path() {
        assert_eq!(shorten_path("/home/user/project/src/main.rs"), "main.rs");
        assert_eq!(shorten_path("src/lib.rs"), "lib.rs");
        assert_eq!(shorten_path("file.txt"), "file.txt");
        assert_eq!(shorten_path("/"), "/");
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("short", 10), "short");
        assert_eq!(truncate_str("this is a long string", 10), "this is...");
        assert_eq!(truncate_str("exactly10!", 10), "exactly10!");
    }

    #[test]
    fn test_count_edit_lines() {
        let input = serde_json::json!({
            "old_string": "line1\nline2\nline3",
            "new_string": "line1\nline2\nline3\nline4"
        });
        let (old, new) = count_edit_lines(Some(&input));
        assert_eq!(old, 3);
        assert_eq!(new, 4);
    }

    #[test]
    fn test_extract_file_path() {
        let input = serde_json::json!({"file_path": "/path/to/file.rs"});
        assert_eq!(
            extract_file_path(Some(&input)),
            Some("/path/to/file.rs".to_string())
        );

        let input2 = serde_json::json!({"path": "/other/path.txt"});
        assert_eq!(
            extract_file_path(Some(&input2)),
            Some("/other/path.txt".to_string())
        );

        assert_eq!(extract_file_path(None), None);
    }

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
    fn test_format_execute_command() {
        let input = serde_json::json!({"command": "git status"});
        let cmd = format_execute_command("Terminal", Some(&input));
        assert_eq!(cmd, vec!["git status"]);

        let cmd_empty = format_execute_command("Terminal", None);
        assert_eq!(cmd_empty, vec!["Terminal"]);
    }

    #[test]
    fn test_format_write_command() {
        let input = serde_json::json!({
            "file_path": "/path/to/new_file.rs",
            "content": "line1\nline2\nline3"
        });

        let cmd = format_write_command(Some(&input));
        assert_eq!(cmd[0], "Write new_file.rs (3 lines)");
    }

    #[test]
    fn test_format_delete_command() {
        let input = serde_json::json!({"path": "/tmp/old_file.txt"});
        let cmd = format_delete_command(Some(&input));
        assert_eq!(cmd, vec!["Delete old_file.txt"]);
    }

    #[test]
    fn test_format_move_command() {
        let input = serde_json::json!({
            "from": "/src/old.rs",
            "to": "/src/new.rs"
        });
        let cmd = format_move_command(Some(&input));
        assert_eq!(cmd, vec!["Move old.rs → new.rs"]);
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
        assert!(reason.is_some());
        let r = reason.unwrap();
        assert!(r.contains("Edit main.rs"));
        assert!(r.contains("replace 2 lines with 3 lines"));
    }

    #[test]
    fn test_extract_reason_execute() {
        let tool_call = acp::ToolCallUpdate::new(
            acp::ToolCallId::from("call-2".to_string()),
            acp::ToolCallUpdateFields::new()
                .title("Terminal")
                .kind(acp::ToolKind::Execute)
                .raw_input(serde_json::json!({"command": "cargo build --release"})),
        );

        let reason = extract_reason_from_tool_call(&tool_call);
        assert!(reason.is_some());
        let r = reason.unwrap();
        assert!(r.contains("Execute:"));
        assert!(r.contains("cargo build --release"));
    }

    #[test]
    fn test_extract_reason_unknown_tool() {
        let tool_call = acp::ToolCallUpdate::new(
            acp::ToolCallId::from("call-3".to_string()),
            acp::ToolCallUpdateFields::new().title("CustomTool"),
        );

        let reason = extract_reason_from_tool_call(&tool_call);
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("CustomTool"));
    }

    #[test]
    fn test_permission_request_edit_formatting() {
        let tool_call = acp::ToolCallUpdate::new(
            acp::ToolCallId::from("call-edit".to_string()),
            acp::ToolCallUpdateFields::new()
                .status(acp::ToolCallStatus::InProgress)
                .title("Edit")
                .kind(acp::ToolKind::Edit)
                .raw_input(serde_json::json!({
                    "file_path": "/home/user/src/lib.rs",
                    "old_string": "fn foo() {}",
                    "new_string": "fn foo() {\n    bar();\n}"
                })),
        );

        let request = acp::RequestPermissionRequest::new(
            acp::SessionId::from("session-1".to_string()),
            tool_call,
            vec![],
        );

        let cwd = std::path::Path::new("/home/user");
        let event = permission_request_to_approval_event(&request, cwd);

        // Command should show "Edit lib.rs" not raw JSON
        assert!(event.command[0].contains("Edit lib.rs"));
        // Command should include diff preview
        assert!(event.command.len() >= 2);
        assert!(event.command[1].contains("--- old"));

        // Reason should be descriptive
        let reason = event.reason.unwrap();
        assert!(reason.contains("lib.rs"));
        assert!(reason.contains("replace"));
    }
}
