//! Drive the built-in Cursor entry with a synthetic stdio peer.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

fn cursor_fixture(executable: &str) -> (tempfile::TempDir, SessionConfig) {
    let bin = tempfile::tempdir().unwrap();
    let mock = std::env::var_os("MOCK_ACP_AGENT_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("mock_acp_agent")
        });
    let shim = bin.path().join(executable);
    std::fs::write(
        &shim,
        r#"#!/bin/sh
[ "$#" = 1 ] && [ "$1" = acp ] && [ "$CURSOR_API_KEY" = synthetic-cursor-key ] || exit 64
export MOCK_AGENT_RESPONSE='synthetic Cursor reply'
exec "$MOCK_ACP_AGENT_BIN"
"#,
    )
    .unwrap();
    std::fs::set_permissions(shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    if executable == "cursor-agent" {
        let unrelated = bin.path().join("agent");
        std::fs::write(&unrelated, "#!/bin/sh\nexit 65\n").unwrap();
        std::fs::set_permissions(unrelated, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let config = SessionConfig::new()
        .with_extra_path(bin.path().to_owned())
        .with_excluded_binary("agent")
        .with_excluded_binary("cursor-agent")
        .with_agent_env("MOCK_ACP_AGENT_BIN", mock.to_str().unwrap())
        .with_agent_env("CURSOR_API_KEY", "synthetic-cursor-key")
        .with_extra_config_toml("[features]\nmulti_session = true");
    (bin, config)
}

#[test]
fn standalone_cursor_accepts_a_prompt_and_explains_login() {
    let (_bin, config) = cursor_fixture("agent");
    let mut session =
        TuiSession::spawn_with_config(40, 120, config.with_agent("cursor".into())).unwrap();
    session
        .wait_for_text("Mock Default Model", TIMEOUT)
        .unwrap();
    session.submit_input("hello Cursor").unwrap();
    session
        .wait_for_text("synthetic Cursor reply", TIMEOUT)
        .unwrap();
    session.submit_input("/login").unwrap();
    session.wait_for_text("CURSOR_API_KEY", TIMEOUT).unwrap();
}

#[test]
fn cursor_child_uses_sessions_executable_and_returns_to_its_parent() {
    let (_bin, config) = cursor_fixture("cursor-agent");
    let mut session = TuiSession::spawn_with_config(40, 120, config).unwrap();
    session
        .wait_for_text("Mock Default Model", TIMEOUT)
        .unwrap();
    session
        .submit_input("/delegate cursor mock:request-permission:write-example")
        .unwrap();
    session
        .wait_for_text("start a cursor session", TIMEOUT)
        .unwrap();
    session.wait_out_answer_grace();
    session.send_key(Key::Char('1')).unwrap();
    session.wait_for_text("waiting on you", TIMEOUT).unwrap();
    session.send_key(Key::AltDown).unwrap();
    session
        .wait_for_text("Would you like to allow", TIMEOUT)
        .unwrap();
    session.wait_for_text("write-example", TIMEOUT).unwrap();
    session
        .wait_for_text("alt + up to parent", TIMEOUT)
        .unwrap();
    session.wait_out_answer_grace();
    session.send_key(Key::Char('1')).unwrap();
    session
        .wait_for_text("Permission granted with option: allow", TIMEOUT)
        .unwrap();
    session
        .wait_for_text("synthetic Cursor reply", TIMEOUT)
        .unwrap();
    session.send_key(Key::AltUp).unwrap();
    session
        .wait_for_text("result from cursor", TIMEOUT)
        .unwrap();
    session
        .wait_for_text("Delegation continuation received", TIMEOUT)
        .unwrap();
}
