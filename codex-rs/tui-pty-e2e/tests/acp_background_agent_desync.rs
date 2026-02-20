//! E2E tests to reproduce the background agent notification desync bug.
//!
//! Bug description: When multiple background agents complete between user turns,
//! their completion notifications queue up and only drain one-per-user-message.
//! This causes the conversation to desync — the user sees responses in the wrong
//! order, and their messages get "replayed" multiple times as the notification
//! queue drains.
//!
//! These tests run the real `nori` binary in a PTY, interact with it as a user
//! would (typing prompts, pressing Enter, reading the screen), and verify that
//! inter-turn notifications don't cause desync.

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

/// Test that all inter-turn notifications surface on screen.
///
/// After a prompt completes, the mock agent sends 3 background notifications
/// (simulating background agent completions). All 3 should appear on screen
/// without the user needing to send additional prompts.
#[test]
#[cfg(target_os = "linux")]
fn test_all_inter_turn_notifications_surface() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_MULTI_TURN", "1")
        .with_agent_env("MOCK_AGENT_INTER_TURN_NOTIFICATIONS", "1")
        .with_agent_env("MOCK_AGENT_INTER_TURN_NOTIFICATION_COUNT", "3")
        .with_agent_env("MOCK_AGENT_INTER_TURN_NOTIFICATION_DELAY_MS", "300");

    let mut session = TuiSession::spawn_with_config(40, 120, config).expect("Failed to spawn nori");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("Should see input prompt");
    std::thread::sleep(TIMEOUT_INPUT);

    // Send first prompt
    session.send_str("ALPHA").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the prompt response
    session
        .wait_for_text("RESPONSE_ALPHA", Duration::from_secs(10))
        .expect("Should see RESPONSE_ALPHA");

    // Now wait for ALL 3 background notifications to appear.
    // These are sent 300ms apart after the prompt completes.
    // If the persistent relay works, they should all appear without user input.
    // Notification text includes the prompt marker (ALPHA) to disambiguate batches.
    session
        .wait_for_text("BACKGROUND_AGENT_ALPHA_1_COMPLETE", Duration::from_secs(10))
        .expect("Should see BACKGROUND_AGENT_ALPHA_1_COMPLETE without sending another prompt");

    session
        .wait_for_text("BACKGROUND_AGENT_ALPHA_2_COMPLETE", Duration::from_secs(10))
        .expect("Should see BACKGROUND_AGENT_ALPHA_2_COMPLETE without sending another prompt");

    session
        .wait_for_text("BACKGROUND_AGENT_ALPHA_3_COMPLETE", Duration::from_secs(10))
        .expect("Should see BACKGROUND_AGENT_ALPHA_3_COMPLETE without sending another prompt");

    eprintln!(
        "All inter-turn notifications surfaced. Screen:\n{}",
        session.screen_contents()
    );
}

/// Test that inter-turn notifications don't desync a subsequent prompt.
///
/// This reproduces the exact scenario from the bug report:
/// 1. Send prompt ALPHA, get RESPONSE_ALPHA
/// 2. Background notifications arrive (simulating background agent completions)
/// 3. Send prompt BETA
/// 4. Verify RESPONSE_BETA appears (not a replay of ALPHA or a stale notification)
///
/// The bug: notifications queue up and when the user sends BETA, the system
/// processes a stale notification instead of the user's actual message, causing
/// the conversation to desync.
#[test]
#[cfg(target_os = "linux")]
fn test_inter_turn_notifications_do_not_desync_conversation() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_MULTI_TURN", "1")
        .with_agent_env("MOCK_AGENT_INTER_TURN_NOTIFICATIONS", "1")
        .with_agent_env("MOCK_AGENT_INTER_TURN_NOTIFICATION_COUNT", "3")
        .with_agent_env("MOCK_AGENT_INTER_TURN_NOTIFICATION_DELAY_MS", "200");

    let mut session = TuiSession::spawn_with_config(40, 120, config).expect("Failed to spawn nori");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("Should see input prompt");
    std::thread::sleep(TIMEOUT_INPUT);

    // Send first prompt
    session.send_str("ALPHA").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the prompt response
    session
        .wait_for_text("RESPONSE_ALPHA", Duration::from_secs(10))
        .expect("Should see RESPONSE_ALPHA");

    // Wait long enough for the background notifications to have been sent by the
    // agent. With 3 notifications at 200ms apart, they'll all be sent within
    // ~600ms. We wait 2 seconds to be safe. We do NOT wait for them to appear on
    // screen — that's what test_all_inter_turn_notifications_surface checks.
    // Here we just need them queued up so we can test whether they cause desync.
    std::thread::sleep(Duration::from_secs(2));

    // Now send second prompt - this is the critical test.
    // If the conversation is desynced, the user will see stale notifications
    // instead of RESPONSE_BETA.
    session.send_str("BETA").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // RESPONSE_BETA should appear. If the bug is present, we might see
    // the conversation desync instead.
    session
        .wait_for_text("RESPONSE_BETA", Duration::from_secs(10))
        .expect(
            "BUG: RESPONSE_BETA should appear after sending BETA prompt. \
             If this fails, the inter-turn notifications caused conversation desync.",
        );

    // Verify no desync: RESPONSE_ALPHA should NOT appear after RESPONSE_BETA
    let screen = session.screen_contents();
    if let Some(beta_pos) = screen.find("RESPONSE_BETA") {
        let after_beta = &screen[beta_pos..];
        assert!(
            !after_beta.contains("RESPONSE_ALPHA"),
            "Desync detected: RESPONSE_ALPHA appeared after RESPONSE_BETA. Screen:\n{screen}"
        );
    }

    eprintln!("Desync test completed. Screen:\n{}", screen);
}
