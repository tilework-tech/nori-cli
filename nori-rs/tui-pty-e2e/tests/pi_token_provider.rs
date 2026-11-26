//! Exercise Pi through the ordinary agent picker and saved connection configuration.
#![cfg(target_os = "linux")]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

/// Puts an `npx` on PATH that records its arguments and then runs the mock
/// ACP agent, standing in for the upstream Pi adapter.
fn fake_npx(bin: &Path, marker: &Path) {
    let mock = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("mock_acp_agent");
    let script = bin.join("npx");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\nexec '{}' \"$@\"\n",
            marker.display(),
            mock.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn picking_pi_prepares_its_native_server_and_completes_a_turn() {
    let bin = tempfile::tempdir().unwrap();
    let marker = bin.path().join("npx-calls");
    fake_npx(bin.path(), &marker);
    let config = SessionConfig::new()
        .with_extra_path(bin.path().to_owned())
        .with_agent_env("MOCK_AGENT_ECHO_ENV", "EXAMPLE_NATIVE_SETTING")
        .with_agent_env("EXAMPLE_NATIVE_SETTING", "synthetic-native-pi");
    let mut session = TuiSession::spawn_with_config(40, 120, config).unwrap();
    session.wait_until_ready().unwrap();
    session.submit_input("/agent").unwrap();
    session.wait_for_text("Select agent", TIMEOUT).unwrap();
    session.send_str("\x1b[F").unwrap();
    session.wait_for_text("▏›    Pi", TIMEOUT).unwrap();
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Start a new session", TIMEOUT)
        .unwrap();
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Started new conversation with agent: Pi", TIMEOUT)
        .unwrap();
    session.submit_input("read the synthetic setting").unwrap();
    session
        .wait_for_text("synthetic-native-pi", TIMEOUT)
        .unwrap();
    assert!(std::fs::read_to_string(marker).unwrap().contains("pi-acp"));
}

#[test]
fn pi_bound_to_subconscious_without_a_key_explains_and_does_not_start() {
    let bin = tempfile::tempdir().unwrap();
    let marker: PathBuf = bin.path().join("npx-calls");
    fake_npx(bin.path(), &marker);
    let config = SessionConfig::new()
        .with_agent("pi".into())
        .with_extra_path(bin.path().to_owned())
        .with_extra_config_toml(
            "[[token_providers]]\nname = 'subconscious'\npreset = 'subconscious'\nprotocol = 'openai-completions'\nenv_key = 'NORI_E2E_SUBC_KEY_UNSET'\nbase_url = 'https://api.subconscious.dev/v1'\nmodel = 'subconscious/glm-5.2'\n\n[agent_token_providers]\npi = 'subconscious'\n",
        );
    let mut session = TuiSession::spawn_with_config(40, 120, config).unwrap();
    session
        .wait_for_text("NORI_E2E_SUBC_KEY_UNSET", TIMEOUT)
        .unwrap_or_else(|_| panic!("{}", session.screen_contents()));
    assert!(!marker.exists(), "Pi must not start without its key");
}
