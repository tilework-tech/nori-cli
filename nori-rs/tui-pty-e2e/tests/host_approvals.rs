//! `/approvals` in plain `nori` sets the host mode of the session on screen.
//! The footer follows the session on screen, and `/new` starts at the host
//! default instead of copying the mode.
#![cfg(unix)]
#[path = "support/two_agents.rs"]
mod two_agents;
use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;
use two_agents::*;

fn wait_mode(tui: &mut TuiSession, name: &str) {
    let label = format!("Approvals: {name}");
    tui.wait_for(
        |screen| {
            screen
                .lines()
                .rev()
                .take(3)
                .any(|line| line.contains(&label))
        },
        TIMEOUT,
    )
    .unwrap_or_else(|_| panic!("footer never showed {label}:\n{}", tui.screen_contents()));
}

/// A session in plain Agent mode keeps the default footer quiet.
fn wait_agent(tui: &mut TuiSession) {
    tui.wait_for(
        |screen| {
            !screen
                .lines()
                .rev()
                .take(3)
                .any(|line| line.contains("Approvals:"))
        },
        TIMEOUT,
    )
    .unwrap_or_else(|_| panic!("footer still shows a mode:\n{}", tui.screen_contents()));
}

fn mode_changes(tui: &TuiSession) -> usize {
    host_requests(tui)
        .iter()
        .filter(|method| *method == "session/set_mode")
        .count()
}

fn choose(tui: &mut TuiSession, key: Key) {
    ask(tui, "/approvals");
    tui.wait_for_text("Approvals for this session", TIMEOUT)
        .unwrap();
    tui.send_key(key).unwrap();
    tui.settle(Duration::from_millis(200));
    tui.send_key(Key::Enter).unwrap();
}

#[test]
fn the_footer_follows_the_session_on_screen_and_new_sessions_start_at_the_host_default() {
    let (_home, mut tui) = terminal(&[]);
    wait_agent(&mut tui);

    choose(&mut tui, Key::Down);
    tui.wait_for_text("Approve every request in this session?", TIMEOUT)
        .unwrap();
    let before_confirming = mode_changes(&tui);
    tui.send_key(Key::Enter).unwrap();
    wait_mode(&mut tui, "Approve all");
    assert_eq!(before_confirming, 0, "nothing changes before confirming");
    assert_eq!(mode_changes(&tui), 1, "one change reaches the host");

    ask(&mut tui, "/new");
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (1).", TIMEOUT)
        .unwrap();
    wait_agent(&mut tui);

    tui.send_key(Key::Alt(',')).unwrap();
    wait_mode(&mut tui, "Approve all");
    tui.send_key(Key::Alt('.')).unwrap();
    wait_agent(&mut tui);
}

#[test]
fn ask_is_refused_for_an_adapter_that_cannot_ask_and_the_mode_stays() {
    let (_home, mut tui) = terminal(&[]);
    wait_agent(&mut tui);

    choose(&mut tui, Key::Up);

    tui.wait_for_text(
        "has no mode that asks before every edit and command",
        TIMEOUT,
    )
    .unwrap();
    wait_agent(&mut tui);
}
