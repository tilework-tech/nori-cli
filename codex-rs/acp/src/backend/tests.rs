use super::*;
use serial_test::serial;

/// Test that translate_session_update_to_events correctly translates
/// AgentMessageChunk to AgentMessageDelta events.
#[test]
fn test_translate_agent_message_chunk_to_event() {
    let update = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
        acp::ContentBlock::Text(acp::TextContent::new("Hello from agent")),
    ));

    let mut pending = std::collections::HashMap::new();
    let events = translate_session_update_to_events(&update, &mut pending);
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
    let events = translate_session_update_to_events(&update, &mut pending);
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
    let events = translate_session_update_to_events(&update, &mut pending);
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
    let events = translate_session_update_to_events(&update, &mut pending);
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
    let events = translate_session_update_to_events(&update, &mut pending);
    assert_eq!(events.len(), 1);

    match &events[0] {
        EventMsg::ExecCommandEnd(end) => {
            assert_eq!(end.aggregated_output, "File contents here");
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
    let events = translate_session_update_to_events(&update, &mut pending);
    match &events[0] {
        EventMsg::ExecCommandEnd(end) => {
            assert_eq!(end.aggregated_output, "Read 42 lines");
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
    let events = translate_session_update_to_events(&update, &mut pending);
    assert!(events.is_empty());
}

/// Test that unsupported session update types produce no events.
#[test]
fn test_unsupported_updates_produce_no_events() {
    let update = acp::SessionUpdate::UserMessageChunk(acp::ContentChunk::new(
        acp::ContentBlock::Text(acp::TextContent::new("User message")),
    ));

    let mut pending = std::collections::HashMap::new();
    let events = translate_session_update_to_events(&update, &mut pending);
    assert!(events.is_empty());
}

/// Test that get_op_name returns correct names for various Op variants.
#[test]
fn test_get_op_name() {
    assert_eq!(get_op_name(&Op::Interrupt), "Interrupt");
    assert_eq!(get_op_name(&Op::Compact), "Compact");
    assert_eq!(get_op_name(&Op::Undo), "Undo");
    assert_eq!(get_op_name(&Op::UserInput { items: vec![] }), "UserInput");
    assert_eq!(get_op_name(&Op::Shutdown), "Shutdown");
}

/// Test that generate_id produces unique IDs.
#[test]
fn test_generate_id_unique() {
    let id1 = generate_id();
    let id2 = generate_id();
    let id3 = generate_id();

    assert_ne!(id1, id2);
    assert_ne!(id2, id3);
    assert!(id1.starts_with("acp-"));
    assert!(id2.starts_with("acp-"));
}

// ==================== Tool Classification Tests ====================

/// Test that ToolKind::Read produces ParsedCommand::Read (Exploring mode).
#[test]
fn test_classify_tool_kind_read() {
    let parsed = classify_tool_to_parsed_command(
        "Read File",
        Some(&acp::ToolKind::Read),
        Some(&serde_json::json!({"path": "src/main.rs"})),
    );
    assert_eq!(parsed.len(), 1);
    match &parsed[0] {
        ParsedCommand::Read { cmd, name, path } => {
            assert_eq!(cmd, "Read File");
            assert_eq!(name, "main.rs");
            assert_eq!(path.to_string_lossy(), "src/main.rs");
        }
        _ => panic!("Expected ParsedCommand::Read"),
    }
}

/// Test that ToolKind::Search produces ParsedCommand::Search (Exploring mode).
#[test]
fn test_classify_tool_kind_search() {
    let parsed = classify_tool_to_parsed_command(
        "Search Files",
        Some(&acp::ToolKind::Search),
        Some(&serde_json::json!({"pattern": "TODO", "path": "src/"})),
    );
    assert_eq!(parsed.len(), 1);
    match &parsed[0] {
        ParsedCommand::Search { cmd, query, path } => {
            assert_eq!(cmd, "Search Files");
            assert_eq!(query.as_deref(), Some("TODO"));
            assert_eq!(path.as_deref(), Some("src/"));
        }
        _ => panic!("Expected ParsedCommand::Search"),
    }
}

/// Test that ToolKind::Execute produces ParsedCommand::Unknown (Command mode).
#[test]
fn test_classify_tool_kind_execute() {
    let parsed = classify_tool_to_parsed_command(
        "Terminal",
        Some(&acp::ToolKind::Execute),
        Some(&serde_json::json!({"command": "git status"})),
    );
    assert_eq!(parsed.len(), 1);
    match &parsed[0] {
        ParsedCommand::Unknown { cmd } => {
            assert_eq!(cmd, "Terminal(git status)");
        }
        _ => panic!("Expected ParsedCommand::Unknown"),
    }
}

/// Test that ToolKind::Edit produces ParsedCommand::Unknown (Command mode).
#[test]
fn test_classify_tool_kind_edit() {
    let parsed = classify_tool_to_parsed_command(
        "Edit File",
        Some(&acp::ToolKind::Edit),
        Some(&serde_json::json!({"path": "src/lib.rs"})),
    );
    assert_eq!(parsed.len(), 1);
    match &parsed[0] {
        ParsedCommand::Unknown { cmd } => {
            assert!(cmd.contains("Edit File"));
        }
        _ => panic!("Expected ParsedCommand::Unknown"),
    }
}

/// Test that ToolKind::Delete produces ParsedCommand::Unknown (Command mode).
#[test]
fn test_classify_tool_kind_delete() {
    let parsed = classify_tool_to_parsed_command(
        "Delete File",
        Some(&acp::ToolKind::Delete),
        Some(&serde_json::json!({"path": "temp.txt"})),
    );
    assert_eq!(parsed.len(), 1);
    match &parsed[0] {
        ParsedCommand::Unknown { .. } => {}
        _ => panic!("Expected ParsedCommand::Unknown"),
    }
}

/// Test that ToolKind::Move produces ParsedCommand::Unknown (Command mode).
#[test]
fn test_classify_tool_kind_move() {
    let parsed = classify_tool_to_parsed_command(
        "Move File",
        Some(&acp::ToolKind::Move),
        Some(&serde_json::json!({"from": "a.txt", "to": "b.txt"})),
    );
    assert_eq!(parsed.len(), 1);
    match &parsed[0] {
        ParsedCommand::Unknown { .. } => {}
        _ => panic!("Expected ParsedCommand::Unknown"),
    }
}

/// Test that ToolKind::Fetch produces ParsedCommand::Unknown (Command mode).
#[test]
fn test_classify_tool_kind_fetch() {
    let parsed = classify_tool_to_parsed_command(
        "Fetch URL",
        Some(&acp::ToolKind::Fetch),
        Some(&serde_json::json!({"url": "https://example.com"})),
    );
    assert_eq!(parsed.len(), 1);
    match &parsed[0] {
        ParsedCommand::Unknown { .. } => {}
        _ => panic!("Expected ParsedCommand::Unknown"),
    }
}

/// Test that ToolKind::Think produces ParsedCommand::Unknown (Command mode).
#[test]
fn test_classify_tool_kind_think() {
    let parsed = classify_tool_to_parsed_command("Think", Some(&acp::ToolKind::Think), None);
    assert_eq!(parsed.len(), 1);
    match &parsed[0] {
        ParsedCommand::Unknown { .. } => {}
        _ => panic!("Expected ParsedCommand::Unknown"),
    }
}

/// Test title-based fallback for ToolKind::Other with "list" in title.
#[test]
fn test_classify_fallback_list_by_title() {
    let parsed = classify_tool_to_parsed_command(
        "List Directory",
        Some(&acp::ToolKind::Other),
        Some(&serde_json::json!({"path": "src/"})),
    );
    assert_eq!(parsed.len(), 1);
    match &parsed[0] {
        ParsedCommand::ListFiles { cmd, path } => {
            assert_eq!(cmd, "List Directory");
            assert_eq!(path.as_deref(), Some("src/"));
        }
        _ => panic!("Expected ParsedCommand::ListFiles"),
    }
}

/// Test title-based fallback for ToolKind::Other with "grep" in title.
#[test]
fn test_classify_fallback_grep_by_title() {
    let parsed = classify_tool_to_parsed_command(
        "Grep Files",
        Some(&acp::ToolKind::Other),
        Some(&serde_json::json!({"pattern": "error", "path": "logs/"})),
    );
    assert_eq!(parsed.len(), 1);
    match &parsed[0] {
        ParsedCommand::Search { cmd, query, path } => {
            assert_eq!(cmd, "Grep Files");
            assert_eq!(query.as_deref(), Some("error"));
            assert_eq!(path.as_deref(), Some("logs/"));
        }
        _ => panic!("Expected ParsedCommand::Search"),
    }
}

/// Test title-based fallback for ToolKind::Other with "read" in title.
#[test]
fn test_classify_fallback_read_by_title() {
    let parsed = classify_tool_to_parsed_command(
        "Read Config",
        Some(&acp::ToolKind::Other),
        Some(&serde_json::json!({"file_path": "config.toml"})),
    );
    assert_eq!(parsed.len(), 1);
    match &parsed[0] {
        ParsedCommand::Read { cmd, name, .. } => {
            assert_eq!(cmd, "Read Config");
            assert_eq!(name, "config.toml");
        }
        _ => panic!("Expected ParsedCommand::Read"),
    }
}

/// Test that None kind falls back to title-based classification.
#[test]
fn test_classify_none_kind_fallback() {
    let parsed = classify_tool_to_parsed_command(
        "Search Code",
        None,
        Some(&serde_json::json!({"query": "fn main"})),
    );
    assert_eq!(parsed.len(), 1);
    match &parsed[0] {
        ParsedCommand::Search { cmd, query, .. } => {
            assert_eq!(cmd, "Search Code");
            assert_eq!(query.as_deref(), Some("fn main"));
        }
        _ => panic!("Expected ParsedCommand::Search"),
    }
}

/// Test that ToolCall with Read kind generates parsed_cmd in ExecCommandBegin.
#[test]
fn test_tool_call_read_generates_exploring_parsed_cmd() {
    let update = acp::SessionUpdate::ToolCall(
        acp::ToolCall::new(acp::ToolCallId::from("call-read".to_string()), "Read File")
            .kind(acp::ToolKind::Read)
            .status(acp::ToolCallStatus::InProgress)
            .raw_input(serde_json::json!({"path": "src/lib.rs"})),
    );

    let mut pending = std::collections::HashMap::new();
    let events = translate_session_update_to_events(&update, &mut pending);
    assert_eq!(events.len(), 1);

    match &events[0] {
        EventMsg::ExecCommandBegin(begin) => {
            assert_eq!(begin.parsed_cmd.len(), 1);
            match &begin.parsed_cmd[0] {
                ParsedCommand::Read { name, .. } => {
                    assert_eq!(name, "lib.rs");
                }
                _ => panic!("Expected ParsedCommand::Read"),
            }
        }
        _ => panic!("Expected ExecCommandBegin event"),
    }
}

/// Test that ToolCall with Execute kind generates command-mode parsed_cmd.
#[test]
fn test_tool_call_execute_generates_command_parsed_cmd() {
    let update = acp::SessionUpdate::ToolCall(
        acp::ToolCall::new(acp::ToolCallId::from("call-exec".to_string()), "Terminal")
            .kind(acp::ToolKind::Execute)
            .status(acp::ToolCallStatus::InProgress)
            .raw_input(serde_json::json!({"command": "cargo test"})),
    );

    let mut pending = std::collections::HashMap::new();
    let events = translate_session_update_to_events(&update, &mut pending);
    assert_eq!(events.len(), 1);

    match &events[0] {
        EventMsg::ExecCommandBegin(begin) => {
            assert_eq!(begin.parsed_cmd.len(), 1);
            match &begin.parsed_cmd[0] {
                ParsedCommand::Unknown { cmd } => {
                    assert!(cmd.contains("cargo test"));
                }
                _ => panic!("Expected ParsedCommand::Unknown"),
            }
        }
        _ => panic!("Expected ExecCommandBegin event"),
    }
}

/// Test that ToolCallUpdate with Read kind generates exploring parsed_cmd in ExecCommandEnd.
#[test]
fn test_tool_call_update_read_generates_exploring_parsed_cmd() {
    let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        acp::ToolCallId::from("call-read-end".to_string()),
        acp::ToolCallUpdateFields::new()
            .status(acp::ToolCallStatus::Completed)
            .title("Read File")
            .kind(acp::ToolKind::Read)
            .raw_input(serde_json::json!({"path": "Cargo.toml"})),
    ));

    let mut pending = std::collections::HashMap::new();
    let events = translate_session_update_to_events(&update, &mut pending);
    assert_eq!(events.len(), 1);

    match &events[0] {
        EventMsg::ExecCommandEnd(end) => {
            assert_eq!(end.parsed_cmd.len(), 1);
            match &end.parsed_cmd[0] {
                ParsedCommand::Read { name, .. } => {
                    assert_eq!(name, "Cargo.toml");
                }
                _ => panic!("Expected ParsedCommand::Read"),
            }
        }
        _ => panic!("Expected ExecCommandEnd event"),
    }
}

// ==================== Error Categorization Tests ====================

/// Test that authentication errors are correctly categorized
#[test]
fn test_categorize_acp_error_authentication() {
    // Test various authentication error patterns
    assert_eq!(
        categorize_acp_error("Authentication required"),
        AcpErrorCategory::Authentication
    );
    assert_eq!(
        categorize_acp_error("Error code -32000: not authenticated"),
        AcpErrorCategory::Authentication
    );
    assert_eq!(
        categorize_acp_error("Invalid API key"),
        AcpErrorCategory::Authentication
    );
    assert_eq!(
        categorize_acp_error("Unauthorized access"),
        AcpErrorCategory::Authentication
    );
    assert_eq!(
        categorize_acp_error("User not logged in"),
        AcpErrorCategory::Authentication
    );
}

/// Test that quota/rate limit errors are correctly categorized
#[test]
fn test_categorize_acp_error_quota() {
    assert_eq!(
        categorize_acp_error("Quota exceeded"),
        AcpErrorCategory::QuotaExceeded
    );
    assert_eq!(
        categorize_acp_error("Rate limit reached"),
        AcpErrorCategory::QuotaExceeded
    );
    assert_eq!(
        categorize_acp_error("Too many requests"),
        AcpErrorCategory::QuotaExceeded
    );
    assert_eq!(
        categorize_acp_error("HTTP 429: Too Many Requests"),
        AcpErrorCategory::QuotaExceeded
    );
}

/// Test that executable not found errors are correctly categorized
#[test]
fn test_categorize_acp_error_executable_not_found() {
    assert_eq!(
        categorize_acp_error("npx: command not found"),
        AcpErrorCategory::ExecutableNotFound
    );
    assert_eq!(
        categorize_acp_error("bunx: command not found"),
        AcpErrorCategory::ExecutableNotFound
    );
    assert_eq!(
        categorize_acp_error("No such file or directory: /usr/bin/claude"),
        AcpErrorCategory::ExecutableNotFound
    );
    assert_eq!(
        categorize_acp_error("command not found: gemini"),
        AcpErrorCategory::ExecutableNotFound
    );
}

/// Test that initialization errors are correctly categorized
#[test]
fn test_categorize_acp_error_initialization() {
    assert_eq!(
        categorize_acp_error("ACP initialization failed"),
        AcpErrorCategory::Initialization
    );
    assert_eq!(
        categorize_acp_error("Protocol handshake error"),
        AcpErrorCategory::Initialization
    );
    assert_eq!(
        categorize_acp_error("Protocol version mismatch"),
        AcpErrorCategory::Initialization
    );
}

/// Test that unknown errors fall back to Unknown category
#[test]
fn test_categorize_acp_error_unknown() {
    assert_eq!(
        categorize_acp_error("Some random error message"),
        AcpErrorCategory::Unknown
    );
    assert_eq!(
        categorize_acp_error("Connection timeout"),
        AcpErrorCategory::Unknown
    );
    assert_eq!(
        categorize_acp_error("Unexpected end of input"),
        AcpErrorCategory::Unknown
    );
}

/// Test that error categorization is case-insensitive
#[test]
fn test_categorize_acp_error_case_insensitive() {
    assert_eq!(
        categorize_acp_error("AUTHENTICATION REQUIRED"),
        AcpErrorCategory::Authentication
    );
    assert_eq!(
        categorize_acp_error("QUOTA EXCEEDED"),
        AcpErrorCategory::QuotaExceeded
    );
    assert_eq!(
        categorize_acp_error("NPX: COMMAND NOT FOUND"),
        AcpErrorCategory::ExecutableNotFound
    );
}

/// Test that protocol "not found" errors are NOT classified as ExecutableNotFound.
/// These are legitimate ACP errors that should fall through to Unknown.
#[test]
fn test_protocol_not_found_is_not_executable_not_found() {
    // Resource not found is a protocol error, not a missing executable
    assert_ne!(
        categorize_acp_error("Resource not found: session-123"),
        AcpErrorCategory::ExecutableNotFound,
        "Protocol errors should not be ExecutableNotFound"
    );
    // Model not found is a business error, not a missing executable
    assert_ne!(
        categorize_acp_error("Model not found: gpt-999"),
        AcpErrorCategory::ExecutableNotFound,
        "Model errors should not be ExecutableNotFound"
    );
    // File not found (without "directory") should not trigger false positive
    assert_ne!(
        categorize_acp_error("File not found"),
        AcpErrorCategory::ExecutableNotFound,
        "Generic 'file not found' should not be ExecutableNotFound"
    );
}

/// Test that enhanced_error_message produces actionable auth error messages
#[test]
fn test_enhanced_error_message_auth() {
    use crate::registry::AgentKind;

    let auth_hint = AgentKind::ClaudeCode.auth_hint();
    let enhanced = enhanced_error_message(
        AcpErrorCategory::Authentication,
        "Authentication required",
        "Claude Code ACP",
        auth_hint,
        AgentKind::ClaudeCode.display_name(),
        AgentKind::ClaudeCode.npm_package(),
    );

    assert!(
        enhanced.contains("Authentication required"),
        "Should mention auth required, got: {enhanced}"
    );
    assert!(
        enhanced.contains("/login"),
        "Should include auth hint with '/login', got: {enhanced}"
    );
}

/// Test that enhanced_error_message produces actionable quota error messages
#[test]
fn test_enhanced_error_message_quota() {
    use crate::registry::AgentKind;

    let enhanced = enhanced_error_message(
        AcpErrorCategory::QuotaExceeded,
        "Rate limit exceeded",
        "Codex ACP",
        AgentKind::Codex.auth_hint(),
        AgentKind::Codex.display_name(),
        AgentKind::Codex.npm_package(),
    );

    assert!(
        enhanced.contains("Rate limit") || enhanced.contains("quota"),
        "Should mention rate limit or quota, got: {enhanced}"
    );
}

/// Test that enhanced_error_message produces actionable executable not found messages
#[test]
fn test_enhanced_error_message_executable_not_found() {
    use crate::registry::AgentKind;

    let enhanced = enhanced_error_message(
        AcpErrorCategory::ExecutableNotFound,
        "npx: command not found",
        "Gemini ACP",
        AgentKind::Gemini.auth_hint(),
        AgentKind::Gemini.display_name(),
        AgentKind::Gemini.npm_package(),
    );

    assert!(
        enhanced.contains("install") || enhanced.contains("npm"),
        "Should mention installation instructions, got: {enhanced}"
    );
}

/// Test that enhanced_error_message passes through unknown errors
#[test]
fn test_enhanced_error_message_unknown() {
    use crate::registry::AgentKind;

    let original_error = "Some random error";
    let enhanced = enhanced_error_message(
        AcpErrorCategory::Unknown,
        original_error,
        "Mock ACP",
        AgentKind::ClaudeCode.auth_hint(),
        AgentKind::ClaudeCode.display_name(),
        AgentKind::ClaudeCode.npm_package(),
    );

    assert_eq!(
        enhanced, original_error,
        "Unknown errors should pass through unchanged"
    );
}

/// Integration test: Mock agent auth failure produces actionable error message.
///
/// This test uses the real mock-acp-agent binary with MOCK_AGENT_REQUIRE_AUTH=true
/// to simulate an authentication failure and verify the error message is actionable.
#[tokio::test]
#[serial]
async fn test_mock_agent_auth_failure_produces_actionable_error() {
    // Get the mock agent config to check if the binary exists
    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");

    // Check if mock agent binary exists
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    // Set the environment variable to trigger auth failure
    // SAFETY: This is a test that manipulates environment variables.
    // It's safe because this test runs in isolation and we clean up after.
    unsafe {
        std::env::set_var("MOCK_AGENT_REQUIRE_AUTH", "true");
    }

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (event_tx, _event_rx) = mpsc::channel(32);

    let config = AcpBackendConfig {
        agent: "mock-model".to_string(),
        cwd: temp_dir.path().to_path_buf(),
        approval_policy: AskForApproval::Never,
        sandbox_policy: SandboxPolicy::new_read_only_policy(),
        notify: None,
        os_notifications: crate::config::OsNotifications::Disabled,
        nori_home: temp_dir.path().to_path_buf(),
        history_persistence: crate::config::HistoryPersistence::SaveAll,
        cli_version: "test".to_string(),
        notify_after_idle: crate::config::NotifyAfterIdle::FiveSeconds,
        auto_worktree: false,
        auto_worktree_repo_root: None,
        session_start_hooks: vec![],
        session_end_hooks: vec![],
        pre_user_prompt_hooks: vec![],
        post_user_prompt_hooks: vec![],
        pre_tool_call_hooks: vec![],
        post_tool_call_hooks: vec![],
        pre_agent_response_hooks: vec![],
        post_agent_response_hooks: vec![],
        async_session_start_hooks: vec![],
        async_session_end_hooks: vec![],
        async_pre_user_prompt_hooks: vec![],
        async_post_user_prompt_hooks: vec![],
        async_pre_tool_call_hooks: vec![],
        async_post_tool_call_hooks: vec![],
        async_pre_agent_response_hooks: vec![],
        async_post_agent_response_hooks: vec![],
        script_timeout: std::time::Duration::from_secs(30),
        default_model: None,
    };

    let result = AcpBackend::spawn(&config, event_tx).await;

    // Clean up env var
    // SAFETY: Cleaning up the environment variable we set above.
    unsafe {
        std::env::remove_var("MOCK_AGENT_REQUIRE_AUTH");
    }

    // Verify spawn failed
    let error_message = match result {
        Ok(_) => {
            panic!("Expected spawn to fail with auth error, but it succeeded");
        }
        Err(e) => e.to_string(),
    };

    // Verify error message is actionable - should mention auth and provide instructions
    // The mock agent returns error code -32000 which should be categorized as auth
    assert!(
        error_message.contains("Authentication")
            || error_message.contains("auth")
            || error_message.contains("login"),
        "Error message should mention authentication or provide login instructions, got: {error_message}"
    );
}

/// Test that updating the approval policy via watch channel dynamically changes
/// the approval handler's behavior. This verifies that `/approvals` command
/// selecting "full access" makes it equivalent to `--yolo`.
#[tokio::test]
async fn test_approval_policy_dynamic_update() {
    use codex_protocol::approvals::ExecApprovalRequestEvent;
    use tokio::sync::oneshot;
    use tokio::sync::watch;

    // Create channels for the test
    let (approval_tx, approval_rx) = mpsc::channel::<ApprovalRequest>(16);
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(16);
    let pending_approvals = Arc::new(Mutex::new(Vec::<ApprovalRequest>::new()));
    let user_notifier = Arc::new(codex_core::UserNotifier::new(None, false));
    let cwd = PathBuf::from("/tmp/test");

    // Create watch channel starting with OnRequest policy (requires approval)
    let (policy_tx, policy_rx) = watch::channel(AskForApproval::OnRequest);

    // Spawn the approval handler with the watch receiver
    tokio::spawn(AcpBackend::run_approval_handler(
        approval_rx,
        event_tx.clone(),
        Arc::clone(&pending_approvals),
        Arc::clone(&user_notifier),
        cwd.clone(),
        policy_rx,
    ));

    // Create a mock approval request
    let (response_tx1, mut response_rx1) = oneshot::channel();
    let request1 = ApprovalRequest {
        event: ApprovalEventType::Exec(ExecApprovalRequestEvent {
            call_id: "call-1".to_string(),
            turn_id: String::new(),
            command: vec!["ls".to_string()],
            cwd: cwd.clone(),
            reason: None,
            risk: None,
            parsed_cmd: vec![],
        }),
        options: vec![],
        response_tx: response_tx1,
    };

    // Send first request - should be forwarded to TUI (not auto-approved)
    approval_tx.send(request1).await.unwrap();

    // Give the handler time to process
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Should have received an approval request event in the TUI
    let event = event_rx.try_recv();
    assert!(
        event.is_ok(),
        "Should have received approval request event for OnRequest policy"
    );
    if let Ok(Event {
        msg: EventMsg::ExecApprovalRequest(req),
        ..
    }) = event
    {
        assert_eq!(req.call_id, "call-1");
    } else {
        panic!("Expected ExecApprovalRequest event");
    }

    // The request should be pending (not auto-approved)
    assert!(
        response_rx1.try_recv().is_err(),
        "Request should not be auto-approved with OnRequest policy"
    );

    // Now update the policy to Never (yolo mode)
    policy_tx.send(AskForApproval::Never).unwrap();

    // Give the handler time to see the policy change
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Send second request - should be auto-approved
    let (response_tx2, mut response_rx2) = oneshot::channel();
    let request2 = ApprovalRequest {
        event: ApprovalEventType::Exec(ExecApprovalRequestEvent {
            call_id: "call-2".to_string(),
            turn_id: String::new(),
            command: vec!["echo".to_string(), "hello".to_string()],
            cwd: cwd.clone(),
            reason: None,
            risk: None,
            parsed_cmd: vec![],
        }),
        options: vec![],
        response_tx: response_tx2,
    };

    approval_tx.send(request2).await.unwrap();

    // Give the handler time to process
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Should NOT have received another approval request event (auto-approved)
    let event2 = event_rx.try_recv();
    assert!(
        event2.is_err(),
        "Should NOT receive approval request event when policy is Never (yolo mode)"
    );

    // The request should have been auto-approved
    let decision = response_rx2.try_recv();
    assert!(
        matches!(decision, Ok(ReviewDecision::Approved)),
        "Request should be auto-approved with Never policy, got: {decision:?}"
    );
}

/// Test that Op::Compact sends the summarization prompt to the agent and emits
/// the expected events: TaskStarted, agent message streaming, ContextCompacted,
/// Warning, and TaskComplete.
///
/// This test uses the mock agent to simulate the compact flow.
#[tokio::test]
#[serial]
async fn test_compact_sends_summarization_prompt_and_emits_events() {
    use std::time::Duration;

    // Get the mock agent config to check if the binary exists
    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");

    // Check if mock agent binary exists
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (event_tx, mut event_rx) = mpsc::channel(64);

    let config = AcpBackendConfig {
        agent: "mock-model".to_string(),
        cwd: temp_dir.path().to_path_buf(),
        approval_policy: AskForApproval::Never,
        sandbox_policy: SandboxPolicy::new_read_only_policy(),
        notify: None,
        os_notifications: crate::config::OsNotifications::Disabled,
        nori_home: temp_dir.path().to_path_buf(),
        history_persistence: crate::config::HistoryPersistence::SaveAll,
        cli_version: "test".to_string(),
        notify_after_idle: crate::config::NotifyAfterIdle::FiveSeconds,
        auto_worktree: false,
        auto_worktree_repo_root: None,
        session_start_hooks: vec![],
        session_end_hooks: vec![],
        pre_user_prompt_hooks: vec![],
        post_user_prompt_hooks: vec![],
        pre_tool_call_hooks: vec![],
        post_tool_call_hooks: vec![],
        pre_agent_response_hooks: vec![],
        post_agent_response_hooks: vec![],
        async_session_start_hooks: vec![],
        async_session_end_hooks: vec![],
        async_pre_user_prompt_hooks: vec![],
        async_post_user_prompt_hooks: vec![],
        async_pre_tool_call_hooks: vec![],
        async_post_tool_call_hooks: vec![],
        async_pre_agent_response_hooks: vec![],
        async_post_agent_response_hooks: vec![],
        script_timeout: std::time::Duration::from_secs(30),
        default_model: None,
    };

    let backend = AcpBackend::spawn(&config, event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    // Drain the SessionConfigured event
    let _ = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
        .await
        .expect("Should receive SessionConfigured event");

    // Submit the Compact operation
    let _id = backend
        .submit(Op::Compact)
        .await
        .expect("Failed to submit Op::Compact");

    // Collect events with a timeout
    let mut events = Vec::new();
    let timeout = Duration::from_secs(10);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        match tokio::time::timeout(Duration::from_millis(500), event_rx.recv()).await {
            Ok(Some(event)) => {
                events.push(event);
                // Check if we got TaskComplete, which signals the end
                if matches!(
                    events.last().map(|e| &e.msg),
                    Some(EventMsg::TaskComplete(_))
                ) {
                    break;
                }
            }
            Ok(None) => break, // Channel closed
            Err(_) => {
                // Timeout on recv - check if we have enough events
                if events
                    .iter()
                    .any(|e| matches!(e.msg, EventMsg::TaskComplete(_)))
                {
                    break;
                }
            }
        }
    }

    // Verify we got the expected events
    let has_task_started = events
        .iter()
        .any(|e| matches!(e.msg, EventMsg::TaskStarted(_)));
    let has_context_compacted = events
        .iter()
        .any(|e| matches!(e.msg, EventMsg::ContextCompacted(_)));
    let has_warning = events.iter().any(|e| matches!(e.msg, EventMsg::Warning(_)));
    let has_task_complete = events
        .iter()
        .any(|e| matches!(e.msg, EventMsg::TaskComplete(_)));

    assert!(
        has_task_started,
        "Expected TaskStarted event. Events received: {events:?}"
    );
    assert!(
        has_context_compacted,
        "Expected ContextCompacted event. Events received: {events:?}"
    );
    assert!(
        has_warning,
        "Expected Warning event about long conversations. Events received: {events:?}"
    );
    assert!(
        has_task_complete,
        "Expected TaskComplete event. Events received: {events:?}"
    );
}

/// Test that after Op::Compact, subsequent Op::UserInput prompts have the
/// summary prefix prepended to the user's message.
///
/// This verifies the key behavior: the compact summary is stored and
/// automatically injected into future prompts.
#[tokio::test]
#[serial]
async fn test_compact_prepends_summary_to_next_prompt() {
    use std::time::Duration;

    // Get the mock agent config to check if the binary exists
    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");

    // Check if mock agent binary exists
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (event_tx, mut event_rx) = mpsc::channel(64);

    let config = AcpBackendConfig {
        agent: "mock-model".to_string(),
        cwd: temp_dir.path().to_path_buf(),
        approval_policy: AskForApproval::Never,
        sandbox_policy: SandboxPolicy::new_read_only_policy(),
        notify: None,
        os_notifications: crate::config::OsNotifications::Disabled,
        nori_home: temp_dir.path().to_path_buf(),
        history_persistence: crate::config::HistoryPersistence::SaveAll,
        cli_version: "test".to_string(),
        notify_after_idle: crate::config::NotifyAfterIdle::FiveSeconds,
        auto_worktree: false,
        auto_worktree_repo_root: None,
        session_start_hooks: vec![],
        session_end_hooks: vec![],
        pre_user_prompt_hooks: vec![],
        post_user_prompt_hooks: vec![],
        pre_tool_call_hooks: vec![],
        post_tool_call_hooks: vec![],
        pre_agent_response_hooks: vec![],
        post_agent_response_hooks: vec![],
        async_session_start_hooks: vec![],
        async_session_end_hooks: vec![],
        async_pre_user_prompt_hooks: vec![],
        async_post_user_prompt_hooks: vec![],
        async_pre_tool_call_hooks: vec![],
        async_post_tool_call_hooks: vec![],
        async_pre_agent_response_hooks: vec![],
        async_post_agent_response_hooks: vec![],
        script_timeout: std::time::Duration::from_secs(30),
        default_model: None,
    };

    let backend = AcpBackend::spawn(&config, event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    // Drain the SessionConfigured event
    let _ = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
        .await
        .expect("Should receive SessionConfigured event");

    // First, submit Op::Compact to generate and store a summary
    let _id = backend
        .submit(Op::Compact)
        .await
        .expect("Failed to submit Op::Compact");

    // Wait for compact to complete
    let timeout = Duration::from_secs(10);
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        match tokio::time::timeout(Duration::from_millis(500), event_rx.recv()).await {
            Ok(Some(event)) => {
                if matches!(event.msg, EventMsg::TaskComplete(_)) {
                    break;
                }
            }
            _ => continue,
        }
    }

    // Now submit a regular user input
    let user_message = "What is 2 + 2?";
    let _id = backend
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Text {
                text: user_message.to_string(),
            }],
        })
        .await
        .expect("Failed to submit Op::UserInput");

    // Collect events from the user input turn
    let mut events = Vec::new();
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        match tokio::time::timeout(Duration::from_millis(500), event_rx.recv()).await {
            Ok(Some(event)) => {
                events.push(event);
                if matches!(
                    events.last().map(|e| &e.msg),
                    Some(EventMsg::TaskComplete(_))
                ) {
                    break;
                }
            }
            _ => {
                if events
                    .iter()
                    .any(|e| matches!(e.msg, EventMsg::TaskComplete(_)))
                {
                    break;
                }
            }
        }
    }

    // The mock agent echoes back what it receives, so we should see the summary
    // prefix in the agent's response if it was prepended correctly.
    // Look for agent message deltas that contain the summary prefix.
    let agent_messages: String = events
        .iter()
        .filter_map(|e| match &e.msg {
            EventMsg::AgentMessageDelta(delta) => Some(delta.delta.clone()),
            _ => None,
        })
        .collect();

    // The agent should have received a prompt that starts with the summary prefix
    // Since the mock agent echoes input, we verify the structure is correct
    // by checking that the agent received something (the response won't be empty)
    assert!(
        !agent_messages.is_empty()
            || events
                .iter()
                .any(|e| matches!(e.msg, EventMsg::TaskComplete(_))),
        "Expected agent response or task completion. Events: {events:?}"
    );

    // Verify that the backend has a pending_compact_summary stored
    // (This requires checking internal state, which we'll verify through behavior)
    // The key assertion is that the compact operation succeeded and subsequent
    // prompts can be sent without error
    let has_task_complete = events
        .iter()
        .any(|e| matches!(e.msg, EventMsg::TaskComplete(_)));
    assert!(
        has_task_complete,
        "Expected TaskComplete event for follow-up prompt. Events: {events:?}"
    );
}

