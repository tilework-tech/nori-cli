//! E2E tests for ACP mode startup and approval bridging
//!
//! These tests verify that:
//! 1. ACP mode starts correctly when configured via wire_api = "acp"
//! 2. The approval bridging infrastructure works correctly
//! 3. Permission requests from ACP agents are properly displayed in the TUI

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TIMEOUT_PRESNAPSHOT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

/// Test that ACP mode starts successfully with mock-model
#[test]
fn test_acp_mode_startup_with_mock_agent() {
    let config = SessionConfig::new().with_model("mock-model".to_owned());

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn codex in ACP mode");

    // Wait for the main prompt to appear (indicated by the chevron character)
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode TUI should start successfully with mock agent");

    std::thread::sleep(TIMEOUT_INPUT);

    let contents = session.screen_contents();

    // Verify we're in the TUI and not stuck at an error screen
    assert!(
        contents.contains("›") && contents.contains("context left"),
        "Should show main prompt with context indicator in ACP mode, got: {}",
        contents
    );

    // Should NOT show any ACP-related errors
    assert!(
        !contents.contains("Error") && !contents.contains("error"),
        "ACP mode should start without errors, got: {}",
        contents
    );
}

/// Test that ACP mode can send a prompt and receive a response from mock agent
#[test]
#[cfg(target_os = "linux")]
fn test_acp_mode_prompt_response_flow() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_mock_response("Hello from ACP mock agent!");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn codex in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a test prompt
    session.send_str("Test ACP prompt").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the mock response
    session
        .wait_for_text("Hello from ACP mock agent!", TIMEOUT)
        .expect("Should receive response from mock ACP agent");
}

/// Test that ACP approval requests are displayed in the TUI
///
/// This test verifies the approval bridging infrastructure by:
/// 1. Configuring the mock agent to request permission
/// 2. Verifying the permission request appears in the TUI
/// 3. Verifying user can respond to the permission request
///
/// ## Prerequisites for this test to pass:
/// 1. Mock agent must support MOCK_AGENT_REQUEST_PERMISSION env var
/// 2. TUI must listen to AcpConnection::take_approval_receiver()
/// 3. TUI must display ExecApprovalRequestEvent and send ReviewDecision back
#[test]
#[cfg(target_os = "linux")]
fn test_acp_approval_request_displayed_in_tui() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        // Configure mock agent to request permission before responding
        .with_agent_env("MOCK_AGENT_REQUEST_PERMISSION", "1");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn codex in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt that triggers a permission request
    session.send_str("Run a shell command").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the approval request to appear
    // The TUI should display something like "ACP agent requests permission"
    // or show the approval popup from ExecApprovalRequestEvent
    let approval_appeared = session.wait_for(
        |screen| {
            screen.contains("permission")
                || screen.contains("approve")
                || screen.contains("allow")
                || screen.contains("deny")
                || screen.contains("ACP agent requests")
        },
        Duration::from_secs(10),
    );

    match approval_appeared {
        Ok(()) => {
            // Approval UI appeared - verify we can see the request
            let contents = session.screen_contents();
            eprintln!("Approval request displayed:\n{}", contents);

            // The approval UI should show:
            // - "Yes, proceed" for approve
            // - "No, and tell" for deny/alternative
            // - The reason from the ACP agent
            assert!(
                contents.contains("Yes, proceed")
                    || contents.contains("Yes,")
                    || contents.contains("No,"),
                "Approval UI should show Yes/No options, got: {}",
                contents
            );
        }
        Err(e) => {
            panic!(
                "Approval request not displayed in TUI. Error: {}. Screen contents:\n{}",
                e,
                session.screen_contents()
            );
        }
    }
}

/// Test full approval flow: approve → agent continues → TUI remains functional
///
/// This test verifies the complete approval handling cycle:
/// 1. User sends a prompt that triggers a permission request
/// 2. Approval UI appears with Yes/No options
/// 3. User approves by pressing 'y'
/// 4. Agent receives the approval and continues
/// 5. Agent sends a continuation message
/// 6. TUI returns to input state and remains functional
#[test]
#[cfg(target_os = "linux")]
fn test_acp_approval_full_flow() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_REQUEST_PERMISSION", "1");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn codex in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt that triggers a permission request
    session.send_str("Test approval flow").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the approval request to appear
    session
        .wait_for(
            |screen| screen.contains("Yes, proceed") || screen.contains("proceed"),
            Duration::from_secs(10),
        )
        .expect("Approval UI should appear");

    eprintln!("Approval UI appeared:\n{}", session.screen_contents());

    // Approve by pressing 'y'
    session.send_key(Key::Char('y')).unwrap();

    // Wait for the agent to continue after approval
    // The mock agent sends "Permission granted with option: allow" after approval
    session
        .wait_for_text("Permission granted", Duration::from_secs(10))
        .expect("Agent should continue after approval and send response");

    eprintln!(
        "Agent continued after approval:\n{}",
        session.screen_contents()
    );

    // Verify TUI is back in input state (prompt visible)
    session
        .wait_for_text("›", Duration::from_secs(5))
        .expect("TUI should return to input state");

    // Verify TUI is still functional - can type a new message
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_str("Follow-up message").unwrap();

    // Verify the typed text appears
    session
        .wait_for_text("Follow-up message", Duration::from_secs(3))
        .expect("TUI should remain functional for further input");
}

// NOTE: Ctrl-C tests cannot be implemented as E2E tests because the PTY
// environment intercepts Ctrl-C (0x03) as SIGINT before it reaches the TUI.
// The Op::Shutdown handling is tested via unit tests in acp/src/backend.rs instead.

/// Test snapshot of ACP mode startup screen
#[test]
#[ignore] // Flaky: ListCustomPrompts error timing varies between runs
fn test_acp_mode_startup_snapshot() {
    let config = SessionConfig::new().with_model("mock-model".to_owned());

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn codex in ACP mode");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    insta::assert_snapshot!(
        "acp_mode_startup",
        normalize_for_input_snapshot(session.screen_contents())
    );
}
