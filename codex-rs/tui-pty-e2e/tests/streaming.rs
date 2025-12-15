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

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    assert_snapshot!(
        "submit_input",
        normalize_for_input_snapshot(session.screen_contents())
    );
}

#[test]
#[cfg(target_os = "linux")]
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

    std::thread::sleep(TIMEOUT);
    session.send_key(Key::Escape).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Verify cancellation completed
    // (exact behavior depends on TUI implementation)
    session
        .wait_for_text(
            "Conversation interrupted - tell the model what to do differently",
            TIMEOUT,
        )
        .expect("No interrupt reported");

    session.wait_for(
        |contents| !contents.contains("• Streaming..."),
        Duration::from_secs(10)
    ).expect("Streaming did not finish");

    assert_snapshot!(
        "escape_cancelled_stream",
        normalize_for_input_snapshot(session.screen_contents())
    )
}

#[test]
#[cfg(target_os = "linux")]
fn test_ctrl_c_cancels_streaming() {
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

    std::thread::sleep(TIMEOUT);
    session.send_key(Key::Ctrl('c')).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Verify cancellation completed
    // (exact behavior depends on TUI implementation)
    session
        .wait_for_text(
            "Conversation interrupted - tell the model what to do differently",
            TIMEOUT,
        )
        .expect("No interrupt reported");

    session.wait_for(
        |contents| !contents.contains("• Streaming..."),
        Duration::from_secs(10)
    ).expect("Streaming did not finish");

    assert_snapshot!(
        "ctrl_c_cancelled_stream",
        normalize_for_input_snapshot(session.screen_contents())
    )
}