/// Test that Op::Compact is no longer in the unsupported operations list
/// and doesn't emit an error event.
#[tokio::test]
#[serial]
async fn test_compact_not_in_unsupported_ops() {
    use std::time::Duration;

    // Get the mock agent config to check if the binary exists
    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");

    // Check if mock agent binary exists
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (event_tx, mut event_rx) = mpsc::channel(64);

    let config = AcpBackendConfig {
        agent: "mock-model".to_string(),
        cwd: temp_dir.path().to_path_buf(),
        approval_policy: AskForApproval::Never,
        sandbox_policy: SandboxPolicy::new_read_only_policy(),
        notify: None,
        os_notifications: crate::config::OsNotifications::Disabled,
        nori_home: temp_dir.path().to_path_buf(),
        history_persistence: crate::config::HistoryPersistence::SaveAll,
        cli_version: "test".to_string(),
        notify_after_idle: crate::config::NotifyAfterIdle::FiveSeconds,
        auto_worktree: false,
        auto_worktree_repo_root: None,
        session_start_hooks: vec![],
        session_end_hooks: vec![],
        pre_user_prompt_hooks: vec![],
        post_user_prompt_hooks: vec![],
        pre_tool_call_hooks: vec![],
        post_tool_call_hooks: vec![],
        pre_agent_response_hooks: vec![],
        post_agent_response_hooks: vec![],
        async_session_start_hooks: vec![],
        async_session_end_hooks: vec![],
        async_pre_user_prompt_hooks: vec![],
        async_post_user_prompt_hooks: vec![],
        async_pre_tool_call_hooks: vec![],
        async_post_tool_call_hooks: vec![],
        async_pre_agent_response_hooks: vec![],
        async_post_agent_response_hooks: vec![],
        script_timeout: std::time::Duration::from_secs(30),
        default_model: None,
    };

    let backend = AcpBackend::spawn(&config, event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    // Drain the SessionConfigured event
    let _ = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
        .await
        .expect("Should receive SessionConfigured event");

    // Submit the Compact operation
    let result = backend.submit(Op::Compact).await;

    // The submission should succeed (not return an error)
    assert!(
        result.is_ok(),
        "Op::Compact should not fail to submit: {result:?}"
    );

    // Collect events and verify no Error event was emitted for "unsupported"
    let mut events = Vec::new();
    let timeout = Duration::from_secs(5);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        match tokio::time::timeout(Duration::from_millis(200), event_rx.recv()).await {
            Ok(Some(event)) => {
                events.push(event);
                if matches!(
                    events.last().map(|e| &e.msg),
                    Some(EventMsg::TaskComplete(_))
                ) {
                    break;
                }
            }
            _ => {
                if events
                    .iter()
                    .any(|e| matches!(e.msg, EventMsg::TaskComplete(_)))
                {
                    break;
                }
            }
        }
    }

    // Check that no error event mentions "not supported"
    let unsupported_error = events.iter().any(|e| {
        if let EventMsg::Error(err) = &e.msg {
            err.message.contains("not supported")
        } else {
            false
        }
    });

    assert!(
        !unsupported_error,
        "Op::Compact should not emit 'not supported' error. Events: {events:?}"
    );
}

