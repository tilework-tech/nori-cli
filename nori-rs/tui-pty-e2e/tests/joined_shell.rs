//! `!` commands from an attached terminal run on the host, in the folder of
//! the session on screen, and their output shows there; a watcher's command
//! is refused and stays in its prompt.
#![cfg(target_os = "linux")]

#[path = "support/joined_host.rs"]
mod joined_host;

use joined_host::JoinedHost;
use tui_pty_e2e::TIMEOUT;

#[test]
fn an_attached_terminal_runs_bang_commands_in_the_host_sessions_folder() {
    let logs = tempfile::tempdir().unwrap();
    let log = logs.path().join("prompts.jsonl");
    let host = JoinedHost::new(&[("MOCK_AGENT_PROMPT_LOG", log.to_str().unwrap())]);
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

    let project = host.project().canonicalize().unwrap();
    let folder = project.display().to_string();
    let terminal = &mut controller.terminal;
    terminal.send_str("!pwd").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    terminal.send_str("\r").unwrap();
    terminal
        .wait_for(
            |screen| {
                screen.contains("Ran pwd")
                    && screen
                        .lines()
                        .any(|line| line.contains(&folder) && !line.contains('›'))
            },
            TIMEOUT,
        )
        .unwrap_or_else(|_| {
            panic!(
                "pwd prints the host session's folder {folder}:\n{}",
                terminal.screen_contents()
            )
        });

    watcher
        .terminal
        .wait_for_text("Ran pwd", TIMEOUT)
        .expect("a watcher sees the command the controller ran");
    watcher.terminal.send_str("!pwd").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    watcher.terminal.send_str("\r").unwrap();
    watcher
        .terminal
        .wait_for_text("Watching: another terminal controls this host", TIMEOUT)
        .expect("a watcher cannot run commands");
    assert!(
        watcher.terminal.screen_contents().contains("› !pwd")
            || watcher.terminal.screen_contents().contains("› pwd"),
        "the watcher's draft is kept:\n{}",
        watcher.terminal.screen_contents()
    );
    let prompts = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !prompts.contains("pwd"),
        "a ! command never reaches the agent as a prompt: {prompts}"
    );
}
