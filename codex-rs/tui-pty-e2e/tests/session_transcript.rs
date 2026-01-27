//! E2E tests for session transcript storage and reload functionality
//!
//! These tests verify that:
//! 1. Session transcripts are properly saved during conversations
//! 2. The /resume-viewonly command lists available sessions
//! 3. Selecting a session displays its transcript correctly
//!
//! ## Test Strategy
//!
//! The tests run multi-turn conversations to generate transcript data,
//! then use /new to start fresh sessions and /resume-viewonly to access
//! the previous session's transcript for view-only replay.

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TIMEOUT_PRESNAPSHOT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

// ============================================================================
// Test: Session Transcript Reload via /resume-viewonly
// ============================================================================

/// Test that users can view previous session transcripts via /resume-viewonly.
///
/// This test verifies the complete session transcript workflow:
/// 1. Run a two-turn conversation with identifiable content
/// 2. Use /new to start a fresh session
/// 3. Use /resume-viewonly to access the session picker
/// 4. Select the previous session
/// 5. Verify the transcript overlay displays the correct content
///
/// The test uses unique prompt text ("Alpha request", "Beta request") to ensure
/// the transcript content can be verified in the snapshot.
#[test]
#[cfg(target_os = "linux")]
fn test_session_transcript_reload_via_resume_viewonly() {
    // Use default mock responses ("Test message 1", "Test message 2")
    // which are sufficient to verify transcript storage and display
    let config = SessionConfig::new().with_model("mock-model".to_string());

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn TUI");

    // Wait for TUI startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    // === Turn 1: Send first prompt with unique identifier ===
    session.send_str("Alpha request - first turn").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for response
    session
        .wait_for_text("Test message", Duration::from_secs(5))
        .expect("Should receive first response");
    std::thread::sleep(Duration::from_millis(500));

    // === Turn 2: Send second prompt with unique identifier ===
    session.send_str("Beta request - second turn").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for second response
    session
        .wait_for_text("Test message", Duration::from_secs(5))
        .expect("Should receive second response");
    std::thread::sleep(Duration::from_millis(500));

    // === Use /new to start a fresh session ===
    session.send_str("/new").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for the /new command popup to show before pressing Enter
    session
        .wait_for_text("/new  start a new chat", Duration::from_secs(2))
        .expect("Command popup should appear");
    std::thread::sleep(TIMEOUT_INPUT);

    session.send_key(Key::Enter).unwrap();

    // Wait for the new session to start - look for the Nori session header
    // which indicates a fresh session has started
    session
        .wait_for(
            |screen| {
                // The old conversation should be cleared
                !screen.contains("Alpha request") && !screen.contains("Beta request")
            },
            Duration::from_secs(10),
        )
        .expect("New session should clear previous conversation");
    std::thread::sleep(Duration::from_millis(500));

    // === Use /resume-viewonly to access session picker ===
    session.send_str("/resume-viewonly").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for session picker to appear
    // The picker should show:
    // - Title: "Select Session" or similar
    // - Session list with timestamp, first message preview, message count
    session
        .wait_for(
            |screen| {
                // Look for session picker indicators
                screen.contains("Select Session")
                    || screen.contains("Previous Sessions")
                    || screen.contains("Alpha request") // First message preview
            },
            Duration::from_secs(8),
        )
        .expect("Session picker should appear with /resume-viewonly");

    std::thread::sleep(TIMEOUT_INPUT);

    // The most recent session should be highlighted/first
    // Select it by pressing Enter
    session.send_key(Key::Enter).unwrap();

    // Wait for transcript overlay to appear
    // The transcript overlay currently reuses the diff overlay which shows "D I F F" title
    session
        .wait_for(
            |screen| {
                // Look for the transcript overlay (currently shows DIFF title)
                // and the session ID line
                screen.contains("D I F F") && screen.contains("Session:")
            },
            Duration::from_secs(5),
        )
        .expect("Transcript overlay should appear after selecting session");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // === Verify transcript content ===
    let contents = session.screen_contents();

    // Verify both prompts are visible in the transcript
    assert!(
        contents.contains("Alpha"),
        "Transcript should contain first prompt 'Alpha'. Screen:\n{}",
        contents
    );
    assert!(
        contents.contains("Beta"),
        "Transcript should contain second prompt 'Beta'. Screen:\n{}",
        contents
    );

    // Verify assistant responses are visible
    assert!(
        contents.contains("Test message"),
        "Transcript should contain assistant responses. Screen:\n{}",
        contents
    );

    // Snapshot the transcript view for visual verification
    insta::assert_snapshot!(
        "session_transcript_reload",
        normalize_for_input_snapshot(contents)
    );
}