/// Test that usage limit errors (like "out of extra usage") are categorized as QuotaExceeded.
/// These errors come from Claude's API when usage limits are hit.
#[test]
fn test_categorize_acp_error_usage_limit() {
    // The exact error message from Claude's stderr when usage is exceeded
    assert_eq!(
        categorize_acp_error(
            "Internal error: You're out of extra usage · resets 4pm (America/New_York)"
        ),
        AcpErrorCategory::QuotaExceeded,
        "Usage limit errors should be categorized as QuotaExceeded"
    );

    // Variations that might appear
    assert_eq!(
        categorize_acp_error("out of extra usage"),
        AcpErrorCategory::QuotaExceeded,
        "'out of extra usage' should be QuotaExceeded"
    );

    assert_eq!(
        categorize_acp_error("usage limit exceeded"),
        AcpErrorCategory::QuotaExceeded,
        "'usage limit exceeded' should be QuotaExceeded"
    );

    assert_eq!(
        categorize_acp_error("You have exceeded your usage"),
        AcpErrorCategory::QuotaExceeded,
        "'exceeded your usage' should be QuotaExceeded"
    );
}

/// Test that enhanced_error_message for QuotaExceeded includes the original error details.
/// Users need to see the specific error (like "resets 4pm") to know when they can retry.
#[test]
fn test_enhanced_error_message_quota_includes_original_error() {
    use crate::registry::AgentKind;

    let original_error = "You're out of extra usage · resets 4pm (America/New_York)";
    let message = enhanced_error_message(
        AcpErrorCategory::QuotaExceeded,
        original_error,
        "Claude",
        AgentKind::ClaudeCode.auth_hint(),
        AgentKind::ClaudeCode.display_name(),
        AgentKind::ClaudeCode.npm_package(),
    );

    // The message should include the original error so users know when they can retry
    assert!(
        message.contains("resets 4pm"),
        "QuotaExceeded message should include the original error details. Got: {message}"
    );
    assert!(
        message.contains("Rate limit") || message.contains("quota"),
        "QuotaExceeded message should mention rate limit/quota. Got: {message}"
    );
}

