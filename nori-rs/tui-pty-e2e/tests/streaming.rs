use insta::assert_snapshot;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

#[test]
#[cfg(target_os = "linux")]
fn test_submit_text() {
    let config = SessionConfig::new().with_stream_until_cancel();
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    session.wait_until_ready().expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    // Submit prompt
    session.submit_input("testing!!!").unwrap();

    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("Conversation did not start");

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
    session.wait_until_ready().expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    // Submit prompt
    session.submit_input("testing!!!").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for streaming to start (status indicator appears with interrupt hint)
    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("Conversation did not start");

    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Escape).unwrap();

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
    session.wait_until_ready().expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    // Submit prompt
    session.submit_input("testing!!!").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for streaming to start (status indicator appears with interrupt hint)
    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("Conversation did not start");

    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Ctrl('c')).unwrap();

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

    session.wait_until_ready().expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    session.submit_input("first try").unwrap();
    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("First prompt should become interruptible");

    session.send_key(Key::Escape).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.submit_input("queued follow up").unwrap();

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

    session.wait_until_ready().expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    session.submit_input("first try").unwrap();
    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("First prompt should become interruptible");

    session
        .press(
            Key::Escape,
            |screen| {
                screen.contains("Conversation interrupted - tell the model what to do differently")
            },
            "First prompt should report interrupt",
        )
        .unwrap();
    session
        .wait_for_composer()
        .expect("Prompt should return after first interrupt");

    session.submit_input("second try").unwrap();
    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("Second prompt should also become interruptible after the first cancel");

    session
        .press(
            Key::Escape,
            |screen| {
                screen.contains("Conversation interrupted - tell the model what to do differently")
            },
            "Second prompt should also be interruptible",
        )
        .unwrap();
}

#[test]
#[cfg(target_os = "linux")]
fn direct_busy_enter_reports_fallback_and_tab_explicitly_queues() {
    let mut session = TuiSession::spawn_with_config(32, 110, SessionConfig::new()).unwrap();
    session.wait_until_ready().unwrap();
    session.submit_input("mock:stream-until-cancel").unwrap();
    session.wait_for_text("esc to interrupt", TIMEOUT).unwrap();
    session.submit_input("Chart the orchard first").unwrap();
    session
        .wait_for_text("Steering unavailable", TIMEOUT)
        .unwrap();
    session.type_input("Count the cedar trees").unwrap();
    session.send_key(Key::Tab).unwrap();
    session
        .wait_for_text("Queued for the next turn", TIMEOUT)
        .unwrap();
}

#[path = "support/steering.rs"]
mod steering;

#[test]
#[cfg(target_os = "linux")]
fn direct_enter_delivers_to_the_original_turn_then_tab_drains_once() {
    let config = SessionConfig::new().with_agent_env("MOCK_AGENT_STEERING", "1");
    let mut session = TuiSession::spawn_with_config(34, 110, config).unwrap();
    session.wait_until_ready().unwrap();
    steering::exercise(&mut session);
}
