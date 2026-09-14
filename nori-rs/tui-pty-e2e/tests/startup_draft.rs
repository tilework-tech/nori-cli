#![cfg(unix)]

//! Hold agent preparation behind a file gate to force a draft across activation.

use pretty_assertions::assert_eq;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

#[test]
fn startup_preserves_draft_cursor_across_activation() {
    let dir = tempfile::tempdir().unwrap();
    let gate = dir.path().join("activate");
    let mock = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("mock_acp_agent");
    assert!(mock.exists(), "missing mock agent: {}", mock.display());
    let config = format!(
        r#"agent = "gated-mock"
[[agents]]
name = "Gated Mock"
slug = "gated-mock"
[agents.distribution.local]
command = "/bin/sh"
args = ["-c", 'while [ ! -f "$1" ]; do sleep 0.01; done; exec "$2"', "gated-mock", {gate:?}, {mock:?}]
"#,
    );
    let mut session = TuiSession::spawn_with_config(
        24,
        80,
        SessionConfig::new()
            .with_agent("gated-mock".to_string())
            .with_config_toml(config),
    )
    .unwrap();
    session.wait_for_text("›", TIMEOUT).unwrap();
    session.send_key(Key::Char('d')).unwrap();
    session.wait_for_text("› d", TIMEOUT).unwrap();
    // The agent cannot prepare until the first character has rendered.
    std::fs::write(&gate, "ready").unwrap();
    session
        .wait_for_text("Mock Default Model", TIMEOUT)
        .unwrap();
    session.type_input("raft message").unwrap();
    let screen = session.screen_contents();
    let composer = screen
        .lines()
        .rev()
        .find(|line| line.contains('›'))
        .unwrap()
        .trim();
    assert_eq!(composer, "› draft message");
    insta::assert_snapshot!("startup_draft_after_activation", composer);

    session.send_key(Key::Ctrl('c')).unwrap();
    session
        .wait_for(|screen| !screen.contains("draft message"), TIMEOUT)
        .unwrap();
}
