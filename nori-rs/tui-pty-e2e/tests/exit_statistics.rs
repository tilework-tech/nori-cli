use insta::assert_snapshot;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TIMEOUT_PRESNAPSHOT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

/// Test that exit message appears when quitting with Ctrl+D
#[test]
#[cfg(target_os = "linux")]
fn test_exit_message_displays_on_ctrl_d() {
    let config = SessionConfig::new().with_mock_response("Sure, I'll help you with that!");

    let mut session = TuiSession::spawn_with_config(30, 100, config).unwrap();

    // Wait for the prompt to appear
    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    // Submit a test prompt to generate some session activity
    session.send_str("hello world").unwrap();
    session.wait_for_text("hello world", TIMEOUT).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for the response to complete
    session
        .wait_for_text("Sure, I'll help you with that!", TIMEOUT)
        .expect("Response did not appear");
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // Quit the session with Ctrl+D (press twice - first clears quit hint, second triggers exit)
    session.send_key(Key::Ctrl('d')).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Ctrl('d')).unwrap();
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // Verify exit message components appear
    session
        .wait_for_text("Goodbye!", TIMEOUT)
        .expect("Exit message 'Goodbye!' did not appear");

    session
        .wait_for_text("Session:", TIMEOUT)
        .expect("Session ID label did not appear");

    session
        .wait_for_text("Messages", TIMEOUT)
        .expect("Messages section did not appear");

    session
        .wait_for_text("Tool Calls", TIMEOUT)
        .expect("Tool Calls section did not appear");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // Snapshot the final screen
    assert_snapshot!(
        "exit_message_ctrl_d",
        normalize_for_input_snapshot(session.screen_contents())
    );
}

/// Test that exit message appears when using /exit command
#[test]
#[cfg(target_os = "linux")]
fn test_exit_message_displays_on_slash_exit() {
    let config = SessionConfig::new().with_mock_response("Sure, I'll help you with that!");

    let mut session = TuiSession::spawn_with_config(30, 100, config).unwrap();

    // Wait for the prompt to appear
    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    // Submit a test prompt to generate some session activity
    session.send_str("hello world").unwrap();
    session.wait_for_text("hello world", TIMEOUT).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for the response to complete
    session
        .wait_for_text("Sure, I'll help you with that!", TIMEOUT)
        .expect("Response did not appear");
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // Quit the session with /exit command
    session.send_str("/exit").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // Verify exit message components appear
    session
        .wait_for_text("Goodbye!", TIMEOUT)
        .expect("Exit message 'Goodbye!' did not appear");

    session
        .wait_for_text("Session:", TIMEOUT)
        .expect("Session ID label did not appear");

    session
        .wait_for_text("Messages", TIMEOUT)
        .expect("Messages section did not appear");

    session
        .wait_for_text("Tool Calls", TIMEOUT)
        .expect("Tool Calls section did not appear");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // Snapshot the final screen
    assert_snapshot!(
        "exit_message_slash_exit",
        normalize_for_input_snapshot(session.screen_contents())
    );
}
