//! With `[tui] program_status = true`, plain `nori` reports the foreground
//! session's status to its terminal as OSC 7501 program status reports:
//! idle, working, blocked, done or error. A report is written only when the
//! status changes and is cleared when the TUI exits.
#![cfg(target_os = "linux")]

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::has_composer_row;
use tui_pty_e2e::program_status_reports;
use tui_pty_e2e::wait_for_program_status;

const ANSWER: &str = "The tide table lists four high tides.";
const ENABLED: &str = "[tui]\nprogram_status = true\n";
const IDLE: &str = "state=idle:app=nori";
const WORKING: &str = "state=working:app=nori";
const DONE: &str = "state=done:app=nori";
const ERROR: &str = "state=error:app=nori";
const CLEAR: &str = "state=clear";
/// `Approval needed`, base64-encoded as the protocol requires.
const BLOCKED: &str = "state=blocked:app=nori:kind=permission:msg=QXBwcm92YWwgbmVlZGVk";

fn spawn(config: SessionConfig) -> TuiSession {
    let mut session = TuiSession::spawn_with_config(30, 100, config).expect("spawn");
    session.wait_until_ready().expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);
    session
}

fn enabled() -> SessionConfig {
    SessionConfig::new()
        .with_mock_response(ANSWER)
        .with_extra_config_toml(ENABLED)
}

fn reports(session: &TuiSession) -> Vec<String> {
    program_status_reports(session.raw_output())
}

/// Submit `prompt` and wait for the turn's own done report.
fn ask(session: &mut TuiSession, prompt: &str) {
    let finished = |session: &TuiSession| reports(session).iter().filter(|r| *r == DONE).count();
    let before = finished(session);
    session.submit_input(prompt).unwrap();
    let deadline = std::time::Instant::now() + TIMEOUT;
    while finished(session) <= before {
        assert!(
            std::time::Instant::now() < deadline,
            "{prompt:?} should finish, saw {:?}:\n{}",
            reports(session),
            session.screen_contents()
        );
        session.settle(Duration::from_millis(50));
    }
}

/// Press Ctrl+L and wait until the TUI is (or is no longer) full-screen.
fn switch_screen(session: &mut TuiSession, full_screen: bool) {
    session.send_key(Key::Ctrl('l')).unwrap();
    let deadline = std::time::Instant::now() + TIMEOUT;
    while session.is_alternate_screen() != full_screen {
        assert!(
            std::time::Instant::now() < deadline,
            "Ctrl+L should switch the view:\n{}",
            session.screen_contents()
        );
        session.settle(Duration::from_millis(50));
    }
    session.settle(Duration::from_millis(300));
}

/// Every OSC 0 window title written so far.
fn titles(session: &TuiSession) -> Vec<String> {
    String::from_utf8_lossy(session.raw_output())
        .split("\u{1b}]0;")
        .skip(1)
        .map(|rest| rest.split("\u{1b}\\").next().unwrap_or(rest).to_string())
        .collect()
}

fn quit(session: &mut TuiSession) {
    session.send_key(Key::Ctrl('d')).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Ctrl('d')).unwrap();
    assert!(session.wait_for_process_exit(TIMEOUT), "nori should exit");
    session.settle(Duration::from_millis(200));
}

#[test]
fn a_turn_through_an_approval_reports_each_state_once_and_clears_on_exit() {
    let mut session = spawn(enabled());
    session
        .submit_input("mock:request-permission:cargo test --offline")
        .unwrap();
    session
        .wait_for_text("Would you like to allow", TIMEOUT)
        .expect("approval prompt");
    wait_for_program_status(&mut session, BLOCKED);
    session.wait_out_answer_grace();
    session.send_key(Key::Char('1')).unwrap();
    session
        .wait_for_text("Permission granted with option: allow", TIMEOUT)
        .unwrap();
    wait_for_program_status(&mut session, DONE);
    quit(&mut session);

    assert_eq!(
        reports(&session),
        vec![IDLE, WORKING, BLOCKED, WORKING, DONE, CLEAR]
    );
    let titles = titles(&session);
    assert!(!titles.is_empty(), "the TUI names the tab");
    assert!(
        titles
            .iter()
            .all(|title| !title.contains("Approval") && !title.contains("blocked")),
        "the tab title never carries the status: {titles:?}"
    );
}

