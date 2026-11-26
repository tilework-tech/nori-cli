//! `!` commands in plain `nori`: the session's host runs them in the
//! session's folder, and the command and its output show in the transcript.

use tui_pty_e2e::Key;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

#[test]
#[cfg(target_os = "linux")]
fn a_bang_command_shows_its_output_in_the_transcript() {
    let mut session = TuiSession::spawn(24, 100).unwrap();
    session.wait_for_composer(TIMEOUT).unwrap();
    session.wait_for_text("Mock ACP options", TIMEOUT).unwrap();

    session.send_str("!echo lantern-$((6*7))").unwrap();
    session.wait_for_text("lantern-$((6*7))", TIMEOUT).unwrap();
    session.send_key(Key::Enter).unwrap();

    session
        .wait_for(
            |screen| screen.contains("Ran echo lantern-$((6*7))") && screen.contains("lantern-42"),
            TIMEOUT,
        )
        .unwrap_or_else(|_| panic!("no shell output:\n{}", session.screen_contents()));
}
