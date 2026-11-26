use insta::assert_snapshot;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

#[test]
#[cfg(target_os = "linux")]
fn test_submit_prompt_default_response() {
    let mut session = TuiSession::spawn(18, 80).expect("Failed to spawn");

    session.wait_until_ready().unwrap();

    // Type and submit prompt
    session.submit_input("Hello").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for default mock responses
    // (extra long waits because the ACP can have retries, and we want the final err)
    session
        .wait_for_text("Test message 1", TIMEOUT)
        .expect("Did not receive mock response");
    session
        .wait_for_text("Test message 2", TIMEOUT)
        .expect("Did not receive second mock response");

    assert_snapshot!(
        "prompt_submitted",
        normalize_for_input_snapshot(session.settled_screen())
    );
}

#[test]
#[cfg(target_os = "linux")]
fn test_submit_prompt_custom_response() {
    let config = SessionConfig::new()
        .with_mock_response("This is a custom test response from the mock agent.");

    let mut session = TuiSession::spawn_with_config(18, 80, config).expect("Failed to spawn");

    session.wait_until_ready().unwrap();

    session.submit_input("test prompt").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    session
        .wait_for_text("This is a custom test response", TIMEOUT)
        .expect("Did not receive custom response");

    assert_snapshot!(
        "custom_response",
        normalize_for_input_snapshot(session.settled_screen())
    );
}

#[test]
fn test_multiline_input() {
    let mut session = TuiSession::spawn(30, 80).unwrap();
    session.wait_until_ready().unwrap();

    // Type multiline prompt
    session.type_input("Line 1\nLine 2\nLine 3").unwrap();

    // Verify all lines visible
    session.wait_for_text("Line 1", TIMEOUT).unwrap();
    session.wait_for_text("Line 2", TIMEOUT).unwrap();
    session.wait_for_text("Line 3", TIMEOUT).unwrap();

    let normalized = normalize_for_input_snapshot(session.settled_screen());
    let composer = normalized
        .lines()
        .skip_while(|line| !line.contains("Line 1"))
        .take(3)
        .collect::<Vec<_>>()
        .join("\n");
    assert_snapshot!("multiline_input", composer);
}
