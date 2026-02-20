use super::*;

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