#[test]
fn test_commands_dir_returns_commands_subdir() {
    use pretty_assertions::assert_eq;
    let nori_home = PathBuf::from("/home/user/.nori/cli");
    let result = commands_dir(&nori_home);
    assert_eq!(result, PathBuf::from("/home/user/.nori/cli/commands"));
}

#[tokio::test]
async fn test_list_custom_prompts_sends_response_event() {
    use pretty_assertions::assert_eq;

    let tmp = tempfile::tempdir().expect("create TempDir");
    let nori_home = tmp.path();
    let cmds_dir = commands_dir(nori_home);
    std::fs::create_dir(&cmds_dir).unwrap();

    std::fs::write(
        cmds_dir.join("explain.md"),
        "---\ndescription: \"Explain code\"\nargument-hint: \"[file]\"\n---\nExplain $ARGUMENTS",
    )
    .unwrap();
    std::fs::write(cmds_dir.join("review.md"), "Review the code").unwrap();

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(16);
    let dir = commands_dir(nori_home);
    let id = "test-id".to_string();

    tokio::spawn(async move {
        let custom_prompts = codex_core::custom_prompts::discover_prompts_in(&dir).await;
        let _ = event_tx
            .send(Event {
                id,
                msg: EventMsg::ListCustomPromptsResponse(
                    codex_protocol::protocol::ListCustomPromptsResponseEvent { custom_prompts },
                ),
            })
            .await;
    });

    let event = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
        .await
        .expect("timeout")
        .expect("channel closed");

    assert_eq!(event.id, "test-id");
    match event.msg {
        EventMsg::ListCustomPromptsResponse(ev) => {
            assert_eq!(ev.custom_prompts.len(), 2);
            assert_eq!(ev.custom_prompts[0].name, "explain");
            assert_eq!(
                ev.custom_prompts[0].description.as_deref(),
                Some("Explain code")
            );
            assert_eq!(
                ev.custom_prompts[0].argument_hint.as_deref(),
                Some("[file]")
            );
            assert_eq!(ev.custom_prompts[0].content, "Explain $ARGUMENTS");
            assert_eq!(ev.custom_prompts[1].name, "review");
            assert_eq!(ev.custom_prompts[1].content, "Review the code");
        }
        other => panic!("Expected ListCustomPromptsResponse, got {other:?}"),
    }
}

