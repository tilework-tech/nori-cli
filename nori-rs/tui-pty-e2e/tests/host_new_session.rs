//! `/new` starts its session on a server the owned host prepares, in the
//! foreground session's folder. The current session stays in front until the
//! new one starts, then keeps running beside it.
#![cfg(unix)]
#[path = "support/two_agents.rs"]
mod two_agents;
use tui_pty_e2e::TIMEOUT;
use two_agents::*;

#[test]
fn input_typed_right_after_new_goes_to_the_new_session() {
    let (home, mut tui) = terminal(&[]);
    std::fs::write(home.path().join("fixture-slow-init"), "").unwrap();
    ask(&mut tui, "/new");
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (1).", TIMEOUT)
        .unwrap();
    let amber = pids(home.path(), "amber");
    let second = methods_of(home.path(), &amber[1]);
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
    assert_eq!(
        methods_of(home.path(), &amber[0])
            .iter()
            .filter(|m| m.starts_with("session/prompt"))
            .count(),
        1
    );
}

#[test]
fn a_refused_new_session_returns_the_held_input_without_sending_it() {
    let (home, mut tui) = terminal(&[]);
    std::fs::write(home.path().join("fixture-refuse-new"), "").unwrap();
    std::fs::write(home.path().join("fixture-slow-init"), "").unwrap();
    ask(&mut tui, "/new");
    ask(&mut tui, "whoami");
    tui.wait_for_text("new session unavailable", TIMEOUT)
        .unwrap();
    tui.wait_for(
        |screen| screen.lines().any(|line| line.trim() == "› whoami"),
        TIMEOUT,
    )
    .unwrap();
    let amber = pids(home.path(), "amber");
    assert_eq!(
        methods_of(home.path(), &amber[0])
            .iter()
            .filter(|m| m.starts_with("session/prompt"))
            .count(),
        1,
        "the held prompt never reaches the previous session"
    );
    tui.verifier_pause(PAUSE);
    tui.send_key(tui_pty_e2e::Key::Enter).unwrap();
    tui.wait_for_text("amber is answering (2).", TIMEOUT)
        .unwrap();
}

fn methods_of(home: &std::path::Path, pid: &str) -> Vec<String> {
    std::fs::read_to_string(home.join("fixture-traffic-amber"))
        .unwrap()
        .lines()
        .filter_map(|line| line.strip_prefix(&format!("{pid} ")).map(str::to_string))
        .collect()
}
