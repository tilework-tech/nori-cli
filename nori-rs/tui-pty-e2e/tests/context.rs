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
    assert!(
        screen.contains("Turn checkpoints"),
        "checkpoint history: {screen}"
    );
    assert!(screen.contains("Start"), "start checkpoint: {screen}");
    assert!(screen.contains("End"), "end checkpoint: {screen}");
}

#[test]
fn context_grid_alternates_turn_segments_without_adding_a_prompt() {
    let mut session = TuiSession::spawn_with_config(
        60,
        110,
        SessionConfig::new()
            .with_agent_env("MOCK_AGENT_MULTI_TURN", "1")
            .with_agent_env(
                "MOCK_AGENT_CONTEXT_SEQUENCE",
                r#"[[{"used":10000,"size":100000}],[{"used":30000,"size":100000}],[{"used":50000,"size":100000}]]"#,
            ),
    )
    .expect("start Nori");
    session.wait_for_text("›", TIMEOUT).expect("composer");
    for marker in ["ALPHA", "BETA", "GAMMA"] {
        session.submit_input(marker).expect("submit prompt");
        session
            .wait_for_text(&format!("RESPONSE_{marker}"), TIMEOUT)
            .expect("response");
    }
    session.submit_input("/context").expect("inspect context");
    session
        .wait_for_text("50,000 / 100,000 tokens", TIMEOUT)
        .expect("latest observation");
    let screen = session.screen_contents();
    assert!(
        screen.contains("3 user turns"),
        "no inspection prompt: {screen}"
    );
    assert!(screen.contains("Turn checkpoints"), "history: {screen}");
    assert!(
        screen.contains('▦') && screen.contains('▤'),
        "alternating segments: {screen}"
    );
    assert!(screen.contains("20,000"), "current-turn growth: {screen}");
}

#[test]
fn context_can_inspect_an_unfinished_turn_without_interrupting_it() {
    let fixture = tempfile::tempdir().expect("fixture directory");
    let release = fixture.path().join("release");
    let mut session = TuiSession::spawn_with_config(
        60,
        110,
        SessionConfig::new()
            .with_agent_env("MOCK_AGENT_CONTEXT_USAGE", "1")
            .with_agent_env("MOCK_AGENT_CONTEXT_RELEASE_FILE", release.to_str().unwrap())
            .with_mock_response("Completed after inspection."),
    )
    .expect("start Nori");
    session.wait_for_text("›", TIMEOUT).expect("composer");
    session.submit_input("hello").expect("start prompt");
    session
        .wait_for_text("34%", TIMEOUT)
        .expect("usage before completion");
    session
        .submit_input("/context")
        .expect("inspect active turn");
    session
        .wait_for_text("Turn checkpoints", TIMEOUT)
        .expect("checkpoint snapshot during turn");
    let screen = session.screen_contents();
    assert!(
        screen.contains("running"),
        "unfinished end checkpoint: {screen}"
    );
    assert!(
        screen.contains("1 user turn"),
        "inspection stays local: {screen}"
    );
    std::fs::write(release, "continue").expect("release prompt");
    session
        .wait_for_text("Completed after inspection.", TIMEOUT)
        .expect("original prompt completes");
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
