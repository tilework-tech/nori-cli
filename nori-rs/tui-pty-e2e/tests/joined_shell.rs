//! `!` commands from a terminal attached on the host's machine run on the
//! host, in the folder of the session on screen, and their output shows
//! there. A watcher's command is refused and stays in its prompt. A
//! controller attached from another machine gets a gray hint, keeps its
//! command in the prompt, and nothing runs.
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

    let mut controller = host.attach_new_local();
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
    terminal
        .wait_for_text("› !pwd", TIMEOUT)
        .expect("the composer holds the command");
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
    watcher
        .terminal
        .wait_for_text("› !pwd", TIMEOUT)
        .expect("the watcher's composer holds the command");
    watcher.terminal.send_str("\r").unwrap();
    watcher
        .terminal
        .wait_for_text("Watching: another terminal controls this host", TIMEOUT)
        .expect("a watcher cannot run commands");
    assert!(
        watcher.terminal.screen_contents().contains("› !pwd"),
        "the watcher's draft is kept:\n{}",
        watcher.terminal.screen_contents()
    );
    let prompts = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !prompts.contains("pwd"),
        "a ! command never reaches the agent as a prompt: {prompts}"
    );
}

#[test]
fn a_remote_controller_gets_a_hint_and_its_bang_command_runs_nowhere() {
    let logs = tempfile::tempdir().unwrap();
    let log = logs.path().join("prompts.jsonl");
    let host = JoinedHost::new(&[("MOCK_AGENT_PROMPT_LOG", log.to_str().unwrap())]);
    host.start();

    let mut remote = host.attach_new();
    let marker = logs.path().join("ran-remotely");
    let command = format!("!touch {}", marker.display());
    let terminal = &mut remote.terminal;
    terminal.send_str(&command).unwrap();
    terminal
        .wait_for_text(&format!("› {command}"), TIMEOUT)
        .expect("the composer holds the command");
    terminal.send_str("\r").unwrap();
    terminal
        .wait_for_text("Remote shell commands are not yet supported.", TIMEOUT)
        .expect("a remote controller sees the hint");
    let restored = format!("! touch {}", marker.display());
    terminal
        .wait_for_text(&restored, TIMEOUT)
        .expect("the command returns to the prompt in shell mode");
    let screen = terminal.screen_contents();
    assert!(
        !screen.contains("Ran touch"),
        "no command cell for a command that did not run:\n{screen}"
    );
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(!marker.exists(), "the command ran nowhere");
    let prompts = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !prompts.contains("touch"),
        "the command never reaches the agent as a prompt: {prompts}"
    );
}