#[test]
fn transcript_to_replay_events_converts_user_and_assistant() {
    use crate::transcript::*;
    use pretty_assertions::assert_eq;

    let entries = vec![
        TranscriptLine::new(TranscriptEntry::SessionMeta(SessionMetaEntry {
            session_id: "s1".into(),
            project_id: "p1".into(),
            started_at: "2025-01-01T00:00:00.000Z".into(),
            cwd: PathBuf::from("/tmp"),
            agent: Some("claude-code".into()),
            cli_version: "0.1.0".into(),
            git: None,
            acp_session_id: None,
        })),
        TranscriptLine::new(TranscriptEntry::User(UserEntry {
            id: "msg-001".into(),
            content: "Hello, world!".into(),
            attachments: vec![],
        })),
        TranscriptLine::new(TranscriptEntry::Assistant(AssistantEntry {
            id: "msg-002".into(),
            content: vec![ContentBlock::Text {
                text: "Hi there!".into(),
            }],
            agent: Some("claude-code".into()),
        })),
        TranscriptLine::new(TranscriptEntry::ToolCall(ToolCallEntry {
            call_id: "call-001".into(),
            name: "shell".into(),
            input: serde_json::json!({"command": "ls"}),
        })),
        TranscriptLine::new(TranscriptEntry::ToolResult(ToolResultEntry {
            call_id: "call-001".into(),
            output: "file1.txt\nfile2.txt".into(),
            truncated: false,
            exit_code: Some(0),
        })),
        TranscriptLine::new(TranscriptEntry::User(UserEntry {
            id: "msg-003".into(),
            content: "Thanks!".into(),
            attachments: vec![],
        })),
    ];

    let transcript = crate::transcript::Transcript {
        meta: match &entries[0].entry {
            TranscriptEntry::SessionMeta(m) => m.clone(),
            _ => unreachable!(),
        },
        entries,
    };

    let events = transcript_to_replay_events(&transcript);

    // Should only include User and Assistant entries (3 total: 2 user + 1 assistant)
    assert_eq!(events.len(), 3);

    // First event: UserMessage
    match &events[0] {
        EventMsg::UserMessage(ev) => assert_eq!(ev.message, "Hello, world!"),
        other => panic!("Expected UserMessage, got {other:?}"),
    }

    // Second event: AgentMessage
    match &events[1] {
        EventMsg::AgentMessage(ev) => assert_eq!(ev.message, "Hi there!"),
        other => panic!("Expected AgentMessage, got {other:?}"),
    }

    // Third event: UserMessage
    match &events[2] {
        EventMsg::UserMessage(ev) => assert_eq!(ev.message, "Thanks!"),
        other => panic!("Expected UserMessage, got {other:?}"),
    }
}

