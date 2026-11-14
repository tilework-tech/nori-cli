//! The session list and background requests, behind `[features] multi_session`.

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

const ALPHA_ANSWER: &str = "alpha agent answer";
const BETA_ANSWER: &str = "beta agent answer";
const BACKGROUND_REQUEST: &str = "mock:request-permission:rm -rf target/debug";

fn config() -> SessionConfig {
    SessionConfig::new()
        .with_extra_config_toml("[features]\nmulti_session = true\n")
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL", ALPHA_ANSWER)
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL_ALT", BETA_ANSWER)
        .with_agent_env("MOCK_AGENT_PERMISSION_DELAY_MS", "1500")
}

fn spawn() -> TuiSession {
    let mut session = TuiSession::spawn_with_config(40, 110, config()).expect("spawn");
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);
    session
}

fn ask(session: &mut TuiSession, prompt: &str, answer: &str) {
    session.submit_input(prompt).unwrap();
    session
        .wait_for_text(answer, TIMEOUT)
        .unwrap_or_else(|e| panic!("{answer:?} should arrive: {e}"));
    session.settle(Duration::from_millis(300));
}

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

fn start_new_session(session: &mut TuiSession) {
    session.submit_input("/new").unwrap();
    session
        .wait_for(
            |screen| screen.contains("Mock Default Model") && screen.contains("›"),
            TIMEOUT,
        )
        .expect("a new session should start");
    session.settle(Duration::from_millis(500));
}

/// Three sessions: a finished alpha session, a beta session whose request
/// arrives while it is in the background, and a finished gamma session on
/// screen.
fn three_sessions() -> TuiSession {
    let mut session = spawn();
    ask(&mut session, "alpha question", ALPHA_ANSWER);
    start_alt_session(&mut session);
    session.submit_input(BACKGROUND_REQUEST).unwrap();
    session.settle(Duration::from_millis(200));
    session.send_key(Key::Alt(',')).unwrap();
    session
        .wait_for_text("1 needs you", Duration::from_secs(8))
        .expect("the beta request should arrive in the background");
    start_new_session(&mut session);
    ask(&mut session, "gamma question", ALPHA_ANSWER);
    session
}

#[test]
#[cfg(target_os = "linux")]
fn left_on_an_empty_prompt_lists_sessions_with_status_and_preview() {
    let mut session = three_sessions();
    session.send_key(Key::Left).unwrap();
    session
        .wait_for_text("Sessions 3", Duration::from_secs(3))
        .expect("Left on an empty prompt opens the session list");
    session.settle(Duration::from_millis(300));
    let screen = session.screen_contents();
    for expected in [
        "alpha question",
        "approve: rm -rf target/debug",
        "needs you",
        "gamma question",
        "(current)",
    ] {
        assert!(screen.contains(expected), "{expected:?} missing:\n{screen}");
    }
    insta::assert_snapshot!(
        "session_list_three_sessions",
        normalize_for_input_snapshot(screen)
    );
}

#[test]
#[cfg(target_os = "linux")]
fn tab_shows_only_sessions_waiting_on_you_and_typing_filters() {
    let mut session = three_sessions();
    session.send_key(Key::Alt('s')).unwrap();
    session
        .wait_for_text("Sessions 3", Duration::from_secs(3))
        .expect("Alt+S opens the session list");

    session.send_key(Key::Tab).unwrap();
    session
        .wait_for(
            |screen| screen.contains("tab: all sessions") && !screen.contains("○ gamma question"),
            Duration::from_secs(3),
        )
        .expect("Tab keeps only the session waiting on you");
    session.settle(Duration::from_millis(200));
    insta::assert_snapshot!(
        "session_list_waiting_on_you",
        normalize_for_input_snapshot(session.screen_contents())
    );
    session.send_key(Key::Tab).unwrap();

    for c in "gamma".chars() {
        session.send_key(Key::Char(c)).unwrap();
    }
    session
        .wait_for(
            |screen| screen.contains("○ gamma question") && !screen.contains("○ alpha question"),
            Duration::from_secs(3),
        )
        .expect("typing filters by title");

    session.send_key(Key::Escape).unwrap();
    session
        .wait_for_text("○ alpha question", Duration::from_secs(3))
        .expect("Esc clears the filter");
    session.send_key(Key::Escape).unwrap();
    session
        .wait_for(
            |screen| !screen.contains("Sessions 3"),
            Duration::from_secs(3),
        )
        .expect("a second Esc closes the list");
}

#[test]
#[cfg(target_os = "linux")]
fn enter_in_the_list_jumps_to_that_session() {
    let mut session = three_sessions();
    session.submit_input("/sessions").unwrap();
    session
        .wait_for_text("Sessions 3", Duration::from_secs(3))
        .expect("/sessions opens the session list");
    session.send_key(Key::Up).unwrap();
    session.send_key(Key::Up).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for(
            |screen| screen.contains("alpha question") && !screen.contains("gamma question"),
            Duration::from_secs(5),
        )
        .expect("Enter shows the chosen session");
}

#[test]
#[cfg(target_os = "linux")]
fn session_list_is_absent_without_the_flag() {
    let mut session = TuiSession::spawn_with_config(30, 100, SessionConfig::new()).expect("spawn");
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Left).unwrap();
    session.send_key(Key::Alt('s')).unwrap();
    session.type_input("/sess").unwrap();
    session.settle(Duration::from_millis(500));
    let screen = session.screen_contents();
    assert!(!screen.contains("Sessions"), "{screen}");
    assert!(!screen.contains("/sessions"), "{screen}");
}
