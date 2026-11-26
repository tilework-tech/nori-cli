//! Prompt history lives on the host: a prompt sent from one attached
//! terminal is recalled with Up and found with Ctrl+R from another terminal
//! whose homes hold no history of their own.
#![cfg(target_os = "linux")]

#[path = "support/joined_host.rs"]
mod joined_host;

use joined_host::ANSWER;
use joined_host::JoinedHost;
use tui_pty_e2e::Key;
use tui_pty_e2e::TIMEOUT;

const PROMPT: &str = "count the walnut trees by the north gate";

#[test]
fn another_terminal_recalls_and_searches_a_prompt_from_the_hosts_history() {
    let host = JoinedHost::new(&[]);
    host.start();

    let mut first = host.attach_new();
    first.terminal.submit_input(PROMPT).unwrap();
    first.terminal.wait_for_text(ANSWER, TIMEOUT).unwrap();
    first.terminal.send_key(Key::Ctrl('d')).unwrap();
    assert!(first.terminal.wait_for_process_exit(TIMEOUT));
    drop(first);
    let stored = std::fs::read_to_string(host.nori_home.path().join("history.jsonl"))
        .expect("the host keeps the history in its own NORI_HOME");
    assert!(stored.contains(PROMPT), "{stored}");

    let mut second = host.attach_first();
    let terminal = &mut second.terminal;
    terminal.wait_for_text(ANSWER, TIMEOUT).unwrap();
    // The replayed transcript already shows the prompt once; the composer
    // adds a second copy only when it recalls it.
    let shown = terminal.settled_screen().matches(PROMPT).count();
    terminal.send_key(Key::Up).unwrap();
    terminal
        .wait_for(
            |screen| screen.matches(PROMPT).count() == shown + 1,
            TIMEOUT,
        )
        .expect("Up recalls the prompt from the host's history");

    terminal.send_key(Key::Ctrl('c')).unwrap();
    terminal
        .wait_for(|screen| screen.matches(PROMPT).count() == shown, TIMEOUT)
        .unwrap();
    terminal.send_key(Key::Ctrl('r')).unwrap();
    terminal
        .wait_for(|screen| screen.matches(PROMPT).count() > shown, TIMEOUT)
        .expect("Ctrl+R lists the prompt from the host's history");
    terminal.send_str("/walnut").unwrap();
    terminal
        .wait_for(
            |screen| screen.contains("search: walnut") && screen.matches(PROMPT).count() > shown,
            TIMEOUT,
        )
        .expect("searching the host's history still finds the prompt");
}
