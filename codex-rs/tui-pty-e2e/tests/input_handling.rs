use insta::assert_snapshot;
use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TIMEOUT_PRESNAPSHOT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

#[test]
#[cfg(target_os = "linux")]
fn test_ctrl_c_clears_input() {
    let mut session = TuiSession::spawn(24, 80).unwrap();
    session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();

    // Type some text
    session.send_str("draft message").unwrap();
    session.wait_for_text("draft message", TIMEOUT).unwrap();

    // Ctrl-C should clear
    session.send_key(Key::Ctrl('c')).unwrap();

    // Verify cleared
    session
        .wait_for(|s| !s.contains("draft message"), TIMEOUT)
        .expect("Input was not cleared");

    // std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    // assert_snapshot!(
    //     "ctrl_c_clears",
    //     normalize_for_input_snapshot(session.screen_contents())
    // );
}

#[test]
#[cfg(target_os = "linux")]
fn test_backspace() {
    let mut session = TuiSession::spawn(24, 80).unwrap();
    session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();

    session.send_str("Hello").unwrap();
    session.wait_for_text("Hello", TIMEOUT).unwrap();

    // Backspace twice
    session.send_key(Key::Backspace).unwrap();
    session.send_key(Key::Backspace).unwrap();

    // Should have "Hel" remaining
    session.wait_for_text("Hel", TIMEOUT).unwrap();
    session.wait_for(|s| !s.contains("Hello"), TIMEOUT).unwrap();

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    assert_snapshot!(
        "typing_and_backspace",
        normalize_for_input_snapshot(session.screen_contents())
    );
}

#[test]
#[cfg(target_os = "linux")]
fn test_arrows() {
    let mut session = TuiSession::spawn(40, 80).unwrap();
    session.wait_for_text("›", TIMEOUT).unwrap();

    session.send_str("/model").unwrap();
    session.wait_for_text("/model", TIMEOUT).unwrap();

    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    session.send_key(Key::Down).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    session.send_key(Key::Down).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    assert_snapshot!(
        "model_changed",
        normalize_for_input_snapshot(session.screen_contents())
    );
}

#[test]
#[cfg(target_os = "linux")]
fn test_history_navigation_up_down() {
    // Spawn session with mock response
    let config = SessionConfig::new().with_mock_response("Mock response");
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    // Wait for TUI to be ready
    session.wait_for_text("›", TIMEOUT).unwrap();

    // Submit first message
    session.send_str("first message").unwrap();
    session.wait_for_text("first message", TIMEOUT).unwrap();
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for response to complete
    session.wait_for_text("Mock response", TIMEOUT).unwrap();
    session
        .wait_for(|screen| !screen.contains("esc to interrupt"), TIMEOUT)
        .unwrap();

    // Wait for prompt to be ready again
    std::thread::sleep(TIMEOUT_INPUT);

    // Press Up arrow to navigate to previous message
    session.send_key(Key::Up).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Verify "first message" is loaded back into input
    session.wait_for_text("› first message", TIMEOUT).unwrap();

    // Press Down arrow to clear history navigation (should return to placeholder)
    session.send_key(Key::Down).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Verify we're back to the placeholder (input cleared from history navigation)
    // Check that the LAST line starting with › does NOT contain "first message"
    // (earlier lines may contain it from conversation history)
    session
        .wait_for(
            |s| {
                let lines: Vec<&str> = s.lines().collect();
                // Find the last line that contains ›
                if let Some(last_prompt) = lines.iter().rev().find(|line| line.contains("›")) {
                    // Verify it doesn't contain our history text
                    !last_prompt.contains("first message")
                } else {
                    false
                }
            },
            TIMEOUT,
        )
        .unwrap();

    // Snapshot the final state
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    assert_snapshot!(
        "history_navigation_up_down",
        normalize_for_input_snapshot(session.screen_contents())
    );
}

#[test]
#[cfg(target_os = "linux")]
fn test_history_navigation_multiple_messages() {
    // Spawn session with mock response
    let config = SessionConfig::new().with_mock_response("Mock response");
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    // Wait for TUI to be ready
    session.wait_for_text("›", TIMEOUT).unwrap();

    // Submit first message
    session.send_str("first message").unwrap();
    session.wait_for_text("› first message", TIMEOUT).unwrap();
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for response
    session.wait_for_text("Mock response", TIMEOUT).unwrap();
    session
        .wait_for(|screen| !screen.contains("esc to interrupt"), TIMEOUT)
        .unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Submit second message
    session.send_str("second message").unwrap();
    session.wait_for_text("› second message", TIMEOUT).unwrap();
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for response
    session.wait_for_text("Mock response", TIMEOUT).unwrap();
    session
        .wait_for(|screen| !screen.contains("esc to interrupt"), TIMEOUT)
        .unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Press Up once - should show "› second message"
    session.send_key(Key::Up).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.wait_for_text("› second message", TIMEOUT).unwrap();

    // Press Up again - should show "› first message"
    session.send_key(Key::Up).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.wait_for_text("› first message", TIMEOUT).unwrap();

    // Press Up again - should be a no-op (stay at "› first message")
    session.send_key(Key::Up).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Verify still showing "› first message"
    session.wait_for_text("› first message", TIMEOUT).unwrap();

    // Snapshot the final state
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    assert_snapshot!(
        "history_navigation_multiple_messages",
        normalize_for_input_snapshot(session.screen_contents())
    );
}