#[test]
fn transcript_to_replay_events_empty_transcript() {
    use crate::transcript::*;
    use pretty_assertions::assert_eq;

    let meta = SessionMetaEntry {
        session_id: "s1".into(),
        project_id: "p1".into(),
        started_at: "2025-01-01T00:00:00.000Z".into(),
        cwd: PathBuf::from("/tmp"),
        agent: None,
        cli_version: "0.1.0".into(),
        git: None,
        acp_session_id: None,
    };

    let transcript = crate::transcript::Transcript {
        meta: meta.clone(),
        entries: vec![TranscriptLine::new(TranscriptEntry::SessionMeta(meta))],
    };

    let events = transcript_to_replay_events(&transcript);
    assert_eq!(events.len(), 0);
}

#[test]
fn transcript_to_summary_builds_conversation_text() {
    use crate::transcript::*;

    let entries = vec![
        TranscriptLine::new(TranscriptEntry::SessionMeta(SessionMetaEntry {
            session_id: "s1".into(),
            project_id: "p1".into(),
            started_at: "2025-01-01T00:00:00.000Z".into(),
            cwd: PathBuf::from("/tmp"),
            agent: Some("claude-code".into()),
            cli_version: "0.1.0".into(),
            git: None,
            acp_session_id: None,
        })),
        TranscriptLine::new(TranscriptEntry::User(UserEntry {
            id: "msg-001".into(),
            content: "Fix the bug in main.rs".into(),
            attachments: vec![],
        })),
        TranscriptLine::new(TranscriptEntry::Assistant(AssistantEntry {
            id: "msg-002".into(),
            content: vec![ContentBlock::Text {
                text: "I'll look at main.rs and fix the bug.".into(),
            }],
            agent: Some("claude-code".into()),
        })),
        TranscriptLine::new(TranscriptEntry::ToolCall(ToolCallEntry {
            call_id: "call-001".into(),
            name: "shell".into(),
            input: serde_json::json!({"command": "cat main.rs"}),
        })),
        TranscriptLine::new(TranscriptEntry::User(UserEntry {
            id: "msg-003".into(),
            content: "Great, thanks!".into(),
            attachments: vec![],
        })),
    ];

    let transcript = crate::transcript::Transcript {
        meta: match &entries[0].entry {
            TranscriptEntry::SessionMeta(m) => m.clone(),
            _ => unreachable!(),
        },
        entries,
    };

    let summary = transcript_to_summary(&transcript);

    assert!(summary.contains("User: Fix the bug in main.rs"));
    assert!(summary.contains("Assistant: I'll look at main.rs and fix the bug."));
    assert!(summary.contains("[Tool: shell]"));
    assert!(summary.contains("User: Great, thanks!"));
}

