//! A session's goal lives on the host: an attached terminal sets it for the
//! session on screen, another session keeps none, switching back shows it
//! again, and a watcher sees it but cannot change it.
#![cfg(target_os = "linux")]

#[path = "support/joined_host.rs"]
mod joined_host;

use std::time::Duration;

use joined_host::JoinedHost;
use tui_pty_e2e::Key;
use tui_pty_e2e::TIMEOUT;

const OBJECTIVE: &str = "Objective: grow the synthetic cedar orchard";

fn objectives(screen: &str) -> usize {
    screen.matches(OBJECTIVE).count()
}

#[test]
fn each_session_shows_its_own_goal_and_a_watcher_only_reads_it() {
    let host = JoinedHost::new(&[("MOCK_AGENT_GOAL_EXT", "1")]);
    host.start();

    let mut controller = host.attach_new();
    let mut watcher = host.attach_first();
    watcher
        .terminal
        .wait_for(
            |screen| screen.contains("watching") || screen.contains("watcher"),
            TIMEOUT,
        )
        .expect("the second terminal watches");

    controller
        .terminal
        .submit_input("/goal grow the synthetic cedar orchard")
        .unwrap();
    controller
        .terminal
        .wait_for_text(OBJECTIVE, TIMEOUT)
        .expect("the controller sets the goal of the session on screen");

    watcher.terminal.submit_input("/goal").unwrap();
    watcher
        .terminal
        .wait_for_text(OBJECTIVE, TIMEOUT)
        .expect("a watcher reads the goal from the host");
    watcher
        .terminal
        .submit_input("/goal tend the plum row")
        .unwrap();
    watcher
        .terminal
        .wait_for_text("Watching: another terminal controls this host", TIMEOUT)
        .expect("a watcher cannot change the goal");
    assert!(
        watcher
            .terminal
            .screen_contents()
            .contains("goal tend the plum row"),
        "the watcher's draft is kept:\n{}",
        watcher.terminal.screen_contents()
    );

    let terminal = &mut controller.terminal;
    terminal
        .submit_input(&format!("/new {}", host.project().display()))
        .unwrap();
    terminal
        .wait_for(|screen| objectives(screen) == 0, TIMEOUT)
        .expect("a new session starts on its own screen");
    terminal.submit_input("/goal").unwrap();
    terminal
        .wait_for_text("No goal is currently set.", TIMEOUT)
        .expect("the other session has no goal");
    assert_eq!(objectives(&terminal.screen_contents()), 0);

    terminal.send_key(Key::Alt(',')).unwrap();
    terminal
        .wait_for(
            |screen| !screen.contains("No goal is currently set."),
            TIMEOUT,
        )
        .expect("switching back shows the first session");
    terminal.settle(Duration::from_millis(300));
    let before = objectives(&terminal.screen_contents());
    terminal.submit_input("/goal").unwrap();
    terminal
        .wait_for(|screen| objectives(screen) > before, TIMEOUT)
        .unwrap_or_else(|_| {
            panic!(
                "the first session still has its goal:\n{}",
                terminal.screen_contents()
            )
        });
}
