//! Keys that open and close the full transcript view.

use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

const TRANSCRIPT_TITLE: &str = "T R A N S C R I P T";

#[test]
#[cfg(target_os = "linux")]
fn ctrl_o_toggles_transcript_when_no_plan_is_pinned() {
    let config = SessionConfig::new().with_agent("mock-model".to_owned());
    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("spawn nori");
    session.wait_for_text("›", TIMEOUT).expect("composer");
    std::thread::sleep(TIMEOUT_INPUT);

    session.send_key(Key::Ctrl('o')).unwrap();
    session
        .wait_for_text(TRANSCRIPT_TITLE, TIMEOUT)
        .expect("Ctrl+O opens the transcript");

    session.send_key(Key::Ctrl('o')).unwrap();
    session
        .wait_for(|screen| !screen.contains(TRANSCRIPT_TITLE), TIMEOUT)
        .expect("Ctrl+O closes the transcript");
}