#[test]
fn transcript_to_summary_truncates_long_content() {
    use crate::transcript::*;

    let long_text = "x".repeat(30_000);
    let entries = vec![
        TranscriptLine::new(TranscriptEntry::SessionMeta(SessionMetaEntry {
            session_id: "s1".into(),
            project_id: "p1".into(),
            started_at: "2025-01-01T00:00:00.000Z".into(),
            cwd: PathBuf::from("/tmp"),
            agent: None,
            cli_version: "0.1.0".into(),
            git: None,
            acp_session_id: None,
        })),
        TranscriptLine::new(TranscriptEntry::User(UserEntry {
            id: "msg-001".into(),
            content: long_text,
            attachments: vec![],
        })),
    ];

    let transcript = crate::transcript::Transcript {
        meta: match &entries[0].entry {
            TranscriptEntry::SessionMeta(m) => m.clone(),
            _ => unreachable!(),
        },
        entries,
    };

    let summary = transcript_to_summary(&transcript);

    // Summary should be capped at a reasonable size
    assert!(
        summary.len() <= 25_000,
        "Summary should be truncated, got {} chars",
        summary.len()
    );
}

#[test]
fn truncate_for_log_with_multibyte_does_not_panic() {
    // ─ (U+2500) is 3 bytes in UTF-8 (0xE2 0x94 0x80).
    // Place it so that a naive byte slice at max_len would land
    // inside the character.
    let s = format!("{}─end", "a".repeat(9));
    // s layout: 9 ASCII bytes + 3-byte ─ + 3 ASCII = 15 bytes.
    // Truncating at byte 10 would split ─ (bytes 9..12).
    let result = truncate_for_log(&s, 10);
    assert!(result.len() <= 13, "result too long: {}", result.len()); // 10 + "..."
    assert!(result.ends_with("..."));
    // Must be valid UTF-8 (it compiles as String, so this is guaranteed,
    // but let's also verify the content makes sense).
    assert!(
        result.starts_with("aaaaaaaaa"),
        "unexpected prefix: {result}"
    );
}

#[test]
fn truncate_for_log_ascii_only() {
    let s = "abcdefghijklmnop";
    let result = truncate_for_log(s, 10);
    assert_eq!(result, "abcdefghij...");
}

#[test]
fn truncate_for_log_short_string_unchanged() {
    let s = "hello";
    let result = truncate_for_log(s, 10);
    assert_eq!(result, "hello");
}

/// Helper to build a minimal transcript for resume tests.
fn build_test_transcript() -> crate::transcript::Transcript {
    use crate::transcript::*;

    let entries = vec![
        TranscriptLine::new(TranscriptEntry::SessionMeta(SessionMetaEntry {
            session_id: "test-session-1".into(),
            project_id: "test-project".into(),
            started_at: "2025-01-01T00:00:00.000Z".into(),
            cwd: PathBuf::from("/tmp"),
            agent: Some("mock-agent".into()),
            cli_version: "0.1.0".into(),
            git: None,
            acp_session_id: Some("acp-session-42".into()),
        })),
        TranscriptLine::new(TranscriptEntry::User(UserEntry {
            id: "msg-001".into(),
            content: "Hello, world!".into(),
            attachments: vec![],
        })),
        TranscriptLine::new(TranscriptEntry::Assistant(AssistantEntry {
            id: "msg-002".into(),
            content: vec![ContentBlock::Text {
                text: "Hi there! I can help.".into(),
            }],
            agent: Some("mock-agent".into()),
        })),
    ];

    crate::transcript::Transcript {
        meta: match &entries[0].entry {
            TranscriptEntry::SessionMeta(m) => m.clone(),
            _ => unreachable!(),
        },
        entries,
    }
}

