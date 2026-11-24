//! `/agent` prepares the chosen ACP agent server on the host this terminal
//! owns, lists its saved sessions, and activates the picked row there. The
//! current session stays in front until the host reports the new one live.
#![cfg(unix)]
#[path = "support/two_agents.rs"]
mod two_agents;
use tui_pty_e2e::Key;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;
use two_agents::*;

/// Opens `/agent`, picks Birch (listed after Amber) and waits for its saved-session picker.
fn open_birch_picker(tui: &mut TuiSession) {
    ask(tui, "/agent");
    tui.wait_for_text("Amber (current)", TIMEOUT).unwrap();
    tui.send_key(Key::Down).unwrap();
    std::thread::sleep(PAUSE);
    tui.send_key(Key::Enter).unwrap();
    tui.wait_for_text("saved-1", TIMEOUT).unwrap();
}

#[test]
fn switching_restores_a_saved_session_of_the_selected_agent() {
    let (home, mut tui) = terminal(&[]);
    open_birch_picker(&mut tui);
    tui.send_key(Key::Down).unwrap();
    std::thread::sleep(PAUSE);
    tui.send_key(Key::Enter).unwrap();
    tui.wait_for_text("saved input", TIMEOUT).unwrap();
    ask(&mut tui, "whoami");
    tui.wait_for_text("birch is answering (1).", TIMEOUT)
        .unwrap();
    assert_eq!(
        trace(home.path(), "birch"),
        [
            "initialize -",
            "session/list -",
            "session/load saved-1",
            "session/prompt saved-1"
        ]
    );
    assert_eq!(pids(home.path(), "birch").len(), 1);
}

#[test]
fn dismissing_the_switch_picker_keeps_the_current_session() {
    let (home, mut tui) = terminal(&[]);
    open_birch_picker(&mut tui);
    let birch = pids(home.path(), "birch");
    tui.send_key(Key::Escape).unwrap();
    tui.wait_for(|_| !running(&birch[0]), TIMEOUT).unwrap();
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (2).", TIMEOUT)
        .unwrap();
    assert_eq!(
        trace(home.path(), "birch"),
        ["initialize -", "session/list -"]
    );
}

#[test]
fn a_refused_restore_keeps_the_current_session() {
    let (home, mut tui) = terminal(&["fixture-refuse-load"]);
    open_birch_picker(&mut tui);
    tui.send_key(Key::Down).unwrap();
    std::thread::sleep(PAUSE);
    tui.send_key(Key::Enter).unwrap();
    tui.wait_for_text("saved session unavailable", TIMEOUT)
        .unwrap();
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (2).", TIMEOUT)
        .unwrap();
    let birch = pids(home.path(), "birch");
    tui.wait_for(|_| !running(&birch[0]), TIMEOUT).unwrap();
}

#[test]
fn switching_to_a_new_session_keeps_the_previous_one_running() {
    let (home, mut tui) = terminal(&[]);
    open_birch_picker(&mut tui);
    tui.send_key(Key::Enter).unwrap();
    tui.wait_for_text("Started new conversation with agent: Birch", TIMEOUT)
        .unwrap();
    ask(&mut tui, "whoami");
    tui.wait_for_text("birch is answering (1).", TIMEOUT)
        .unwrap();
    assert_eq!(
        trace(home.path(), "birch"),
        [
            "initialize -",
            "session/list -",
            "session/new -",
            "session/prompt 0"
        ]
    );
    let amber = pids(home.path(), "amber");
    assert_eq!(amber.len(), 1);
    assert!(
        running(&amber[0]),
        "the previous session keeps running beside it"
    );
}
