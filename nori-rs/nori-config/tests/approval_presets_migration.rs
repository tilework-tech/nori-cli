//! The one-time move from the retired approval presets (Read Only, Agent,
//! Full Access) to the host's per-session modes, as it reads on disk.

use nori_config::AskForApproval;
use nori_config::HostDefaultMode;
use nori_config::NoriConfig;
use nori_config::SandboxPolicy;
use nori_config::migrate_approval_presets;
use pretty_assertions::assert_eq;
use std::path::Path;
use tempfile::TempDir;

struct Outcome {
    default_mode: Option<HostDefaultMode>,
    approval_policy: AskForApproval,
    sandbox_policy: SandboxPolicy,
    message: Option<String>,
}

fn home_with(config_toml: &str) -> anyhow::Result<TempDir> {
    let home = TempDir::new()?;
    std::fs::write(home.path().join("config.toml"), config_toml)?;
    Ok(home)
}

fn migrate(home: &Path) -> anyhow::Result<Outcome> {
    let message = migrate_approval_presets(home)?;
    let config = NoriConfig::load_from_path(&home.join("config.toml"))?;
    Ok(Outcome {
        default_mode: config.host.default_mode,
        approval_policy: config.approval_policy,
        sandbox_policy: config.sandbox_policy,
        message,
    })
}

fn config_text(home: &Path) -> anyhow::Result<String> {
    Ok(std::fs::read_to_string(home.join("config.toml"))?)
}

#[test]
fn read_only_preset_starts_sessions_in_ask_and_keeps_its_sandbox() -> anyhow::Result<()> {
    let home = home_with(
        "# chosen by hand\nagent = \"claude-code\"\napproval_policy = \"on-request\"\nsandbox_mode = \"read-only\"\n",
    )?;

    let outcome = migrate(home.path())?;

    assert_eq!(outcome.default_mode, Some(HostDefaultMode::Ask));
    assert_eq!(outcome.sandbox_policy, SandboxPolicy::ReadOnly);
    assert_eq!(outcome.approval_policy, AskForApproval::OnRequest);
    let message = outcome.message.expect("the move is announced");
    assert!(message.contains("Read Only"), "{message}");
    assert!(message.contains("Ask"), "{message}");
    let text = config_text(home.path())?;
    assert!(text.contains("# chosen by hand"), "{text}");
    assert!(text.contains("agent = \"claude-code\""), "{text}");
    Ok(())
}

#[test]
fn full_access_preset_approves_all_and_leaves_the_sandbox_off() -> anyhow::Result<()> {
    let home = home_with("approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\n")?;

    let outcome = migrate(home.path())?;

    assert_eq!(outcome.default_mode, Some(HostDefaultMode::ApproveAll));
    assert_eq!(outcome.sandbox_policy, SandboxPolicy::DangerFullAccess);
    assert!(outcome.message.is_some());
    Ok(())
}

#[test]
fn agent_preset_stays_agent() -> anyhow::Result<()> {
    let home = home_with("approval_policy = \"on-request\"\nsandbox_mode = \"workspace-write\"\n")?;

    let outcome = migrate(home.path())?;

    assert_eq!(outcome.default_mode, Some(HostDefaultMode::Agent));
    assert!(matches!(
        outcome.sandbox_policy,
        SandboxPolicy::WorkspaceWrite { .. }
    ));
    Ok(())
}

#[test]
fn a_migrated_config_is_left_alone_on_every_later_launch() -> anyhow::Result<()> {
    let home = home_with("approval_policy = \"on-request\"\nsandbox_mode = \"read-only\"\n")?;
    migrate(home.path())?;
    let after_first = config_text(home.path())?;

    let second = migrate(home.path())?;
    let third = migrate(home.path())?;

    assert_eq!(second.message, None);
    assert_eq!(third.message, None);
    assert_eq!(config_text(home.path())?, after_first);
    Ok(())
}

#[test]
fn a_later_change_to_the_default_mode_is_not_undone() -> anyhow::Result<()> {
    let home = home_with("approval_policy = \"on-request\"\nsandbox_mode = \"read-only\"\n")?;
    migrate(home.path())?;
    let edited = config_text(home.path())?
        .replace("default_mode = \"ask\"", "default_mode = \"approve-all\"");
    std::fs::write(home.path().join("config.toml"), &edited)?;

    let outcome = migrate(home.path())?;

    assert_eq!(outcome.default_mode, Some(HostDefaultMode::ApproveAll));
    assert_eq!(config_text(home.path())?, edited);
    Ok(())
}

#[test]
fn a_pair_that_matches_no_preset_is_kept_and_reported_once() -> anyhow::Result<()> {
    let home = home_with("approval_policy = \"never\"\nsandbox_mode = \"workspace-write\"\n")?;

    let first = migrate(home.path())?;
    let second = migrate(home.path())?;

    assert_eq!(first.default_mode, None);
    assert_eq!(first.approval_policy, AskForApproval::Never);
    assert!(matches!(
        first.sandbox_policy,
        SandboxPolicy::WorkspaceWrite { .. }
    ));
    let message = first.message.expect("the kept pair is reported");
    assert!(message.contains("left as is"), "{message}");
    assert_eq!(second.message, None);
    Ok(())
}

#[test]
fn a_single_explicit_key_is_not_a_preset() -> anyhow::Result<()> {
    let home = home_with("sandbox_mode = \"read-only\"\n")?;

    let outcome = migrate(home.path())?;

    assert_eq!(outcome.default_mode, None);
    assert_eq!(outcome.sandbox_policy, SandboxPolicy::ReadOnly);
    Ok(())
}

#[test]
fn a_config_without_approval_settings_is_not_rewritten() -> anyhow::Result<()> {
    let original = "agent = \"codex\"\n";
    let home = home_with(original)?;

    let outcome = migrate(home.path())?;

    assert_eq!(outcome.message, None);
    assert_eq!(config_text(home.path())?, original);
    Ok(())
}

#[test]
fn a_fresh_home_gets_no_config_file() -> anyhow::Result<()> {
    let home = TempDir::new()?;

    let message = migrate_approval_presets(home.path())?;

    assert_eq!(message, None);
    assert!(!home.path().join("config.toml").exists());
    Ok(())
}

#[test]
fn an_explicit_default_mode_wins_over_a_preset() -> anyhow::Result<()> {
    let original = "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\n\n[host]\ndefault_mode = \"ask\"\n";
    let home = home_with(original)?;

    let outcome = migrate(home.path())?;

    assert_eq!(outcome.default_mode, Some(HostDefaultMode::Ask));
    assert_eq!(outcome.message, None);
    assert_eq!(config_text(home.path())?, original);
    Ok(())
}
