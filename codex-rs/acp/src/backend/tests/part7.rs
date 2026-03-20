use super::*;

/// Test that extract_command_from_permission_title extracts the shell command
/// from Gemini's compound title format:
///   `echo "hello" [current working directory /path/to/dir] (description)`
/// → `echo "hello"`
#[test]
fn test_extract_command_from_gemini_permission_title_full_format() {
    let title = r#"echo "Random command 1" [current working directory /home/user/project] (Running random command 1)"#;
    let result = tool_display::extract_command_from_permission_title(title);
    assert_eq!(result, r#"echo "Random command 1""#);
}

/// Test that extract_command_from_permission_title handles title with cwd but no description.
#[test]
fn test_extract_command_from_gemini_permission_title_no_description() {
    let title = "date [current working directory /home/user/project]";
    let result = tool_display::extract_command_from_permission_title(title);
    assert_eq!(result, "date");
}

/// Test that extract_command_from_permission_title returns the full title
/// when it doesn't contain the cwd pattern.
#[test]
fn test_extract_command_from_gemini_permission_title_no_cwd() {
    let title = "git status";
    let result = tool_display::extract_command_from_permission_title(title);
    assert_eq!(result, "git status");
}

/// Test that extract_command_from_permission_title handles commands with
/// brackets that aren't the cwd pattern.
#[test]
fn test_extract_command_from_gemini_permission_title_brackets_in_command() {
    let title = r#"echo "[test]" [current working directory /home/user]"#;
    let result = tool_display::extract_command_from_permission_title(title);
    assert_eq!(result, r#"echo "[test]""#);
}

/// Integration test: When a permission request stores tool call metadata in
/// pending_tool_calls, and a subsequent ToolCallUpdate(completed) arrives with
/// an empty title, the resolved command should contain the actual shell command
/// instead of falling back to "Tool".
#[test]
fn test_permission_metadata_resolves_tool_call_title_on_completion() {
    // Simulate what happens when a Gemini shell command goes through request_permission:
    // 1. The approval handler stores metadata in pending_tool_calls
    // 2. A ToolCallUpdate(completed) arrives with no title/kind

    let mut pending_patch_changes = std::collections::HashMap::new();
    let mut pending_tool_calls = std::collections::HashMap::new();

    // Step 1: Simulate storing metadata from the permission request
    let call_id = "run_shell_command-1774039255814";
    pending_tool_calls.insert(
        call_id.to_string(),
        AccumulatedToolCall {
            title: Some(r#"echo "hello world""#.to_string()),
            kind: Some(acp::ToolKind::Execute),
            raw_input: None,
            meta_tool_name: None,
        },
    );

    // Step 2: ToolCallUpdate(completed) arrives with empty fields (Gemini pattern)
    let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        acp::ToolCallId::from(call_id.to_string()),
        acp::ToolCallUpdateFields::new().status(acp::ToolCallStatus::Completed),
    ));

    let events = translate_session_update_to_events(
        &update,
        &mut pending_patch_changes,
        &mut pending_tool_calls,
    );

    assert_eq!(events.len(), 1);
    match &events[0] {
        EventMsg::ExecCommandEnd(end) => {
            // The command should contain the actual shell command, not "Tool"
            let cmd_str = end.command.join(" ");
            assert!(
                cmd_str.contains("echo") && cmd_str.contains("hello world"),
                "Command should contain 'echo \"hello world\"', got: {cmd_str}"
            );
            // The title should NOT be "Tool"
            assert!(
                !cmd_str.eq("Tool"),
                "Command should not be generic 'Tool', got: {cmd_str}"
            );
        }
        _ => panic!("Expected ExecCommandEnd event, got: {:?}", events[0]),
    }
}

/// Test that permission metadata also provides proper parsed_cmd classification
/// so that Execute kind tools render in command mode (not exploring mode).
#[test]
fn test_permission_metadata_provides_execute_classification() {
    let mut pending_patch_changes = std::collections::HashMap::new();
    let mut pending_tool_calls = std::collections::HashMap::new();

    let call_id = "run_shell_command-12345";
    pending_tool_calls.insert(
        call_id.to_string(),
        AccumulatedToolCall {
            title: Some("pwd".to_string()),
            kind: Some(acp::ToolKind::Execute),
            raw_input: None,
            meta_tool_name: None,
        },
    );

    let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        acp::ToolCallId::from(call_id.to_string()),
        acp::ToolCallUpdateFields::new().status(acp::ToolCallStatus::Completed),
    ));

    let events = translate_session_update_to_events(
        &update,
        &mut pending_patch_changes,
        &mut pending_tool_calls,
    );

    assert_eq!(events.len(), 1);
    match &events[0] {
        EventMsg::ExecCommandEnd(end) => {
            // Should be classified as Unknown (command mode), not Read/Search (exploring)
            assert_eq!(end.parsed_cmd.len(), 1);
            match &end.parsed_cmd[0] {
                ParsedCommand::Unknown { cmd } => {
                    assert!(
                        cmd.contains("pwd"),
                        "Command should contain 'pwd', got: {cmd}"
                    );
                }
                other => panic!("Expected ParsedCommand::Unknown for Execute kind, got: {other:?}"),
            }
        }
        _ => panic!("Expected ExecCommandEnd event"),
    }
}
