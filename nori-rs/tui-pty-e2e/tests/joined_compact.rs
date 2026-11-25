//! `/compact` from an attached terminal compacts the session on screen on
//! its host, and the finish shows in that session's transcript only.
#![cfg(target_os = "linux")]

#[path = "support/joined_host.rs"]
mod joined_host;

use std::time::Duration;

use joined_host::JoinedHost;
use tui_pty_e2e::Key;
use tui_pty_e2e::TIMEOUT;

const COMPACTED: &str = "Context compacted";

#[test]
fn an_attached_terminal_compacts_its_session_and_sees_the_finish_there_only() {
    let host = JoinedHost::new(&[("MOCK_AGENT_ADVERTISE_COMPACT", "1")]);
    host.start();

    let mut attached = host.attach_new();
    let terminal = &mut attached.terminal;
    terminal
        .submit_input("seed the demo orchard notes")
        .unwrap();
    terminal
        .wait_for_text(joined_host::ANSWER, TIMEOUT)
        .unwrap();
    terminal.submit_input("/compact").unwrap();
    terminal
        .wait_for_text(COMPACTED, TIMEOUT)
        .expect("the finish shows in the compacted session");

    terminal
        .submit_input(&format!("/new {}", host.project().display()))
        .unwrap();
    terminal
        .wait_for(|screen| !screen.contains(COMPACTED), TIMEOUT)
        .expect("another session has no compaction");
    terminal.settle(Duration::from_millis(500));
    assert!(!terminal.screen_contents().contains(COMPACTED));

    terminal.send_key(Key::Alt(',')).unwrap();
    terminal
        .wait_for_text(COMPACTED, TIMEOUT)
        .expect("switching back still shows the finish in the first session");
}
