use super::*;

/// Test that translate_session_update_to_events correctly translates
/// AgentMessageChunk to AgentMessageDelta events.
#[test]
fn test_translate_agent_message_chunk_to_event() {
    let update = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
        acp::ContentBlock::Text(acp::TextContent::new("Hello from agent")),
    ));

    let mut pending = std::collections::HashMap::new();
    let mut pending_tool_calls = std::collections::HashMap::new();
    let events = translate_session_update_to_events(&update, &mut pending, &mut pending_tool_calls);
    assert_eq!(events.len(), 1);

    match &events[0] {
        EventMsg::AgentMessageDelta(delta) => {
            assert_eq!(delta.delta, "Hello from agent");
        }
        _ => panic!("Expected AgentMessageDelta event"),
    }
}

/// Test that "prompt is too long" errors are correctly categorized
#[test]
fn test_categorize_acp_error_prompt_too_long() {
    assert_eq!(
        categorize_acp_error("Internal error: Prompt is too long"),
        AcpErrorCategory::PromptTooLong
    );
    assert_eq!(
        categorize_acp_error("Error code -32603: Internal error: Prompt is too long"),
        AcpErrorCategory::PromptTooLong
    );
    assert_eq!(
        categorize_acp_error("prompt is too long"),
        AcpErrorCategory::PromptTooLong
    );
    // Case insensitive
    assert_eq!(
        categorize_acp_error("PROMPT IS TOO LONG"),
        AcpErrorCategory::PromptTooLong
    );
}

/// Test that enhanced_error_message for PromptTooLong suggests /compact
#[test]
fn test_enhanced_error_message_prompt_too_long() {
    use crate::registry::AgentKind;

    let enhanced = enhanced_error_message(
        AcpErrorCategory::PromptTooLong,
        "Internal error: Prompt is too long",
        "Claude Code",
        AgentKind::ClaudeCode.auth_hint(),
        AgentKind::ClaudeCode.display_name(),
        AgentKind::ClaudeCode.npm_package(),
    );

    assert!(
        enhanced.contains("/compact"),
        "PromptTooLong message should suggest /compact, got: {enhanced}"
    );
}

/// Test that a wrapped "prompt is too long" error produces an actionable user message
/// when processed through the full categorize + format path (simulating what
/// handle_user_input does on prompt failure).
#[test]
fn test_prompt_too_long_error_produces_actionable_message() {
    // Simulate the error chain: acp library error -> .context("ACP prompt failed")
    let inner = anyhow::anyhow!("Internal error: Prompt is too long");
    let wrapped: anyhow::Error = inner.context("ACP prompt failed");

    // This is what categorize_and_handle_prompt_error does in backend/mod.rs:
    let error_string = format!("{wrapped:?}");
    let category = categorize_acp_error(&error_string);

    assert_eq!(
        category,
        AcpErrorCategory::PromptTooLong,
        "Debug-formatted error chain should be categorized as PromptTooLong"
    );
}

/// Test that translate_session_update_to_events correctly translates
/// AgentThoughtChunk to AgentReasoningDelta events.
#[test]
fn test_translate_agent_thought_to_reasoning_event() {
    let update = acp::SessionUpdate::AgentThoughtChunk(acp::ContentChunk::new(
        acp::ContentBlock::Text(acp::TextContent::new("Thinking about the problem...")),
    ));

    let mut pending = std::collections::HashMap::new();
    let mut pending_tool_calls = std::collections::HashMap::new();
    let events = translate_session_update_to_events(&update, &mut pending, &mut pending_tool_calls);
    assert_eq!(events.len(), 1);

    match &events[0] {
        EventMsg::AgentReasoningDelta(delta) => {
            assert_eq!(delta.delta, "Thinking about the problem...");
        }
        _ => panic!("Expected AgentReasoningDelta event"),
    }
}

/// Test that ToolCall updates are translated to ExecCommandBegin events.
#[test]
fn test_translate_tool_call_to_exec_command_begin() {
    let update = acp::SessionUpdate::ToolCall(
        acp::ToolCall::new(acp::ToolCallId::from("call-123".to_string()), "shell")
            .kind(acp::ToolKind::Execute)
            .status(acp::ToolCallStatus::InProgress)
            .raw_input(serde_json::json!({"command": "ls -la"})),
    );

    let mut pending = std::collections::HashMap::new();
    let mut pending_tool_calls = std::collections::HashMap::new();
    let events = translate_session_update_to_events(&update, &mut pending, &mut pending_tool_calls);
    assert_eq!(events.len(), 1);

    match &events[0] {
        EventMsg::ExecCommandBegin(begin) => {
            assert_eq!(begin.call_id, "call-123");
            // Command now includes formatted arguments
            assert_eq!(begin.command[0], "shell(ls -la)");
        }
        _ => panic!("Expected ExecCommandBegin event"),
    }
}

