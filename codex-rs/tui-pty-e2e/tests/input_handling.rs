use insta::assert_snapshot;
use std::time::Duration;
use tui_pty_e2e::normalize_for_snapshot;
use tui_pty_e2e::Key;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;

#[test]
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

    std::thread::sleep(TIMEOUT_INPUT);
    std::thread::sleep(TIMEOUT_INPUT);
    std::thread::sleep(TIMEOUT_INPUT);
    assert_snapshot!(
        "ctrl_c_clears",
        normalize_for_snapshot(session.screen_contents())
    );
}

#[test]
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

    std::thread::sleep(TIMEOUT_INPUT);
    std::thread::sleep(TIMEOUT_INPUT);
    std::thread::sleep(TIMEOUT_INPUT);
    assert_snapshot!(
        "typing_and_backspace",
        normalize_for_snapshot(session.screen_contents())
    );
}

#[test]
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

    std::thread::sleep(TIMEOUT_INPUT);
    std::thread::sleep(TIMEOUT_INPUT);
    std::thread::sleep(TIMEOUT_INPUT);
    assert_snapshot!(
        "model_changed",
        normalize_for_snapshot(session.screen_contents())
    );
}
