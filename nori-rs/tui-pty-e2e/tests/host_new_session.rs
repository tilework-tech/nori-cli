//! `/new` starts its session on a server the owned host prepares, in the
//! foreground session's folder. The current session stays in front until the
//! new one starts, then keeps running beside it.
#![cfg(unix)]
#[path = "support/two_agents.rs"]
mod two_agents;
use tui_pty_e2e::TIMEOUT;
use two_agents::*;

#[test]
fn new_starts_beside_the_current_session_on_a_prepared_server() {
    let (home, mut tui) = terminal(&[]);
    ask(&mut tui, "/new");
    tui.wait_for(|_| pids(home.path(), "amber").len() == 2, TIMEOUT)
        .unwrap();
    std::thread::sleep(PAUSE);
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (1).", TIMEOUT)
        .unwrap();
    let amber = pids(home.path(), "amber");
    let second: Vec<String> = std::fs::read_to_string(home.path().join("fixture-traffic-amber"))
        .unwrap()
        .lines()
        .filter(|line| line.starts_with(&amber[1]))
        .map(|line| line.split_once(' ').unwrap().1.to_string())
        .collect();
    assert_eq!(
        second,
        [
            "initialize -",
            "session/list -",
            "session/new -",
            "session/prompt 0"
        ]
    );
    assert!(running(&amber[0]), "the earlier session keeps running");
}

#[test]
fn a_refused_new_session_keeps_the_current_one_in_front() {
    let (home, mut tui) = terminal(&[]);
    std::fs::write(home.path().join("fixture-refuse-new"), "").unwrap();
    ask(&mut tui, "/new");
    tui.wait_for_text("new session unavailable", TIMEOUT)
        .unwrap();
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (2).", TIMEOUT)
        .unwrap();
}
