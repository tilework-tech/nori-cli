//! Keys that open and close the full transcript view and toggle the plan drawer.

use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

const TRANSCRIPT_TITLE: &str = "T R A N S C R I P T";
const PLAN_DRAWER: &str = "Plan: 1/2 completed";

fn session_with_plan() -> TuiSession {
    let config = SessionConfig::new()
        .with_agent("mock-model".to_owned())
        .with_mock_response("Reply done")
        .with_agent_env("MOCK_AGENT_SEND_PLAN", "1");
    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("spawn nori");
    session.wait_for_composer(TIMEOUT).expect("composer");
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_str("make a plan").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session.wait_for_text("Reply done", TIMEOUT).expect("reply");
    std::thread::sleep(TIMEOUT_INPUT);
    session
}

fn assert_ctrl_o_toggles_transcript(session: &mut TuiSession) {
    session.send_key(Key::Ctrl('o')).unwrap();
    session
        .wait_for_text(TRANSCRIPT_TITLE, TIMEOUT)
        .expect("Ctrl+O opens the transcript");

    session.send_key(Key::Ctrl('o')).unwrap();
    session
        .wait_for(|screen| !screen.contains(TRANSCRIPT_TITLE), TIMEOUT)
        .expect("Ctrl+O closes the transcript");
}

#[test]
#[cfg(target_os = "linux")]
fn ctrl_o_toggles_transcript() {
    let config = SessionConfig::new().with_agent("mock-model".to_owned());
    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("spawn nori");
    session.wait_for_composer(TIMEOUT).expect("composer");
    std::thread::sleep(TIMEOUT_INPUT);

    assert_ctrl_o_toggles_transcript(&mut session);
}

#[test]
#[cfg(target_os = "linux")]
fn ctrl_o_toggles_transcript_after_agent_sends_a_plan() {
    let mut session = session_with_plan();

    assert_ctrl_o_toggles_transcript(&mut session);
}

#[test]
#[cfg(target_os = "linux")]
fn alt_p_toggles_the_plan_drawer() {
    let mut session = session_with_plan();
    session.settle(TIMEOUT_INPUT);
    assert!(
        !session.screen_contents().contains(PLAN_DRAWER),
        "drawer starts closed:\n{}",
        session.screen_contents()
    );

    session.send_key(Key::Alt('p')).unwrap();
    session
        .wait_for_text(PLAN_DRAWER, TIMEOUT)
        .expect("Alt+P opens the plan drawer");
    session.settle(TIMEOUT_INPUT);
    assert!(!session.screen_contents().contains(TRANSCRIPT_TITLE));
}
