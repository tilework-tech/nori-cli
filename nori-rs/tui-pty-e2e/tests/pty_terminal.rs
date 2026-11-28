#![cfg(unix)]

//! The harness terminal stays attached while a test is not polling.
//!
//! A real terminal reads everything the program writes and answers its queries
//! at once, whatever the person at it is doing. Tests often wait on something
//! other than the screen (an agent's stderr, a file, a sleep). The program must
//! not stall meanwhile: once the PTY buffer fills, its writes block, and on
//! macOS that buffer holds only a few kilobytes.

use pretty_assertions::assert_eq;
use std::path::Path;
use std::time::Duration;
use std::time::Instant;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

/// Spawn `script` under `/bin/sh` in the harness terminal.
fn spawn_shell(script: String) -> TuiSession {
    let config = SessionConfig {
        binary: "/bin/sh",
        command_args: Some(vec!["-c".to_string(), script]),
        ..SessionConfig::new()
    };
    TuiSession::spawn_with_config(24, 80, config).expect("spawn shell")
}

/// Wait for `path` to exist without polling the session.
fn wait_for_file(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while !path.exists() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

#[test]
fn output_written_while_the_test_is_not_polling_does_not_block_the_program() {
    let dir = tempfile::tempdir().unwrap();
    let done = dir.path().join("done");
    // Far more than any PTY buffers, on Linux or macOS.
    let bytes = 4 * 1024 * 1024;
    let mut session = spawn_shell(format!(
        "head -c {bytes} /dev/zero | tr '\\0' x; touch '{}'; sleep 30",
        done.display()
    ));

    assert!(
        wait_for_file(&done, TIMEOUT),
        "the program blocked writing to a terminal the test was not polling"
    );
    let written = |session: &TuiSession| session.raw_output().len();
    let deadline = Instant::now() + TIMEOUT;
    while written(&session) < bytes && Instant::now() < deadline {
        session.poll().unwrap();
    }
    assert_eq!(
        written(&session),
        bytes,
        "polling afterwards shows everything it wrote"
    );
    session.terminate_and_wait(TIMEOUT);
}

#[test]
fn queries_are_answered_while_the_test_is_not_polling() {
    let dir = tempfile::tempdir().unwrap();
    let reply = dir.path().join("reply");
    // Ask for the cursor position, as crossterm does, and record the answer.
    let mut session = spawn_shell(format!(
        "stty raw -echo; printf '\\033[6n'; dd bs=1 count=6 2>/dev/null > '{tmp}'; mv '{tmp}' '{reply}'; sleep 30",
        tmp = dir.path().join("partial").display(),
        reply = reply.display(),
    ));

    assert!(
        wait_for_file(&reply, TIMEOUT),
        "the cursor position query went unanswered while the test was not polling"
    );
    assert_eq!(std::fs::read(&reply).unwrap(), b"\x1b[1;1R");
    session.terminate_and_wait(TIMEOUT);
}
