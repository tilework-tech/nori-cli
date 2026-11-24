//! Verifier scratch test (not part of any PR): a session the owned host
//! starts with `/new` keeps receiving its own child-session traffic — a head
//! fork that continues under a new conversation, and a delegated child's
//! result. Copy into nori-rs/tui-pty-e2e/tests/ of the head under test.
#![cfg(target_os = "linux")]
use std::time::Duration;
use std::time::Instant;

use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::Key;

const ANSWER: &str = "lantern orchard answer";

fn spawn() -> TuiSession {
    let config = SessionConfig::new()
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL", ANSWER)
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL_ALT", "harbor child answer")
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_FORK", "1");
    let mut session = TuiSession::spawn_with_config(40, 120, config).expect("spawn");
    session.wait_for_text("›", TIMEOUT).expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);
    session
}

fn ask(session: &mut TuiSession, prompt: &str, answer: &str) {
    let before = session.history_contents().matches(answer).count();
    session.submit_input(prompt).unwrap();
    let deadline = Instant::now() + TIMEOUT;
    while session.history_contents().matches(answer).count() <= before {
        assert!(
            Instant::now() < deadline,
            "{answer:?} should arrive for {prompt:?}:\n{}",
            session.screen_contents()
        );
        session.settle(Duration::from_millis(50));
    }
    session.settle(Duration::from_millis(300));
}

fn new_session(session: &mut TuiSession) {
    ask(session, "first orchard question", ANSWER);
    session.submit_input("/new").unwrap();
    session.wait_for_text("2 ready", TIMEOUT).expect("/new starts a second session");
    session.settle(Duration::from_millis(800));
    ask(session, "question in the new session", ANSWER);
}

#[test]
fn a_new_session_follows_its_head_fork() {
    let mut session = spawn();
    new_session(&mut session);
    session.submit_input("/fork").unwrap();
    session.wait_for_text("Branch from current point", TIMEOUT).unwrap();
    session.send_key(Key::Enter).unwrap();
    session.wait_for_text("Session forked.", TIMEOUT).expect("head fork");
    ask(&mut session, "after the head fork", ANSWER);
}

#[test]
fn a_new_session_shows_its_delegated_childs_result() {
    let mut session = spawn();
    new_session(&mut session);
    session.submit_input("/delegate mock-model inspect the harbor").unwrap();
    session
        .wait_for(
            |screen| screen.contains("result from mock-model") && screen.contains(" · done"),
            TIMEOUT,
        )
        .expect("the delegated child's result reaches the session that delegated it");
}
