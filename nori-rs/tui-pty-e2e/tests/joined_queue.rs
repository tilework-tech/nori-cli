//! A paused prompt queue survives detaching: a terminal that attaches again,
//! with homes of its own, sees the held queue, takes one prompt back into
//! its composer exactly once, and resumes the rest, each reaching the agent
//! once.
#![cfg(target_os = "linux")]

#[path = "support/joined_host.rs"]
mod joined_host;

use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use joined_host::JoinedHost;
use pretty_assertions::assert_eq;
use tui_pty_e2e::Key;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;

fn delivered(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str::<String>(line).expect("prompt log line"))
        .collect()
}

fn wait_delivered(log: &Path, count: usize) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let prompts = delivered(log);
        if prompts.len() >= count {
            return prompts;
        }
        assert!(
            Instant::now() < deadline,
            "the agent saw {count} prompts: {prompts:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn a_reattached_terminal_takes_one_prompt_back_and_resumes_the_other_once() {
    let logs = tempfile::tempdir().unwrap();
    let log = logs.path().join("prompts.jsonl");
    let host = JoinedHost::new(&[
        ("MOCK_AGENT_PROMPT_LOG", log.to_str().unwrap()),
        ("MOCK_AGENT_DELAY_MS", "300"),
    ]);
    host.start();

    let mut first = host.attach_new();
    let terminal = &mut first.terminal;
    terminal.submit_input("mock:stream-until-cancel").unwrap();
    terminal.wait_for_text("esc to interrupt", TIMEOUT).unwrap();
    for text in ["plant the cedar row", "water the orchard"] {
        terminal.type_input(text).unwrap();
        terminal.send_key(Key::Tab).unwrap();
        std::thread::sleep(TIMEOUT_INPUT);
    }
    terminal
        .wait_for_text("water the orchard", TIMEOUT)
        .unwrap();
    terminal
        .press(
            Key::Escape,
            |screen| screen.contains("2 queued · paused (interrupted)"),
            "the interrupted turn holds the queue",
        )
        .unwrap();
    terminal.send_key(Key::Ctrl('d')).unwrap();
    assert!(terminal.wait_for_process_exit(TIMEOUT), "Ctrl+D detaches");
    drop(first);

    let mut second = host.attach_first();
    let terminal = &mut second.terminal;
    terminal
        .wait_for_text("2 queued · paused (interrupted)", TIMEOUT)
        .expect("the reattached terminal sees the held queue");
    assert_eq!(delivered(&log), ["mock:stream-until-cancel"]);

    for row in ["  ↳ plant the cedar row", "  ↳ water the orchard"] {
        assert!(
            terminal.screen_contents().contains(row),
            "the held prompts are listed in order:\n{}",
            terminal.screen_contents()
        );
    }

    // Up focuses the newest held prompt and Up again the older one; Enter
    // takes that one back into the composer.
    terminal.send_key(Key::Up).unwrap();
    terminal.wait_for_text("j/k select", TIMEOUT).unwrap();
    terminal.send_key(Key::Up).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    terminal.send_key(Key::Enter).unwrap();
    terminal
        .wait_for_text("1 queued · paused (interrupted)", TIMEOUT)
        .expect("the taken prompt leaves the queue");
    let screen = terminal.screen_contents();
    assert!(
        screen.contains("› plant the cedar row"),
        "the taken prompt is back in the composer:\n{screen}"
    );
    assert!(
        !screen.contains("  ↳ plant the cedar row"),
        "the taken prompt is no longer queued:\n{screen}"
    );

    // Clear the composer, then Enter on it empty resumes the rest.
    terminal.send_key(Key::Ctrl('u')).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    terminal.send_key(Key::Enter).unwrap();
    let prompts = wait_delivered(&log, 2);
    // Once the session is ready with nothing queued, no further prompt can
    // still be on its way to the agent.
    terminal
        .wait_for(
            |screen| screen.contains("· ready ·") && !screen.contains("queued"),
            TIMEOUT,
        )
        .expect("the session settles with an empty queue");
    assert_eq!(
        prompts,
        ["mock:stream-until-cancel", "water the orchard"],
        "{}",
        terminal.screen_contents()
    );
    assert_eq!(
        delivered(&log),
        prompts,
        "the taken prompt is never sent and the other is sent once"
    );
}

#[test]
fn a_terminal_that_takes_control_edits_another_terminals_queued_prompt() {
    let logs = tempfile::tempdir().unwrap();
    let log = logs.path().join("prompts.jsonl");
    let host = JoinedHost::new(&[
        ("MOCK_AGENT_PROMPT_LOG", log.to_str().unwrap()),
        ("MOCK_AGENT_DELAY_MS", "300"),
    ]);
    host.start();

    let mut first = host.attach_new();
    let terminal = &mut first.terminal;
    terminal.submit_input("mock:stream-until-cancel").unwrap();
    terminal.wait_for_text("esc to interrupt", TIMEOUT).unwrap();
    terminal.type_input("plant the cedar row").unwrap();
    terminal.send_key(Key::Tab).unwrap();
    terminal
        .wait_for_text("  ↳ plant the cedar row", TIMEOUT)
        .unwrap();
    terminal.send_key(Key::Escape).unwrap();
    terminal
        .wait_for_text("1 queued · paused (interrupted)", TIMEOUT)
        .expect("the interrupted turn holds the queue");

    let mut second = host.attach_first();
    let other = &mut second.terminal;
    other
        .wait_for_text("1 queued · paused (interrupted)", TIMEOUT)
        .expect("the other terminal sees the held queue");
    other.send_key(Key::Alt('t')).unwrap();
    other
        .wait_for_text("controlling", TIMEOUT)
        .expect("the other terminal takes control");
    other.send_key(Key::Up).unwrap();
    other.wait_for_text("j/k select", TIMEOUT).unwrap();
    other.send_key(Key::Enter).unwrap();
    other
        .wait_for(
            |screen| screen.contains("› plant the cedar row") && !screen.contains("queued"),
            TIMEOUT,
        )
        .expect("the controlling terminal holds the prompt to edit");
    first
        .terminal
        .wait_for(|screen| !screen.contains("queued"), TIMEOUT)
        .expect("the queue is empty in the terminal that typed it too");

    let other = &mut second.terminal;
    other.type_input(" by the fence").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    other.send_key(Key::Enter).unwrap();
    let prompts = wait_delivered(&log, 2);
    other
        .wait_for(
            |screen| screen.contains("· ready ·") && !screen.contains("queued"),
            TIMEOUT,
        )
        .expect("the session settles");
    assert_eq!(
        prompts,
        [
            "mock:stream-until-cancel",
            "plant the cedar row by the fence"
        ],
        "only the corrected prompt reaches the agent"
    );
    assert_eq!(delivered(&log), prompts);
}
