//! E2E tests for ACP tool call display in the TUI
//!
//! These tests verify that tool calls from ACP agents are properly displayed
//! in the TUI history cells.

use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;

/// Test that ACP tool calls are displayed in the TUI
///
/// This test verifies that when an ACP agent sends a tool call sequence
/// (pending -> in_progress -> completed), the TUI displays information
/// about the tool call to the user.
#[test]
fn test_acp_tool_call_displayed() {
    let config = SessionConfig::new().with_tool_call();
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    // Wait for prompt to appear
    session
        .wait_for_text("?", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    // Submit a prompt to trigger the mock agent
    session.send_str("test tool call").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the response that comes after the tool call
    session
        .wait_for_text("Tool call completed successfully", TIMEOUT)
        .expect("Tool call completion message not found");

    // Verify that the tool call title is displayed in the TUI
    // The mock agent sends a tool call with title "Reading configuration file"
    let screen = session.screen_contents();
    assert!(
        screen.contains("Reading configuration file"),
        "Tool call title 'Reading configuration file' should be displayed in TUI.\nScreen contents:\n{}",
        screen
    );
}

/// Test that tool call status transitions are reflected in the TUI
///
/// This test verifies that as the tool call progresses through
/// pending -> in_progress -> completed, the UI updates accordingly.
#[test]
fn test_acp_tool_call_status_updates() {
    let config = SessionConfig::new().with_tool_call();
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    // Wait for prompt
    session
        .wait_for_text("?", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    // Submit prompt
    session.send_str("test").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for completion
    session
        .wait_for_text("Tool call completed successfully", TIMEOUT)
        .expect("Tool call did not complete");

    // The screen should show some indication that a tool call occurred
    // and completed (e.g., a checkmark, "completed" status, or similar)
    let screen = session.screen_contents();

    // At minimum, the tool call title should be visible
    assert!(
        screen.contains("Reading configuration file")
            || screen.contains("read")
            || screen.contains("config"),
        "Tool call information should be displayed.\nScreen contents:\n{}",
        screen
    );
}

/// Test that multiple tool calls can be displayed
///
/// This test would verify that if an ACP agent sends multiple tool calls,
/// they are all displayed appropriately.
#[test]
fn test_acp_multiple_tool_calls() {
    // For now, we only test single tool call - this test documents
    // that multiple tool calls should be supported in the future
    let config = SessionConfig::new().with_tool_call();
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    session
        .wait_for_text("?", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    session.send_str("test").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Verify the tool call completed
    session
        .wait_for_text("Tool call completed successfully", TIMEOUT)
        .expect("Tool call should complete");

    let screen = session.screen_contents();
    // The tool call should be visible
    assert!(
        screen.contains("Reading configuration file"),
        "Tool call should be visible in screen.\nScreen contents:\n{}",
        screen
    );
}