/// Test that /resume-viewonly shows appropriate message when no previous sessions exist.
///
/// When a user opens /resume-viewonly in a fresh environment with no prior sessions,
/// they should see a helpful message indicating no sessions are available.
#[test]
#[cfg(target_os = "linux")]
fn test_resume_viewonly_no_sessions() {
    let config = SessionConfig::new().with_model("mock-model".to_string());

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn TUI");

    // Wait for TUI startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    // Immediately try /resume-viewonly without any prior conversation
    // (the current session shouldn't count as a "previous" session)
    session.send_str("/resume-viewonly").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Should show a message about no previous sessions
    session
        .wait_for(
            |screen| {
                screen.contains("No previous sessions")
                    || screen.contains("no sessions")
                    || screen.contains("No sessions found")
            },
            Duration::from_secs(5),
        )
        .expect("Should indicate no previous sessions available");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    let contents = session.screen_contents();
    insta::assert_snapshot!(
        "resume_viewonly_no_sessions",
        normalize_for_input_snapshot(contents)
    );
}

/// Test that the session picker shows timestamp, first message preview, and message count.
///
/// The session picker should display useful information to help users identify
/// which session they want to view:
/// - Timestamp (when the session started or was last active)
/// - First message preview (truncated if too long)
/// - Message count (number of turns in the conversation)
#[test]
#[cfg(target_os = "linux")]
fn test_session_picker_displays_session_info() {
    let config = SessionConfig::new().with_model("mock-model".to_string());

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn TUI");

    // Wait for TUI startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    // Create a session with identifiable content
    session
        .send_str("Unique identifier prompt for session picker test")
        .unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    session
        .wait_for_text("Test message", Duration::from_secs(5))
        .expect("Should receive response");
    std::thread::sleep(Duration::from_millis(500));

    // Add a second turn to have multiple messages
    session.send_str("Second message in session").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    session
        .wait_for_text("Test message", Duration::from_secs(5))
        .expect("Should receive second response");
    std::thread::sleep(Duration::from_millis(500));

    // Start new session
    session.send_str("/new").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for the /new command popup to show before pressing Enter
    session
        .wait_for_text("/new  start a new chat", Duration::from_secs(2))
        .expect("Command popup should appear");
    std::thread::sleep(TIMEOUT_INPUT);

    session.send_key(Key::Enter).unwrap();

    // Wait for the new session to start - the old conversation should be cleared
    session
        .wait_for(
            |screen| !screen.contains("Unique identifier prompt"),
            Duration::from_secs(10),
        )
        .expect("New session should clear previous conversation");
    std::thread::sleep(Duration::from_millis(500));

    // Open session picker
    session.send_str("/resume-viewonly").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    session
        .wait_for(
            |screen| {
                screen.contains("Select Session")
                    || screen.contains("Previous Sessions")
                    || screen.contains("Unique identifier")
            },
            Duration::from_secs(8),
        )
        .expect("Session picker should appear");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    let contents = session.screen_contents();

    // Verify session info is displayed
    // Should show first message preview
    assert!(
        contents.contains("Unique identifier") || contents.contains("Unique"),
        "Session picker should show first message preview. Screen:\n{}",
        contents
    );

    // Snapshot for visual verification of layout
    insta::assert_snapshot!(
        "session_picker_info",
        normalize_for_input_snapshot(contents)
    );
}
