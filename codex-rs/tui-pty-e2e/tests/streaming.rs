use insta::assert_snapshot;
use tui_pty_e2e::normalize_for_input_snapshot;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;

#[test]
fn test_submit_text() {
    let config = SessionConfig::new().with_stream_until_cancel();
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    // Submit prompt
    session.send_str("testing!!!").unwrap();
    session.wait_for_text("testing!!!", TIMEOUT).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    std::thread::sleep(TIMEOUT_INPUT);
    session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    assert_snapshot!(
        "submit_input",
        normalize_for_input_snapshot(session.screen_contents())
    );
}

#[test]
#[ignore]
// TODO: this was broken by the new TUI event loop calls to AcpBackend.
// Need to fix, and support the cancellation Op with the new TUI event loop
fn test_escape_cancels_streaming() {
    // Use git_init to prevent "Snapshots disabled" from racing with "Working" status
    let config = SessionConfig::new().with_stream_until_cancel();
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    // Wait for the prompt to appear (indicated by the chevron character)
    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    // Submit prompt
    session.send_str("testing!!!").unwrap();
    session.wait_for_text("testing!!!", TIMEOUT).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for streaming to start
    session
        .wait_for_text("Working", TIMEOUT)
        .expect("Streaming did not start");

    // Press Escape to cancel doesn't work?
    session.send_key(Key::Escape).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    // Press ctrl-c to cancel doesn't work?
    // session.send_key(Key::Ctrl('c')).unwrap();
    // std::thread::sleep(TIMEOUT_INPUT);

    std::thread::sleep(TIMEOUT);
    // Verify cancellation completed
    // (exact behavior depends on TUI implementation)
    session
        .wait_for_text(
            "Conversation interrupted - tell the model what to do differently",
            TIMEOUT,
        )
        .expect("No interrupt reported");

    assert_snapshot!(
        "cancelled_stream",
        normalize_for_input_snapshot(session.screen_contents())
    )
}