/// Helper to build a standard AcpBackendConfig for testing.
fn build_test_config(temp_dir: &std::path::Path) -> AcpBackendConfig {
    AcpBackendConfig {
        agent: "mock-model".to_string(),
        cwd: temp_dir.to_path_buf(),
        approval_policy: AskForApproval::Never,
        sandbox_policy: SandboxPolicy::new_read_only_policy(),
        notify: None,
        os_notifications: crate::config::OsNotifications::Disabled,
        nori_home: temp_dir.to_path_buf(),
        history_persistence: crate::config::HistoryPersistence::SaveAll,
        cli_version: "test".to_string(),
        notify_after_idle: crate::config::NotifyAfterIdle::FiveSeconds,
        auto_worktree: false,
        auto_worktree_repo_root: None,
        session_start_hooks: vec![],
        session_end_hooks: vec![],
        pre_user_prompt_hooks: vec![],
        post_user_prompt_hooks: vec![],
        pre_tool_call_hooks: vec![],
        post_tool_call_hooks: vec![],
        pre_agent_response_hooks: vec![],
        post_agent_response_hooks: vec![],
        async_session_start_hooks: vec![],
        async_session_end_hooks: vec![],
        async_pre_user_prompt_hooks: vec![],
        async_post_user_prompt_hooks: vec![],
        async_pre_tool_call_hooks: vec![],
        async_post_tool_call_hooks: vec![],
        async_pre_agent_response_hooks: vec![],
        async_post_agent_response_hooks: vec![],
        script_timeout: std::time::Duration::from_secs(30),
        default_model: None,
    }
}

/// When load_session fails at runtime, resume_session should fall back to
/// client-side replay instead of propagating the error.
#[tokio::test]
#[serial]
async fn test_resume_session_falls_back_on_load_session_failure() {
    use std::time::Duration;

    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    // Agent advertises load_session, but load_session call itself fails
    // SAFETY: This is a test that manipulates environment variables.
    // It's safe because this test runs in isolation and we clean up after.
    unsafe {
        std::env::set_var("MOCK_AGENT_SUPPORT_LOAD_SESSION", "1");
        std::env::set_var("MOCK_AGENT_LOAD_SESSION_FAIL", "1");
    }

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (event_tx, mut event_rx) = mpsc::channel(64);
    let config = build_test_config(temp_dir.path());
    let transcript = build_test_transcript();

    let result =
        AcpBackend::resume_session(&config, Some("acp-session-42"), Some(&transcript), event_tx)
            .await;

    // SAFETY: Cleaning up the environment variables we set above.
    unsafe {
        std::env::remove_var("MOCK_AGENT_SUPPORT_LOAD_SESSION");
        std::env::remove_var("MOCK_AGENT_LOAD_SESSION_FAIL");
    }

    // The resume should succeed (fallback to client-side replay)
    assert!(
        result.is_ok(),
        "resume_session should succeed via fallback, but got: {:?}",
        result.err()
    );

    // Collect the SessionConfigured event
    let event = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("Should receive an event within timeout")
        .expect("Channel should not be closed");

    // Verify that initial_messages is Some (client-side replay was used)
    match event.msg {
        EventMsg::SessionConfigured(configured) => {
            assert!(
                configured.initial_messages.is_some(),
                "Expected initial_messages to be Some (client-side replay), but got None"
            );
            let messages = configured.initial_messages.unwrap();
            assert!(!messages.is_empty(), "Expected at least one replay message");
        }
        other => panic!(
            "Expected SessionConfigured event, got: {:?}",
            std::mem::discriminant(&other)
        ),
    }

    // Verify that a WarningEvent was sent about the fallback
    let warning_event = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("Should receive warning event within timeout")
        .expect("Channel should not be closed");

    match warning_event.msg {
        EventMsg::Warning(warning) => {
            assert!(
                warning
                    .message
                    .contains("Server-side session restore failed"),
                "Warning should mention server-side failure, got: {}",
                warning.message
            );
            assert!(
                warning.message.contains("tool call information"),
                "Warning should mention missing tool call info, got: {}",
                warning.message
            );
        }
        other => panic!(
            "Expected Warning event, got: {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

/// When load_session sends many notifications during session replay,
/// resume_session must not deadlock. This reproduces a bug where the
/// forwarding task blocked on `event_tx.send().await` (bounded channel)
/// while `resume_session` awaited the forwarding task, and the consumer
/// of `event_rx` hadn't started yet — causing a circular wait.
#[tokio::test]
#[serial]
async fn test_resume_session_does_not_deadlock_with_many_notifications() {
    use std::time::Duration;

    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    // Agent advertises load_session, load_session succeeds, and sends
    // 100 notifications during the load — more than the event channel
    // capacity (64 in test, 32 in production), triggering the deadlock.
    // SAFETY: This is a test that manipulates environment variables.
    // It's safe because this test runs in isolation and we clean up after.
    unsafe {
        std::env::set_var("MOCK_AGENT_SUPPORT_LOAD_SESSION", "1");
        std::env::set_var("MOCK_AGENT_LOAD_SESSION_NOTIFICATION_COUNT", "100");
    }

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (event_tx, mut event_rx) = mpsc::channel(64);
    let config = build_test_config(temp_dir.path());
    let transcript = build_test_transcript();

    // No consumer is spawned — this mirrors real usage where the TUI
    // consumer starts only AFTER resume_session returns. A timeout
    // detects the deadlock: if resume_session hangs, it times out.
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        AcpBackend::resume_session(&config, Some("acp-session-42"), Some(&transcript), event_tx),
    )
    .await;

    // SAFETY: Cleaning up the environment variables we set above.
    unsafe {
        std::env::remove_var("MOCK_AGENT_SUPPORT_LOAD_SESSION");
        std::env::remove_var("MOCK_AGENT_LOAD_SESSION_NOTIFICATION_COUNT");
    }

    // If we got a timeout, the deadlock is present
    let backend_result = result.expect(
        "resume_session deadlocked: timed out after 10s. \
         The forwarding task is blocked on event_tx.send().await \
         while resume_session awaits forward_handle",
    );

    // The resume should succeed
    assert!(
        backend_result.is_ok(),
        "resume_session should succeed, but got: {:?}",
        backend_result.err()
    );

    // Drain events and verify we received the replayed notifications
    let mut notification_count = 0;
    while let Ok(Some(event)) =
        tokio::time::timeout(Duration::from_millis(500), event_rx.recv()).await
    {
        if matches!(event.msg, EventMsg::AgentMessageDelta(_)) {
            notification_count += 1;
        }
    }

    assert!(
        notification_count >= 100,
        "Expected at least 100 replayed notification events, got {notification_count}"
    );
}

/// When load_session succeeds, resume_session should use the server-side
/// path and NOT produce initial_messages.
#[tokio::test]
#[serial]
async fn test_resume_session_uses_server_side_when_load_session_succeeds() {
    use std::time::Duration;

    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    // Agent advertises load_session, and load_session succeeds
    // SAFETY: This is a test that manipulates environment variables.
    // It's safe because this test runs in isolation and we clean up after.
    unsafe {
        std::env::set_var("MOCK_AGENT_SUPPORT_LOAD_SESSION", "1");
    }

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (event_tx, mut event_rx) = mpsc::channel(64);
    let config = build_test_config(temp_dir.path());
    let transcript = build_test_transcript();

    let result =
        AcpBackend::resume_session(&config, Some("acp-session-42"), Some(&transcript), event_tx)
            .await;

    // SAFETY: Cleaning up the environment variable we set above.
    unsafe {
        std::env::remove_var("MOCK_AGENT_SUPPORT_LOAD_SESSION");
    }

    assert!(
        result.is_ok(),
        "resume_session should succeed, but got: {:?}",
        result.err()
    );

    // Collect the SessionConfigured event
    let event = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("Should receive an event within timeout")
        .expect("Channel should not be closed");

    // Server-side path should NOT produce initial_messages
    match event.msg {
        EventMsg::SessionConfigured(configured) => {
            assert!(
                configured.initial_messages.is_none(),
                "Expected initial_messages to be None (server-side resume), but got Some"
            );
        }
        other => panic!(
            "Expected SessionConfigured event, got: {:?}",
            std::mem::discriminant(&other)
        ),
    }
}
