use insta::assert_snapshot;
use std::time::Duration;
use std::time::Instant;
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

/// Test that HTTP models produce immediate errors when allow_http_fallback=false.
///
/// When HTTP mode is disabled (the default), using an HTTP-only model like
/// gpt-5.1-codex-mini should produce an immediate error at startup, NOT
/// after submitting a prompt, and should NOT go through retry loops.
#[test]
fn test_http_model_immediate_error_without_retries() {
    let start = Instant::now();

    let mut session = TuiSession::spawn_with_config(
        18,
        80,
        SessionConfig::new().with_model("gpt-5.1-codex-mini".to_owned()),
    )
    .expect("Failed to spawn codex");

    // The error should appear immediately at startup, not after prompt submission
    session
        .wait_for_text("is not registered as an ACP agent", TIMEOUT)
        .expect("Should show ACP registration error");

    let elapsed = start.elapsed();

    // Verify the error appeared quickly (< 6 seconds) - proving no retry loops
    // This accounts for PTY spawn overhead (~4s) but would catch retry loops (10+ seconds)
    // If there were 5 retries with backoff, this would take 10+ seconds
    assert!(
        elapsed < Duration::from_secs(6),
        "Error took {:?} to appear - suggests retries are happening. Expected immediate error.",
        elapsed
    );

    std::thread::sleep(TIMEOUT_INPUT);
    assert_snapshot!(
        "http_model_immediate_error",
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
