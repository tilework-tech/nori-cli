//! Choosing Pi with the Subconscious token provider through `/agent`, and
//! refusing to start Pi when the chosen provider's key is missing.
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

fn select_row(session: &mut TuiSession, downs: usize) {
    for _ in 0..downs {
        session.send_key(Key::Down).unwrap();
    }
    session.send_key(Key::Enter).unwrap();
}

#[test]
fn picking_pi_with_subconscious_launches_pi_with_its_own_provider_directory() {
    let bin = tempfile::tempdir().unwrap();
    let marker = bin.path().join("npx-calls");
    fake_npx(bin.path(), &marker);
    let config = SessionConfig::new()
        .with_extra_path(bin.path().to_owned())
        .with_agent_env("SUBCONSCIOUS_API_KEY", "subc-synthetic-key-0003")
        .with_agent_env("MOCK_AGENT_ECHO_ENV", "PI_CODING_AGENT_DIR");
    let mut session = TuiSession::spawn_with_config(40, 120, config).unwrap();
    session.wait_for_text("›", TIMEOUT).unwrap();
    let home = session.nori_home_path().unwrap();

    session.submit_input("/agent").unwrap();
    session.wait_for_text("Select agent", TIMEOUT).unwrap();
    select_row(&mut session, 6);
    session
        .wait_for_text("Model provider for Pi", TIMEOUT)
        .unwrap();
    select_row(&mut session, 1);
    session.wait_for_text("Subconscious key", TIMEOUT).unwrap();
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Subconscious model for Pi", TIMEOUT)
        .unwrap();
    select_row(&mut session, 1);
    session
        .wait_for_text("Start a new session", TIMEOUT)
        .unwrap();
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for(
            |screen| screen.contains("Started new conversation with agent: Pi"),
            TIMEOUT,
        )
        .unwrap();
    session.submit_input("show the provider directory").unwrap();
    session
        .wait_for(
            |screen| screen.contains("token-providers/subconscious-pi-acp"),
            TIMEOUT,
        )
        .unwrap_or_else(|_| panic!("{}", session.screen_contents()));

    let calls = std::fs::read_to_string(&marker).unwrap();
    assert!(calls.contains("pi-acp"), "{calls}");
    let saved = std::fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(saved.contains("SUBCONSCIOUS_API_KEY"), "{saved}");
    assert!(!saved.contains("subc-synthetic-key-0003"), "{saved}");
    let settings =
        std::fs::read_to_string(home.join("token-providers/subconscious-pi-acp/settings.json"))
            .unwrap();
    assert!(settings.contains("subconscious/glm-5.2"), "{settings}");
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
            "[token_providers.subconscious]\npreset = 'subconscious'\napi_key_env = 'NORI_E2E_SUBC_KEY_UNSET'\n\n[agent_token_providers]\npi = 'subconscious'\n",
        );
    let mut session = TuiSession::spawn_with_config(40, 120, config).unwrap();
    session
        .wait_for_text("NORI_E2E_SUBC_KEY_UNSET", TIMEOUT)
        .unwrap_or_else(|_| panic!("{}", session.screen_contents()));
    assert!(!marker.exists(), "Pi must not start without its key");
}
