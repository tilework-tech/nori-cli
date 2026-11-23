use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

#[test]
#[cfg(target_os = "linux")]
fn working_indicator_shows_a_thought_without_a_bold_header() {
    let config = SessionConfig::new()
        .with_agent("mock-model".to_owned())
        .with_mock_response("Reply done")
        .with_agent_env(
            "MOCK_AGENT_THOUGHT",
            "Reading the config loader before changing the default.\nThen the tests.",
        )
        .with_agent_env("MOCK_AGENT_THOUGHT_DELAY_MS", "3000");
    let mut session = TuiSession::spawn_with_config(24, 100, config).expect("spawn nori");
    session.wait_for_text("›", TIMEOUT).expect("composer");
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_str("change the default").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    session
        .wait_for_text(
            "Reading the config loader before changing the default.",
            Duration::from_secs(10),
        )
        .expect("the working indicator shows the thought's first line");
    session
        .wait_for_text("Reply done", TIMEOUT)
        .expect("the turn completes");
}
