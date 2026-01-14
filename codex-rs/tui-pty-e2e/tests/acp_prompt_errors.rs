//! E2E tests for ACP prompt error handling
//!
//! These tests verify that when ACP prompts fail, the error is properly
//! displayed to the user in the TUI rather than being silently swallowed.

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

/// Test that ACP prompt failures are displayed to the user
///
/// This test verifies that when an ACP agent's prompt() method returns an error,
/// the error is shown to the user in the TUI rather than being silently swallowed.
///
/// ## Bug being fixed:
/// Previously, when `connection.prompt()` failed in `AcpBackend::on_submit()`,
/// the error was only logged with `warn!()` but not sent to the TUI. This caused
/// a confusing UX where the "Working" indicator would disappear but no response
/// or error message would appear.
///
/// ## Expected behavior (after fix):
/// When a prompt fails, the user should see an error message in the TUI
/// indicating what went wrong (e.g., authentication error, rate limit, etc.)
#[test]
#[cfg(target_os = "linux")]
fn test_acp_prompt_failure_shows_error_to_user() {
    // Configure mock agent to fail on prompt
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_PROMPT_FAIL", "1");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt that will trigger the failure
    session.send_str("Hello").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for some response - we should see either an error message or
    // at minimum the prompt should become active again
    std::thread::sleep(Duration::from_secs(3));

    let contents = session.screen_contents();

    // Debug: Print what we see
    eprintln!("=== Screen contents after prompt failure ===");
    eprintln!("{}", contents);
    eprintln!("============================================");

    // The user should see a SPECIFIC error message, not just any text
    // The error message from the mock agent is "Mock prompt failure for testing"
    // The enhanced message will be "ACP prompt failed: ACP prompt failed"
    //
    // NOTE: The error might scroll off the 24-row screen, so we also check
    // by waiting specifically for the error text to appear
    let error_appeared = session.wait_for_text("ACP prompt failed", Duration::from_secs(5));

    // Re-capture screen after waiting
    let contents = session.screen_contents();

    // Debug: Print what we see
    eprintln!("=== Screen after waiting for error ===");
    eprintln!("{}", contents);
    eprintln!("======================================");

    assert!(
        error_appeared.is_ok() || contents.contains("ACP prompt failed"),
        "ACP prompt failure should display an error to the user.\n\
         Expected to find 'ACP prompt failed' in the TUI.\n\
         Wait result: {:?}\n\
         Screen contents:\n{}",
        error_appeared,
        contents
    );

    // Verify the "Working" indicator is gone (turn has ended)
    // This confirms TaskComplete was sent
    assert!(
        !contents.contains("Working"),
        "Working indicator should be gone after error. Screen contents:\n{}",
        contents
    );
}

/// Test that prompt errors don't leave the TUI in a broken state
///
/// After a prompt error, the TUI should be responsive and ready for new input.
/// The prompt indicator "›" should be visible after the error is processed.
#[test]
#[cfg(target_os = "linux")]
fn test_acp_prompt_failure_tui_remains_responsive() {
    // Configure mock agent to fail on prompt
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_PROMPT_FAIL", "1");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt that will fail
    session.send_str("Test message").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for error event to appear (confirms the error was processed)
    session
        .wait_for_text("ACP prompt failed", Duration::from_secs(5))
        .expect("Error message should appear");

    // After the error, the TUI should show the prompt indicator,
    // meaning it's ready for new input
    let contents = session.screen_contents();

    // The prompt indicator should be visible (TUI is responsive)
    assert!(
        contents.contains("›"),
        "TUI should show prompt indicator after error (ready for new input). Screen:\n{}",
        contents
    );

    // Working indicator should be gone
    assert!(
        !contents.contains("Working"),
        "Working indicator should be gone after error. Screen:\n{}",
        contents
    );
}
