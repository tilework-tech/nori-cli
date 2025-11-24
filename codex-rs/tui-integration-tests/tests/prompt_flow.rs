use insta::assert_snapshot;
use std::time::Duration;
use tui_integration_tests::Key;
use tui_integration_tests::SessionConfig;
use tui_integration_tests::TuiSession;

const TIMEOUT: Duration = Duration::from_secs(10);

#[test]
fn test_submit_prompt_default_response() {
    let mut session = TuiSession::spawn(24, 80).expect("Failed to spawn codex");

    session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();

    // session.send_str("/model").unwrap();
    // std::thread::sleep(Duration::from_millis(200));
    // session.wait_for_text("/model", TIMEOUT).unwrap();
    // session.send_key(Key::Enter).unwrap();
    // std::thread::sleep(Duration::from_millis(100));
    // assert_snapshot!("list_models", session.screen_contents());
    // session.send_key(Key::Escape).unwrap();
    // std::thread::sleep(Duration::from_millis(100));

    // Type prompt
    session.send_str("Hello").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    session.wait_for_text("Hello", TIMEOUT).unwrap();

    // Submit
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // // Wait for default mock responses
    // // (extra long waits because the ACP can have retries, and we want the final err)
    // session
    //     .wait_for_text("Test message 1", Duration::from_secs(25))
    //     .expect("Did not receive mock response");
    // session
    //     .wait_for_text("Test message 2", TIMEOUT)
    //     .expect("Did not receive second mock response");
    session
        .wait_for_text("Test message", Duration::from_secs(15))
        .unwrap();

    assert_snapshot!("prompt_submitted", session.screen_contents());
}

#[test]
fn test_submit_prompt_missing_model() {
    let mut session = TuiSession::spawn_with_config(
        24,
        80,
        SessionConfig::new().with_model("nonexistent".to_owned()),
    )
    .expect("Failed to spawn codex");

    session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();

    // Type prompt
    session.send_str("Hello").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    session.wait_for_text("Hello", TIMEOUT).unwrap();

    // Submit
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    session
        .wait_for_text(
            "ACP agent config error: Unknown ACP model: nonexistent-acp",
            Duration::from_secs(10),
        )
        .unwrap();

    assert_snapshot!("missing_model", session.screen_contents());
}

// #[test]
// fn test_submit_prompt_custom_response() {
//     let config = SessionConfig::new()
//         .with_mock_response("This is a custom test response from the mock agent.");
//
//     let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn codex");
//
//     session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();
//
//     session.send_str("test prompt").unwrap();
//     std::thread::sleep(Duration::from_millis(100));
//     session.send_key(Key::Enter).unwrap();
//     std::thread::sleep(Duration::from_millis(100));
//
//     session
//         .wait_for_text("This is a custom test response", Duration::from_secs(10))
//         .expect("Did not receive custom response");
//
//     assert_snapshot!("custom_response", session.screen_contents());
// }
//
// #[test]
// fn test_multiline_input() {
//     let mut session = TuiSession::spawn(24, 80).unwrap();
//     session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();
//
//     // Type multiline prompt
//     session.send_str("Line 1").unwrap();
//     session.send_key(Key::Enter).unwrap();
//     session.send_str("Line 2").unwrap();
//     session.send_key(Key::Enter).unwrap();
//     session.send_str("Line 3").unwrap();
//
//     // Verify all lines visible
//     session.wait_for_text("Line 1", TIMEOUT).unwrap();
//     session.wait_for_text("Line 2", TIMEOUT).unwrap();
//     session.wait_for_text("Line 3", TIMEOUT).unwrap();
// }
