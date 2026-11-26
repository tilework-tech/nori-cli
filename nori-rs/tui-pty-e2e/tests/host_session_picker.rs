//! `/resume` and the picker after the last `/close` list the current agent's
//! saved sessions through a server the owned host prepares, and restore the
//! picked row there.
#![cfg(unix)]
#[path = "support/two_agents.rs"]
mod two_agents;
use tui_pty_e2e::Key;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;
use two_agents::*;

fn pick_saved_row(tui: &mut TuiSession) {
    tui.wait_for_text("saved-1", TIMEOUT).unwrap();
    tui.send_key(Key::Down).unwrap();
    tui.settle(PAUSE);
    tui.send_key(Key::Enter).unwrap();
}

#[test]
fn resume_restores_a_saved_session_on_a_freshly_prepared_server() {
    let (home, mut tui) = terminal(&[]);
    ask(&mut tui, "/resume");
    pick_saved_row(&mut tui);
    tui.wait_for_text("saved input", TIMEOUT).unwrap();
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (1).", TIMEOUT)
        .unwrap();
    let amber = pids(home.path(), "amber");
    assert_eq!(amber.len(), 2, "the restore runs on its own server");
    let restored: Vec<String> = std::fs::read_to_string(home.path().join("fixture-traffic-amber"))
        .unwrap()
        .lines()
        .filter(|line| line.starts_with(&amber[1]))
        .map(|line| line.split_once(' ').unwrap().1.to_string())
        .collect();
    assert_eq!(
        restored,
        [
            "initialize -",
            "session/list -",
            "session/load saved-1",
            "session/prompt saved-1"
        ]
    );
}

#[test]
fn dismissing_resume_keeps_the_current_session_and_reaps_the_listing_server() {
    let (home, mut tui) = terminal(&[]);
    ask(&mut tui, "/resume");
    tui.wait_for_text("saved-1", TIMEOUT).unwrap();
    let amber = pids(home.path(), "amber");
    assert_eq!(amber.len(), 2);
    tui.send_key(Key::Escape).unwrap();
    tui.wait_for(|_| !running(&amber[1]), TIMEOUT).unwrap();
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (2).", TIMEOUT)
        .unwrap();
    assert!(!tui.screen_contents().contains("Agent switch cancelled"));
}

#[test]
fn a_refused_resume_reports_the_agent_and_keeps_the_current_session() {
    let (_home, mut tui) = terminal(&["fixture-refuse-load"]);
    ask(&mut tui, "/resume");
    pick_saved_row(&mut tui);
    tui.wait_for_text("saved session unavailable", TIMEOUT)
        .unwrap();
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (2).", TIMEOUT)
        .unwrap();
}

#[test]
fn closing_the_last_session_offers_saved_sessions_to_restore() {
    let (home, mut tui) = terminal(&[]);
    ask(&mut tui, "/close");
    pick_saved_row(&mut tui);
    tui.wait_for_text("saved input", TIMEOUT).unwrap();
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (1).", TIMEOUT)
        .unwrap();
    assert_eq!(pids(home.path(), "amber").len(), 2);
}

#[test]
fn dismissing_the_picker_after_the_last_close_says_how_to_continue() {
    let (_home, mut tui) = terminal(&[]);
    ask(&mut tui, "/close");
    tui.wait_for_text("saved-1", TIMEOUT).unwrap();
    tui.send_key(Key::Escape).unwrap();
    tui.wait_for_text("No session is active", TIMEOUT).unwrap();
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (1).", TIMEOUT)
        .unwrap();
}
