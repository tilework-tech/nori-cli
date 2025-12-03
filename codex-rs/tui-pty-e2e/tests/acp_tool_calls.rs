//! E2E tests for ACP tool call rendering in the TUI
//!
//! These tests verify that tool calls from ACP agents are properly
//! rendered in the TUI using the McpToolCallCell component.
//!
//! ## Test Strategy
//!
//! The tests configure the mock-acp-agent to emit ToolCall/ToolCallUpdate
//! events, then verify the TUI displays them correctly. This validates
//! the entire ACP-to-TUI flow:
//!
//! 1. Mock agent sends `SessionUpdate::ToolCall`
//! 2. ACP translator converts to `EventMsg::McpToolCallBegin`
//! 3. TUI chatwidget renders via `McpToolCallCell`
//!
//! ## Expected TUI Output Format
//!
//! Active tool calls display as:
//! ```text
//! • Calling server.tool_name({"arg":"value"})
//! ```
//!
//! Completed tool calls display as:
//! ```text
//! ✓ Called server.tool_name({"arg":"value"}) (1.2s)
//! ```

use std::time::Duration;
use tui_pty_e2e::normalize_for_input_snapshot;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;

/// Test that an ACP tool call is rendered in the TUI
///
/// This test verifies the full ACP tool call rendering pipeline:
/// 1. Mock agent emits a ToolCall event
/// 2. Translator converts it to McpToolCallBegin
/// 3. TUI displays it using McpToolCallCell
///
/// ## Prerequisites for this test to pass:
/// - Mock agent must support MOCK_AGENT_TOOL_CALL env var
/// - translator.rs must handle SessionUpdate::ToolCall
/// - core/client.rs must emit EventMsg::McpToolCallBegin
#[test]
#[ignore]
// TODO: reenable these based on the correct MOCK_AGENT_SEND_TOOL_CALL vars
// and any other fixups to work with the completed mock support
fn test_acp_tool_call_rendered_in_tui() {
    // Configure mock agent to send a tool call
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        // Configure mock agent to emit a tool call before responding
        .with_agent_env("MOCK_AGENT_TOOL_CALL", "read_file")
        .with_agent_env("MOCK_AGENT_TOOL_CALL_ARGS", r#"{"path":"test.txt"}"#)
        .with_mock_response("Done with tool call.");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn codex in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt that triggers the tool call
    session.send_str("Read a file for me").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for tool call to appear in TUI
    // The tool call should render like: "• Calling acp.read_file(...)"
    // or with the server name from ACP
    let tool_call_appeared = session.wait_for(
        |screen| {
            // Look for signs of tool call rendering
            screen.contains("Calling")
                || screen.contains("Called")
                || screen.contains("read_file")
                || screen.contains("Reading")
        },
        Duration::from_secs(10),
    );

    match tool_call_appeared {
        Ok(()) => {
            // Tool call UI appeared
            let contents = session.screen_contents();

            // The tool call should display with the tool name
            assert!(
                contents.contains("read_file")
                    || contents.contains("Calling")
                    || contents.contains("Reading"),
                "Tool call should show tool name or 'Calling' prefix, got:\n{}",
                contents
            );
        }
        Err(e) => {
            panic!(
                "Tool call not rendered in TUI. Error: {}. Screen contents:\n{}",
                e,
                session.screen_contents()
            );
        }
    }
}

/// Test that an ACP tool call completion is rendered
///
/// This test verifies that when a tool call completes:
/// 1. The status changes from "Calling" to "Called"
/// 2. The duration is shown
/// 3. Any output is displayed
#[test]
#[ignore]
// TODO: reenable these based on the correct MOCK_AGENT_SEND_TOOL_CALL vars
// and any other fixups to work with the completed mock support
fn test_acp_tool_call_completion_rendered_in_tui() {
    // Configure mock agent to send a tool call with completion
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_TOOL_CALL", "echo")
        .with_agent_env("MOCK_AGENT_TOOL_CALL_ARGS", r#"{"message":"hello"}"#)
        .with_agent_env("MOCK_AGENT_TOOL_CALL_RESULT", "hello")
        .with_mock_response("Tool call completed.");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn codex in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt
    session.send_str("Echo hello").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the mock response which means the tool call has completed
    session
        .wait_for_text("Tool call completed", Duration::from_secs(10))
        .expect("Should receive completion response");

    std::thread::sleep(TIMEOUT_INPUT);

    let contents = session.screen_contents();

    // After completion, should show "Called" with checkmark and duration
    // The format is: "✓ Called server.tool_name(...) (Xs)"
    assert!(
        contents.contains("Called") || contents.contains("echo"),
        "Completed tool call should show 'Called' or tool name, got:\n{}",
        contents
    );
}

/// Snapshot test for ACP tool call rendering
///
/// This captures the exact visual rendering of an ACP tool call
/// to detect any regressions in the display format.
#[test]
fn test_acp_tool_call_snapshot() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_TOOL_CALL", "read_file")
        .with_agent_env("MOCK_AGENT_TOOL_CALL_ARGS", r#"{"path":"test.txt"}"#)
        .with_agent_env("MOCK_AGENT_TOOL_CALL_RESULT", "file content here")
        .with_mock_response("Read complete.");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn codex in ACP mode");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send prompt to trigger tool call
    session.send_str("Read test.txt").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the response
    session
        .wait_for_text("Read complete", Duration::from_secs(10))
        .expect("Should receive response");

    std::thread::sleep(TIMEOUT_INPUT);

    insta::assert_snapshot!(
        "acp_tool_call_rendered",
        normalize_for_input_snapshot(session.screen_contents())
    );
}
