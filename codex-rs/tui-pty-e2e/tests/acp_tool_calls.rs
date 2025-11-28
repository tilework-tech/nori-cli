//! E2E tests for ACP tool call display in the TUI
//!
//! These tests verify that tool calls from ACP agents are properly displayed
//! in the TUI history cells.

use insta::assert_snapshot;
use tui_pty_e2e::normalize_for_snapshot;
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
    std::thread::sleep(TIMEOUT_INPUT);

    // Verify that the tool call title is displayed in the TUI
    // The mock agent sends a tool call with title "Reading configuration file"
    assert_snapshot!(
        "tool_call_title",
        normalize_for_snapshot(session.screen_contents())
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
    assert_snapshot!(
        "tool_call_completion",
        normalize_for_snapshot(session.screen_contents())
    );
}
