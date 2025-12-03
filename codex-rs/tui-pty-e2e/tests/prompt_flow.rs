use insta::assert_snapshot;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TIMEOUT_PRESNAPSHOT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

#[test]
fn test_submit_prompt_default_response() {
    let mut session = TuiSession::spawn(18, 80).expect("Failed to spawn codex");

    session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();

    // Type prompt
    session.send_str("Hello").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.wait_for_text("Hello", TIMEOUT).unwrap();

    // Submit
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for default mock responses
    // (extra long waits because the ACP can have retries, and we want the final err)
    session
        .wait_for_text("Test message 1", TIMEOUT)
        .expect("Did not receive mock response");
    session
        .wait_for_text("Test message 2", TIMEOUT)
        .expect("Did not receive second mock response");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    assert_snapshot!(
        "prompt_submitted",
        normalize_for_input_snapshot(session.screen_contents())
    );
}

#[test]
#[ignore]
// TODO: this falls back on an HTTP model.
// Need to fix this after we have a purely ACP launch mode config in place.
fn test_submit_prompt_missing_model() {
    let mut session = TuiSession::spawn_with_config(
        18,
        80,
        SessionConfig::new().with_model("nonexistent".to_owned()),
    )
    .expect("Failed to spawn codex");

    session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();

    // Type prompt
    session.send_str("Hello").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.wait_for_text("Hello", TIMEOUT).unwrap();

    // Submit
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    session
        .wait_for_text(
            "Model 'nonexistent' has wire_api=acp but is not registered",
            TIMEOUT,
        )
        .unwrap();

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    assert_snapshot!(
        "missing_model",
        normalize_for_input_snapshot(session.screen_contents())
    );
}

#[test]
fn test_submit_prompt_custom_response() {
    let config = SessionConfig::new()
        .with_mock_response("This is a custom test response from the mock agent.");

    let mut session = TuiSession::spawn_with_config(18, 80, config).expect("Failed to spawn codex");

    session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();

    session.send_str("test prompt").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    session
        .wait_for_text("This is a custom test response", TIMEOUT)
        .expect("Did not receive custom response");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    assert_snapshot!(
        "custom_response",
        normalize_for_input_snapshot(session.screen_contents())
    );
}

#[test]
fn test_multiline_input() {
    let mut session = TuiSession::spawn(30, 80).unwrap();
    session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();

    // Type multiline prompt
    session.send_str("Line 1\nLine 2\nLine 3").unwrap();

    // Verify all lines visible
    session.wait_for_text("Line 1", TIMEOUT).unwrap();
    session.wait_for_text("Line 2", TIMEOUT).unwrap();
    session.wait_for_text("Line 3", TIMEOUT).unwrap();

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    assert_snapshot!(
        "multiline_input",
        normalize_for_input_snapshot(session.screen_contents())
    );
}
