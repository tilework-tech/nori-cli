//! E2E tests for ACP transcript persistence and view-only reload
//!
//! Tests that:
//! 1. Transcripts are persisted during ACP sessions
//! 2. The /resume-viewonly command shows previous sessions
//! 3. Selecting a session displays the transcript correctly
//!
//! These tests require the `transcript-viewonly` feature to be enabled.

#![cfg(feature = "transcript-viewonly")]

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TIMEOUT_PRESNAPSHOT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

/// Test that transcripts are persisted and can be viewed via /resume-viewonly
///
/// This test validates the entire transcript persistence and view-only reload flow:
/// 1. Spawn a TUI session with mock-model
/// 2. Send a multi-turn interaction (2 user prompts with distinct content)
/// 3. Wait for assistant responses containing recognizable text
/// 4. Execute /new command to start a fresh session
/// 5. Execute /resume-viewonly command to open the session picker
/// 6. Select the previous session from the list
/// 7. Verify the transcript viewer shows the unique content from the first session
// @current-session
#[test]
#[cfg(target_os = "linux")]
fn test_transcript_persistence_and_viewonly_reload() {
    // Configure mock agent with multi-turn response support
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_MULTI_TURN", "1");

    let mut session = TuiSession::spawn_with_config(30, 100, config).expect("Failed to spawn");

    // Wait for startup
    session.wait_for_text("›", TIMEOUT).expect("Should start");
    std::thread::sleep(TIMEOUT_INPUT);

    // === Turn 1: Send first user message ===
    session.send_str("UNIQUE_PROMPT_ALPHA_12345").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for response containing ALPHA marker
    session
        .wait_for_text("RESPONSE_ALPHA", Duration::from_secs(10))
        .expect("Should receive first response");
    std::thread::sleep(TIMEOUT_INPUT);

    // === Turn 2: Send second user message ===
    session.send_str("UNIQUE_PROMPT_BETA_67890").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for second response containing BETA marker
    session
        .wait_for_text("RESPONSE_BETA", Duration::from_secs(10))
        .expect("Should receive second response");
    std::thread::sleep(TIMEOUT_INPUT);

    // === Start new session with /new ===
    session.send_str("/new").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for new session to be ready (fresh prompt)
    session
        .wait_for_text("›", TIMEOUT)
        .expect("New session should start");
    std::thread::sleep(TIMEOUT_INPUT);

    // === Open view-only transcript picker with /resume-viewonly ===
    session.send_str("/resume-viewonly").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the session picker to appear (check both title and footer hint to ensure full render)
    session
        .wait_for_text("View previous session", Duration::from_secs(5))
        .expect("Should show viewonly session picker title");
    session
        .wait_for_text("to navigate", Duration::from_secs(2))
        .expect("Should show picker footer hint");

    // Wait a bit longer to ensure the picker is fully ready for input
    std::thread::sleep(Duration::from_millis(200));

    // Select the second session (the one with the first conversation - more messages)
    // First session listed is the newer one (from /new), second is older with 5 messages
    session.send_key(Key::Down).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Press Enter to select the session
    session.send_key(Key::Enter).unwrap();

    // Wait for the transcript viewer to load - should show the first user prompt
    session
        .wait_for_text("UNIQUE_PROMPT_ALPHA_12345", Duration::from_secs(5))
        .expect("Transcript should show first user prompt");

    // Verify second prompt is also visible
    session
        .wait_for_text("UNIQUE_PROMPT_BETA_67890", Duration::from_secs(2))
        .expect("Transcript should show second user prompt");

    // Verify responses are shown
    let contents = session.screen_contents();
    assert!(
        contents.contains("RESPONSE_ALPHA"),
        "Transcript should show first response, got:\n{}",
        contents
    );
    assert!(
        contents.contains("RESPONSE_BETA"),
        "Transcript should show second response, got:\n{}",
        contents
    );

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // Snapshot the transcript viewer
    insta::assert_snapshot!(
        "acp_transcript_viewonly",
        normalize_for_input_snapshot(session.screen_contents())
    );
}
