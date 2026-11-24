//! The retired approval presets move to the host's modes once, on the first
//! launch that finds them, and later launches leave the config alone.
#![cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

fn mock_agent() -> PathBuf {
    std::env::var_os("MOCK_ACP_AGENT_BIN").map_or_else(
        || {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("mock_acp_agent")
        },
        PathBuf::from,
    )
}

/// A home whose config has `top` above a Claude Code stand-in that can ask.
fn home_with(top: &str) -> (tempfile::TempDir, PathBuf) {
    let home = tempfile::tempdir().unwrap();
    let folder = home.path().canonicalize().unwrap();
    std::fs::write(
        folder.join("config.toml"),
        format!(
            "agent = \"claude-code\"\n{top}\n[projects.{folder:?}]\ntrust_level = \"trusted\"\n\n\
             [[agents]]\nname = \"Claude Code\"\nslug = \"claude-code\"\n\
             [agents.distribution.local]\ncommand = {:?}\n\
             [agents.distribution.local.env]\n\
             MOCK_AGENT_MODES = \"default,auto,acceptEdits,plan\"\n\
             MOCK_AGENT_ASKING_MODES = \"default\"\n",
            mock_agent().display().to_string()
        ),
    )
    .unwrap();
    (home, folder)
}

fn launch(folder: &Path) -> TuiSession {
    let config = SessionConfig::new()
        .with_agent("claude-code".into())
        .with_cwd(folder)
        .with_nori_home(folder);
    let mut tui = TuiSession::spawn_with_config(30, 110, config).unwrap();
    tui.wait_for_text("›", TIMEOUT).unwrap();
    tui
}

fn footer_mode(tui: &mut TuiSession, name: &str) {
    let label = format!("Approvals: {name}");
    tui.wait_for(|screen| screen.contains(&label), TIMEOUT)
        .unwrap_or_else(|_| panic!("never showed {label}:\n{}", tui.screen_contents()));
}

fn config(folder: &Path) -> String {
    std::fs::read_to_string(folder.join("config.toml")).unwrap()
}

#[test]
fn a_read_only_preset_starts_sessions_in_ask_on_this_and_every_later_launch() {
    let (_home, folder) =
        home_with("approval_policy = \"on-request\"\nsandbox_mode = \"read-only\"");

    let mut first = launch(&folder);
    footer_mode(&mut first, "Ask");
    let first_output = first.history_contents();
    drop(first);
    let migrated = config(&folder);
    let mut second = launch(&folder);
    footer_mode(&mut second, "Ask");
    let second_output = second.history_contents();
    drop(second);

    assert!(
        first_output.contains("Approval preset Read Only"),
        "{first_output}"
    );
    assert!(
        !second_output.contains("Approval preset"),
        "{second_output}"
    );
    assert!(migrated.contains("default_mode = \"ask\""), "{migrated}");
    assert!(
        migrated.contains("sandbox_mode = \"read-only\""),
        "{migrated}"
    );
    assert_eq!(config(&folder), migrated);
}

#[test]
fn a_config_without_presets_is_untouched_across_launches() {
    let (_home, folder) = home_with("");
    let original = config(&folder);

    for _ in 0..2 {
        let mut tui = launch(&folder);
        tui.wait_for_text("Claude Code options", TIMEOUT).unwrap();
        assert!(
            !tui.screen_contents().contains("Approvals:"),
            "an Agent session keeps the footer quiet:\n{}",
            tui.screen_contents()
        );
    }

    assert_eq!(config(&folder), original);
}

/// How many sessions the owned host has started for this terminal.
fn sessions_started(tui: &TuiSession) -> usize {
    tui.acp_log_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains("host ACP request") && line.contains("method=session/new"))
        .count()
}

#[test]
fn the_host_default_applies_to_the_first_session_and_to_new_ones() {
    let (_home, folder) = home_with("[host]\ndefault_mode = \"ask\"");

    let mut tui = launch(&folder);
    footer_mode(&mut tui, "Ask");
    let first = sessions_started(&tui);
    tui.send_str("/new").unwrap();
    tui.settle(std::time::Duration::from_millis(900));
    tui.send_key(tui_pty_e2e::Key::Enter).unwrap();
    let deadline = std::time::Instant::now() + TIMEOUT;
    while sessions_started(&tui) <= first {
        assert!(
            std::time::Instant::now() < deadline,
            "/new never started a session:\n{}",
            tui.screen_contents()
        );
        tui.settle(std::time::Duration::from_millis(50));
    }

    footer_mode(&mut tui, "Ask");
}
