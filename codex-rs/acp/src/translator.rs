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
fn extract_command_from_tool_call(tool_call: &acp::ToolCallUpdate) -> Vec<String> {
    // The tool call contains the tool title and raw_input in fields
    let mut cmd = Vec::new();

    // Use title as the command name if available
    if let Some(title) = &tool_call.fields.title {
        cmd.push(title.to_string());
    } else {
        cmd.push(tool_call.tool_call_id.to_string());
    }

    // Add stringified raw_input if present
    if let Some(input) = &tool_call.fields.raw_input
        && let Ok(args_str) = serde_json::to_string(input)
    {
        cmd.push(args_str);
    }

    cmd
}

/// Extract a human-readable reason from the tool call.
fn extract_reason_from_tool_call(tool_call: &acp::ToolCallUpdate) -> Option<String> {
    // Use the title as a basic description, or fall back to ID
    let name = tool_call.fields.title.as_deref().unwrap_or("unknown tool");
    Some(format!("ACP agent requests permission to use: {name}"))
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
        assert!(event.command.contains(&"shell".to_string()));
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
}
