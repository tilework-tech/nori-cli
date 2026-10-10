use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

#[test]
fn context_command_renders_agent_reported_usage() {
    let mut session = TuiSession::spawn_with_config(
        45,
        100,
        SessionConfig::new()
            .with_agent_env("MOCK_AGENT_CONTEXT_USAGE", "1")
            .with_mock_response("Context fixture ready."),
    )
    .expect("start Nori");
    session.wait_for_text("›", TIMEOUT).expect("composer");
    session.submit_input("hello").expect("submit prompt");
    session
        .wait_for_text("Context fixture ready.", TIMEOUT)
        .expect("response");
    session.submit_input("/context").expect("submit command");
    session
        .wait_for_text("340,412 / 1,000,000 tokens", TIMEOUT)
        .expect("reported usage");
    let screen = session.screen_contents();
    assert!(screen.contains("34.0% used"), "percentage: {screen}");
    assert!(
        screen.contains("1 user turn"),
        "inspection must not add a turn: {screen}"
    );
}

#[test]
fn context_command_renders_unknown_usage_without_prompting_the_agent() {
    let mut session = TuiSession::spawn(45, 100).expect("start Nori");
    session.wait_for_text("›", TIMEOUT).expect("composer");
    session.submit_input("/context").expect("submit command");
    session
        .wait_for_text("Waiting for context usage", TIMEOUT)
        .expect("unknown usage");
    session
        .wait_for_text("Skills observed", TIMEOUT)
        .expect("context inventory");
    let screen = session.screen_contents();
    assert!(
        screen.contains("0 user turns"),
        "slash command must not become a user turn: {screen}"
    );
    assert!(screen.contains("›"), "composer remains available: {screen}");
}
