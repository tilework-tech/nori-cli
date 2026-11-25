use insta::assert_snapshot;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TIMEOUT_PRESNAPSHOT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

/// Token usage the mock agent reports with every prompt response.
const PROMPT_USAGE: &str =
    r#"{"totalTokens":12480,"inputTokens":9120,"outputTokens":3360,"cachedReadTokens":8000}"#;

/// The exit card lands in scrollback with the agent's reported usage, and the
/// last line of output is only the resume command.
fn assert_exit_card(session: &mut TuiSession) {
    for text in [
        "session ended",
        "Messages",
        "Tokens",
        "12,480  total",
        "Context",
    ] {
        session
            .wait_for_text(text, TIMEOUT)
            .unwrap_or_else(|_| panic!("exit card did not show {text:?}"));
    }
    session
        .wait_for_text("nori resume", TIMEOUT)
        .expect("resume command did not appear");
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    let screen = session.screen_contents();
    let last = screen
        .lines()
        .map(str::trim_end)
        .rfind(|line| !line.is_empty())
        .unwrap_or_default();
    assert!(
        last.starts_with("nori resume "),
        "the last line must be only the resume command: {screen}"
    );
}

/// Test that the exit card appears when quitting with Ctrl+D
#[test]
#[cfg(target_os = "linux")]
fn test_exit_message_displays_on_ctrl_d() {
    let config = SessionConfig::new()
        .with_mock_response("Sure, I'll help you with that!")
        .with_agent_env("MOCK_AGENT_PROMPT_USAGE", PROMPT_USAGE)
        .with_agent_env("MOCK_AGENT_CONTEXT_USAGE", "1");

    let mut session = TuiSession::spawn_with_config(30, 100, config).unwrap();

    // Wait for the prompt to appear
    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    // Submit a test prompt to generate some session activity
    session.submit_input("hello world").unwrap();
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

    assert_exit_card(&mut session);

    // Snapshot the final screen
    assert_snapshot!(
        "exit_message_ctrl_d",
        normalize_for_input_snapshot(session.screen_contents())
    );
}

/// Test that the exit card appears when using /exit command
#[test]
#[cfg(target_os = "linux")]
fn test_exit_message_displays_on_slash_exit() {
    let config = SessionConfig::new()
        .with_mock_response("Sure, I'll help you with that!")
        .with_agent_env("MOCK_AGENT_PROMPT_USAGE", PROMPT_USAGE)
        .with_agent_env("MOCK_AGENT_CONTEXT_USAGE", "1");

    let mut session = TuiSession::spawn_with_config(30, 100, config).unwrap();

    // Wait for the prompt to appear
    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    // Submit a test prompt to generate some session activity
    session.submit_input("hello world").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for the response to complete
    session
        .wait_for_text("Sure, I'll help you with that!", TIMEOUT)
        .expect("Response did not appear");
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // Quit the session with /exit command
    session.submit_input("/exit").unwrap();
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    assert_exit_card(&mut session);

    // Snapshot the final screen
    assert_snapshot!(
        "exit_message_slash_exit",
        normalize_for_input_snapshot(session.screen_contents())
    );
}
