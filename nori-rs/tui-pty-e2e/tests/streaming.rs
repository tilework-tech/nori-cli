use insta::assert_snapshot;
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

    // Wait for streaming to start (status indicator appears with interrupt hint)
    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("Conversation did not start");

    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Escape).unwrap();
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // Verify cancellation completed
    // (exact behavior depends on TUI implementation)
    session
        .wait_for_text(
            "Conversation interrupted - tell the model what to do differently",
            TIMEOUT,
        )
        .expect("No interrupt reported");

    // There are timing issues for when the "Streaming..." chunk shows up,
    // that make a snapshot here very flaky. Rely on the above assert for now
    // assert_snapshot!(
    //     "escape_cancelled_stream",
    //     normalize_for_input_snapshot(session.screen_contents())
    // )
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

    // Wait for streaming to start (status indicator appears with interrupt hint)
    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("Conversation did not start");

    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Ctrl('c')).unwrap();
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // Verify cancellation completed
    // (exact behavior depends on TUI implementation)
    session
        .wait_for_text(
            "Conversation interrupted - tell the model what to do differently",
            TIMEOUT,
        )
        .expect("No interrupt reported");

    // There are timing issues for when the "Streaming..." chunk shows up,
    // that make a snapshot here very flaky. Rely on the above assert for now
    // assert_snapshot!(
    //     "ctrl_c_cancelled_stream",
    //     normalize_for_input_snapshot(session.screen_contents())
    // )
}

#[test]
#[cfg(target_os = "linux")]
fn test_prompt_submitted_during_cancelling_is_not_lost() {
    let config = SessionConfig::new().with_stream_until_cancel();
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    session.send_str("first try").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("First prompt should become interruptible");

    session.send_key(Key::Escape).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_str("queued follow up").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    session
        .wait_for_text(
            "Conversation interrupted - tell the model what to do differently",
            TIMEOUT,
        )
        .expect("Interrupted turn should finish before the queued prompt is sent");
    session
        .wait_for_text("queued follow up", TIMEOUT)
        .expect("Queued follow-up prompt should stay visible");
    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("Queued follow-up prompt should eventually start once ACP is idle");
}

#[test]
#[cfg(target_os = "linux")]
fn test_prompt_still_streams_after_interrupt() {
    let config = SessionConfig::new().with_stream_until_cancel();
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    session.send_str("first try").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("First prompt should become interruptible");

    session.send_key(Key::Escape).unwrap();
    session
        .wait_for_text(
            "Conversation interrupted - tell the model what to do differently",
            TIMEOUT,
        )
        .expect("First prompt should report interrupt");
    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt should return after first interrupt");

    session.send_str("second try").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("Second prompt should also become interruptible after the first cancel");

    session.send_key(Key::Escape).unwrap();
    session
        .wait_for_text(
            "Conversation interrupted - tell the model what to do differently",
            TIMEOUT,
        )
        .expect("Second prompt should also be interruptible");
}

#[test]
#[cfg(target_os = "linux")]
fn test_prompt_after_interrupt_absorbs_empty_end_turn_tail() {
    let config = SessionConfig::new()
        .with_stream_until_cancel()
        .with_agent_env("MOCK_AGENT_CANCEL_TAIL_EMPTY_END_TURNS", "2")
        .with_agent_env(
            "MOCK_AGENT_CANCEL_TAIL_FOLLOW_UP_RESPONSE",
            "Recovered after cancel tail",
        );
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    session.send_str("first try").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("First prompt should become interruptible");

    session.send_key(Key::Escape).unwrap();
    session
        .wait_for_text(
            "Conversation interrupted - tell the model what to do differently",
            TIMEOUT,
        )
        .expect("First prompt should report interrupt");
    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt should return after first interrupt");

    session.send_str("what have you finished?").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    session
        .wait_for_text("Recovered after cancel tail", TIMEOUT)
        .expect("The follow-up prompt should still produce a real response after the stale end_turn tail");
    session
        .wait_for(
            |screen| {
                screen.contains("Recovered after cancel tail")
                    && !screen.contains("esc to interrupt")
            },
            TIMEOUT,
        )
        .expect("The follow-up prompt should settle instead of getting stuck in another interruptible turn");
}