/// Test that completed ToolCallUpdate is translated to ExecCommandEnd.
#[test]
fn test_translate_tool_call_update_completed_to_exec_command_end() {
    let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        acp::ToolCallId::from("call-456".to_string()),
        acp::ToolCallUpdateFields::new()
            .status(acp::ToolCallStatus::Completed)
            .title("read_file"),
    ));

    let mut pending = std::collections::HashMap::new();
    let mut pending_tool_calls = std::collections::HashMap::new();
    let events = translate_session_update_to_events(&update, &mut pending, &mut pending_tool_calls);
    assert_eq!(events.len(), 1);

    match &events[0] {
        EventMsg::ExecCommandEnd(end) => {
            assert_eq!(end.call_id, "call-456");
        }
        _ => panic!("Expected ExecCommandEnd event"),
    }
}

/// Test that ToolCallUpdate with content extracts the output text.
#[test]
fn test_extract_tool_output_from_content() {
    let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        acp::ToolCallId::from("call-789".to_string()),
        acp::ToolCallUpdateFields::new()
            .status(acp::ToolCallStatus::Completed)
            .title("read_file")
            .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                acp::ContentBlock::Text(acp::TextContent::new("File contents here")),
            ))]),
    ));

    let mut pending = std::collections::HashMap::new();
    let mut pending_tool_calls = std::collections::HashMap::new();
    let events = translate_session_update_to_events(&update, &mut pending, &mut pending_tool_calls);
    assert_eq!(events.len(), 1);

    match &events[0] {
        EventMsg::ExecCommandEnd(end) => {
            assert_eq!(end.aggregated_output, "File contents here");
            assert_eq!(
                end.formatted_output, end.aggregated_output,
                "formatted_output should match aggregated_output for ACP tool calls"
            );
        }
        _ => panic!("Expected ExecCommandEnd event"),
    }
}

/// Test that ToolCallUpdate with raw_output extracts meaningful info.
#[test]
fn test_extract_tool_output_from_raw_output() {
    let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        acp::ToolCallId::from("call-read".to_string()),
        acp::ToolCallUpdateFields::new()
            .status(acp::ToolCallStatus::Completed)
            .title("read_file")
            .raw_output(serde_json::json!({"lines": 42})),
    ));

    let mut pending = std::collections::HashMap::new();
    let mut pending_tool_calls = std::collections::HashMap::new();
    let events = translate_session_update_to_events(&update, &mut pending, &mut pending_tool_calls);
    match &events[0] {
        EventMsg::ExecCommandEnd(end) => {
            assert_eq!(end.aggregated_output, "Read 42 lines");
            assert_eq!(
                end.formatted_output, end.aggregated_output,
                "formatted_output should match aggregated_output for ACP tool calls"
            );
        }
        _ => panic!("Expected ExecCommandEnd event"),
    }
}

/// Test that tool command is formatted with path argument.
#[test]
fn test_format_tool_call_command_with_path() {
    let cmd = format_tool_call_command(
        "Read File",
        Some(&serde_json::json!({"path": "src/main.rs"})),
    );
    assert_eq!(cmd, "Read File(src/main.rs)");
}

/// Test that shell command is formatted with command argument.
#[test]
fn test_format_tool_call_command_shell() {
    let cmd = format_tool_call_command(
        "Terminal",
        Some(&serde_json::json!({"command": "git status"})),
    );
    assert_eq!(cmd, "Terminal(git status)");
}

/// Test that search command is formatted with pattern and path.
#[test]
fn test_format_tool_call_command_search() {
    let cmd = format_tool_call_command(
        "Find Files",
        Some(&serde_json::json!({"pattern": "*.rs", "path": "src/"})),
    );
    assert_eq!(cmd, "Find Files(*.rs in src/)");
}

/// Test that duplicate args are not appended when title equals the command.
#[test]
fn test_format_tool_call_command_no_duplicate_when_title_equals_args() {
    let cmd = format_tool_call_command(
        "git diff HEAD",
        Some(&serde_json::json!({"command": "git diff HEAD"})),
    );
    assert_eq!(cmd, "git diff HEAD");
}

/// Test that duplicate args are not appended when title contains the command.
#[test]
fn test_format_tool_call_command_no_duplicate_when_title_contains_args() {
    let cmd = format_tool_call_command(
        "Running: git status",
        Some(&serde_json::json!({"command": "git status"})),
    );
    assert_eq!(cmd, "Running: git status");
}

/// Test that similar but different commands are still formatted with suffix.
#[test]
fn test_format_tool_call_command_partial_overlap_not_duplicate() {
    // "git diff HEAD" is NOT a substring of "git diff", so we should append
    let cmd = format_tool_call_command(
        "git diff",
        Some(&serde_json::json!({"command": "git diff HEAD"})),
    );
    assert_eq!(cmd, "git diff(git diff HEAD)");
}

/// Test that non-text content blocks produce no events.
#[test]
fn test_non_text_content_produces_no_events() {
    let update = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
        acp::ContentBlock::Image(acp::ImageContent::new(String::new(), "image/png")),
    ));

    let mut pending = std::collections::HashMap::new();
    let mut pending_tool_calls = std::collections::HashMap::new();
    let events = translate_session_update_to_events(&update, &mut pending, &mut pending_tool_calls);
    assert!(events.is_empty());
}
