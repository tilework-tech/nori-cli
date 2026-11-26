//! Escape with the `/` menu open during a turn: the first press closes the
//! menu and keeps the draft and the turn; the next press interrupts as usual.

use std::time::Duration;

use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

const STREAM: &str = "survey the orchard mock:stream-until-cancel";
const RUNNING: &str = "esc to interrupt";
const MENU_ROW: &str = "/approvals";

#[test]
fn the_first_escape_closes_the_menu_and_the_second_interrupts_the_turn() {
    let mut session = TuiSession::spawn_with_config(40, 110, SessionConfig::new()).expect("spawn");
    session.wait_until_ready().expect("TUI starts");
    std::thread::sleep(TIMEOUT_INPUT);
    session.submit_input(STREAM).unwrap();
    session
        .wait_for_text(RUNNING, TIMEOUT)
        .expect("the turn streams");

    session.send_str("/").unwrap();
    session
        .wait_for_text(MENU_ROW, TIMEOUT)
        .expect("the menu opens");
    session.settle(Duration::from_millis(500));
    session
        .press(
            Key::Escape,
            |screen| !screen.contains(MENU_ROW),
            "the first Escape closes the menu",
        )
        .unwrap();
    session.settle(Duration::from_millis(1000));
    let screen = session.screen_contents();
    assert!(
        screen.contains(RUNNING) && !screen.contains(MENU_ROW),
        "the turn keeps running and the menu stays closed:\n{screen}"
    );
    assert!(
        screen.lines().any(|line| line.trim() == "/"),
        "the draft is kept:\n{screen}"
    );

    session
        .press(
            Key::Escape,
            |screen| !screen.contains(RUNNING),
            "the second Escape interrupts the turn",
        )
        .unwrap();
}