#[test]
fn reports_are_off_unless_configured() {
    let mut session = spawn(SessionConfig::new().with_mock_response(ANSWER));
    session.submit_input("how many high tides").unwrap();
    session.wait_for_text(ANSWER, TIMEOUT).expect("answer");
    session.settle(Duration::from_millis(500));
    quit(&mut session);

    assert_eq!(reports(&session), Vec::<String>::new());
}

#[test]
fn opted_in_blocked_reports_carry_the_approval_title() {
    let mut session = spawn(
        SessionConfig::new()
            .with_mock_response(ANSWER)
            .with_extra_config_toml(
                "[tui]\nprogram_status = true\nprogram_status_approval_titles = true\n",
            ),
    );
    session.submit_input("mock:request-permission").unwrap();
    // `Execute shell command`, the mock agent's approval title, base64-encoded.
    wait_for_program_status(
        &mut session,
        "state=blocked:app=nori:kind=permission:msg=RXhlY3V0ZSBzaGVsbCBjb21tYW5k",
    );
}

#[test]
fn a_failed_turn_reports_an_error() {
    let mut session = spawn(
        SessionConfig::new()
            .with_extra_config_toml(ENABLED)
            .with_agent_env("MOCK_AGENT_PROMPT_FAIL", "1"),
    );
    session.submit_input("check the tide gauge").unwrap();
    wait_for_program_status(&mut session, ERROR);
    assert_eq!(reports(&session), vec![IDLE, WORKING, ERROR]);
}

#[test]
fn a_cancelled_turn_reports_idle() {
    let mut session = spawn(
        SessionConfig::new()
            .with_stream_until_cancel()
            .with_extra_config_toml(ENABLED),
    );
    session.submit_input("keep reading the gauge").unwrap();
    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("turn starts");
    session
        .press(
            Key::Escape,
            |screen| screen.contains("Conversation interrupted"),
            "turn cancelled",
        )
        .unwrap();
    session.settle(Duration::from_millis(300));
    assert_eq!(reports(&session), vec![IDLE, WORKING, IDLE]);
}

#[test]
fn done_survives_typing_and_screen_switches_until_the_next_prompt() {
    let mut session = spawn(enabled());
    ask(&mut session, "how many high tides");

    session.type_input("draft about the tides").unwrap();
    session.settle(Duration::from_millis(300));
    switch_screen(&mut session, true);
    switch_screen(&mut session, false);
    assert_eq!(reports(&session), vec![IDLE, WORKING, DONE]);

    for _ in "draft about the tides".chars() {
        session.send_key(Key::Backspace).unwrap();
    }
    ask(&mut session, "and the low tides");
    assert_eq!(reports(&session), vec![IDLE, WORKING, DONE, WORKING, DONE]);
}

#[test]
fn foreground_switches_report_the_shown_session_and_background_sessions_stay_silent() {
    let mut session = spawn(enabled().with_agent_env("MOCK_AGENT_PERMISSION_DELAY_MS", "1500"));
    ask(&mut session, "how many high tides");
    session.submit_input("/new").unwrap();
    session
        .wait_for(
            |screen| screen.contains("Mock Default Model") && has_composer_row(screen),
            TIMEOUT,
        )
        .expect("a new session should start");
    wait_for_program_status(&mut session, IDLE);
    session
        .submit_input("mock:request-permission:cargo build")
        .unwrap();
    session.settle(Duration::from_millis(200));
    session.send_key(Key::Alt(',')).unwrap();
    wait_for_program_status(&mut session, DONE);
    session
        .wait_for_text("1 waiting on you", Duration::from_secs(8))
        .expect("the request arrives in the background");
    session.settle(Duration::from_millis(300));
    assert_eq!(reports(&session).last().map(String::as_str), Some(DONE));
    assert!(
        !reports(&session).iter().any(|report| report == BLOCKED),
        "a background request is not reported: {:?}",
        reports(&session)
    );

    session.send_key(Key::Alt('.')).unwrap();
    wait_for_program_status(&mut session, BLOCKED);
}
