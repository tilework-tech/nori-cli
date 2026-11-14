//! Several sessions in one TUI, behind `[features] multi_session`.
//!
//! Each session runs on its own mock agent process. The scrollback always
//! belongs to the session on screen: moving to another session clears it and
//! reprints that session's history.

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

const ALPHA_ANSWER: &str = "alpha agent answer";
const BETA_ANSWER: &str = "beta agent answer";

fn multi_session_config() -> SessionConfig {
    SessionConfig::new()
        .with_extra_config_toml("[features]\nmulti_session = true\n")
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL", ALPHA_ANSWER)
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL_ALT", BETA_ANSWER)
}

fn spawn(config: SessionConfig) -> TuiSession {
    let mut session = TuiSession::spawn_with_config(30, 100, config).expect("spawn");
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);
    session
}

/// Start a second session on the alternate mock agent through `/agent`.
fn start_alt_session(session: &mut TuiSession) {
    session.submit_input("/agent").unwrap();
    session
        .wait_for(
            |screen| screen.contains("Select agent") || screen.contains("mock-model"),
            Duration::from_secs(8),
        )
        .expect("agent picker should open");
    session.send_key(Key::Down).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Start a new session", Duration::from_secs(10))
        .expect("candidate session picker should open");
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text(
            "Started new conversation with agent: Mock ACP Alt",
            Duration::from_secs(10),
        )
        .expect("the alternate session should start");
    std::thread::sleep(TIMEOUT_INPUT);
}

fn ask(session: &mut TuiSession, prompt: &str, answer: &str) {
    session.submit_input(prompt).unwrap();
    session
        .wait_for_text(answer, TIMEOUT)
        .unwrap_or_else(|e| panic!("{answer:?} should arrive: {e}"));
    session.settle(Duration::from_millis(300));
}

/// Send a key and wait until the full history satisfies `pred`.
fn wait_for_history(session: &mut TuiSession, pred: impl Fn(&str) -> bool, what: &str) -> String {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        session.settle(Duration::from_millis(100));
        let history = session.history_contents();
        if pred(&history) {
            return history;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{what}; history was:\n{history}"
        );
    }
}

#[test]
#[cfg(target_os = "linux")]
fn session_moves_reprint_only_the_target_and_keep_unsent_input() {
    let mut session = spawn(multi_session_config());
    ask(&mut session, "first alpha question", ALPHA_ANSWER);

    start_alt_session(&mut session);
    let history = session.history_contents();
    assert!(
        !history.contains(ALPHA_ANSWER),
        "a new session starts on a clean screen:\n{history}"
    );
    ask(&mut session, "first beta question", BETA_ANSWER);

    session.type_input("unsent beta draft").unwrap();

    session.send_key(Key::Alt(',')).unwrap();
    let history = wait_for_history(
        &mut session,
        |h| h.contains("first alpha question") && !h.contains("first beta question"),
        "previous_session should show only the alpha session",
    );
    assert!(!history.contains("unsent beta draft"), "{history}");

    ask(&mut session, "second alpha question", ALPHA_ANSWER);

    session.send_key(Key::Alt('.')).unwrap();
    let history = wait_for_history(
        &mut session,
        |h| h.contains("first beta question") && !h.contains("alpha question"),
        "next_session should show only the beta session",
    );
    assert!(
        session.screen_contents().contains("unsent beta draft"),
        "the beta session keeps its unsent input:\n{history}"
    );

    session.send_key(Key::Alt('.')).unwrap();
    let history = wait_for_history(
        &mut session,
        |h| h.contains("second alpha question") && !h.contains("beta question"),
        "next_session wraps around to the alpha session",
    );
    assert_eq!(
        history.matches("first alpha question").count(),
        1,
        "each alpha turn appears once after the reprint:\n{history}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn session_keys_do_nothing_without_the_flag() {
    let config = SessionConfig::new()
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL", ALPHA_ANSWER)
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL_ALT", BETA_ANSWER);
    let mut session = spawn(config);
    ask(&mut session, "first alpha question", ALPHA_ANSWER);
    start_alt_session(&mut session);
    ask(&mut session, "first beta question", BETA_ANSWER);

    session.send_key(Key::Alt(',')).unwrap();
    session.settle(Duration::from_secs(1));
    let history = session.history_contents();
    assert!(
        history.contains("first alpha question") && history.contains("first beta question"),
        "single-session use keeps appending one history:\n{history}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn new_keeps_the_previous_session_running_behind_the_new_one() {
    let mut session = spawn(multi_session_config());
    ask(&mut session, "question before new", ALPHA_ANSWER);

    session.submit_input("/new").unwrap();
    wait_for_history(
        &mut session,
        |h| !h.contains("question before new") && h.contains("Mock Default Model"),
        "/new should start a session on a clean screen",
    );
    ask(&mut session, "question after new", ALPHA_ANSWER);

    session.send_key(Key::Alt(',')).unwrap();
    wait_for_history(
        &mut session,
        |h| h.contains("question before new") && !h.contains("question after new"),
        "the first session is still there behind the new one",
    );
}

#[test]
#[cfg(target_os = "linux")]
fn status_line_counts_sessions_by_status() {
    let mut session = spawn(multi_session_config());
    ask(&mut session, "finished question", ALPHA_ANSWER);

    session.submit_input("/new").unwrap();
    session
        .wait_for_text("Mock Default Model", TIMEOUT)
        .expect("a second session should start");
    std::thread::sleep(Duration::from_millis(500));
    session.submit_input("mock:stream-until-cancel").unwrap();
    session
        .wait_for_text("1 working · 1 ready", TIMEOUT)
        .expect("the status line counts one working and one ready session");
}

#[test]
#[cfg(target_os = "linux")]
fn one_session_shows_no_status_line() {
    let mut session = spawn(multi_session_config());
    ask(&mut session, "only question", ALPHA_ANSWER);
    session.settle(Duration::from_millis(500));
    let screen = session.screen_contents();
    assert!(!screen.contains("1 ready"), "{screen}");
}
