//! Delegation traverses the real manager, child process, and terminal.
use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

fn spawn(enabled: bool) -> TuiSession {
    let config = SessionConfig::new()
        .with_extra_config_toml(format!("[features]\nmulti_session = {enabled}\n"))
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL", "parent final answer")
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL_ALT", "child final answer");
    let mut session = TuiSession::spawn_with_config(40, 130, config).unwrap();
    session
        .wait_for_text("Mock Default Model", TIMEOUT)
        .unwrap();
    session.submit_input("parent task identity").unwrap();
    session
        .wait_for_text("parent final answer", TIMEOUT)
        .unwrap();
    session
}

#[test]
fn same_kind_child_starts_and_returns_without_approval_or_exiting_terminal() {
    let mut session = spawn(true);
    session
        .submit_input("/delegate mock-model inspect the parser")
        .unwrap();
    session
        .wait_for(
            |screen| screen.contains("completed") || screen.contains("Completed"),
            TIMEOUT,
        )
        .unwrap();
    assert!(
        !session
            .screen_contents()
            .contains("Start delegated session")
    );
    session.submit_input("/sessions").unwrap();
    session
        .wait_for_text("inspect the parser", TIMEOUT)
        .unwrap();
    session.send_key(Key::Down).unwrap();
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for(
            |screen| {
                screen.contains("inspect the parser") && !screen.contains("parent task identity")
            },
            TIMEOUT,
        )
        .unwrap();
    session
        .wait_for_text("parent final answer", TIMEOUT)
        .unwrap();
    session.send_key(Key::Alt(',')).unwrap();
    session
        .wait_for_text("parent task identity", TIMEOUT)
        .unwrap();
    let before = session
        .history_contents()
        .matches("parent final answer")
        .count();
    session.submit_input("still usable").unwrap();
    let deadline = std::time::Instant::now() + TIMEOUT;
    while session
        .history_contents()
        .matches("parent final answer")
        .count()
        <= before
    {
        assert!(
            std::time::Instant::now() < deadline,
            "parent must answer after child completion"
        );
        session.settle(Duration::from_millis(50));
    }
}

#[test]
fn different_kind_child_needs_start_and_its_permission_stays_in_child() {
    let mut session = spawn(true);
    session
        .submit_input("/delegate mock-model-alt mock:request-permission:write-child-file")
        .unwrap();
    session
        .wait_for_text("Start delegated session", TIMEOUT)
        .unwrap();
    assert!(
        !session
            .screen_contents()
            .contains("Would you like to allow")
    );
    session.send_key(Key::Char('1')).unwrap();
    session
        .wait_for(
            |screen| screen.contains("waiting on you") && screen.contains("parent"),
            TIMEOUT,
        )
        .unwrap();
    assert!(
        !session
            .screen_contents()
            .contains("Would you like to allow"),
        "child permission must not open in parent"
    );
    session.send_key(Key::AltDown).unwrap();
    session
        .wait_for_text("Would you like to allow", TIMEOUT)
        .unwrap();
    session.wait_for_text("write-child-file", TIMEOUT).unwrap();
    session.send_key(Key::Char('1')).unwrap();
    session
        .wait_for_text("Permission granted with option: allow", TIMEOUT)
        .unwrap();
    session.send_key(Key::Alt(',')).unwrap();
    session
        .wait_for(
            |screen| screen.contains("completed") || screen.contains("Completed"),
            TIMEOUT,
        )
        .unwrap();
    session
        .wait_for(
            |screen| {
                screen.contains("parent task identity")
                    && screen.contains("Permission granted with option: allow")
            },
            TIMEOUT,
        )
        .expect("attributed result is visible in parent after child redraw");
}

#[test]
fn declining_different_kind_does_not_start_child() {
    let mut session = spawn(true);
    session
        .submit_input("/delegate mock-model-alt inspect parser without starting a child")
        .unwrap();
    session
        .wait_for_text("Start delegated session", TIMEOUT)
        .unwrap();
    session.send_key(Key::Char('2')).unwrap();
    session
        .wait_for_text("The user declined this delegation.", TIMEOUT)
        .unwrap();
    session
        .wait_for_text("Delegation continuation received:", TIMEOUT)
        .unwrap();
    session
        .wait_for(
            |screen| !screen.contains("Working") && !screen.contains("Start delegated session"),
            TIMEOUT,
        )
        .unwrap();
    session.submit_input("/sessions").unwrap();
    assert!(!session.screen_contents().contains("child final answer"));
    session.wait_for_text("Sessions 1", TIMEOUT).unwrap();
}

#[test]
fn delegation_is_unavailable_without_multi_session() {
    let mut session = spawn(false);
    session
        .submit_input("/delegate mock-model-alt should not run")
        .unwrap();
    session
        .wait_for_text("Unrecognized command '/delegate'", TIMEOUT)
        .unwrap();
    assert!(
        !session
            .screen_contents()
            .contains("Start delegated session")
    );
}
