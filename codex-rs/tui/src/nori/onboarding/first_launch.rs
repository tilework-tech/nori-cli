//! First-launch detection for Nori CLI.
//!
//! Detects whether this is the user's first time running Nori by checking
//! for the existence of `~/.nori/cli/config.toml`. This file is created
//! after the first-launch onboarding flow completes.
//!
//! Note: The nori_home path (`~/.nori/cli`) is provided by `codex_acp::config::find_nori_home()`.

use std::io;
use std::path::Path;

use codex_core::config::edit::Array as TomlArray;
use codex_core::config::edit::ConfigEditsBuilder;
use codex_core::config::edit::Item as TomlItem;
use codex_core::config::edit::toml_value;

/// The bundled notify-hook.sh script content.
/// This script is deployed to ~/.nori/cli/notify-hook.sh on first launch.
const NOTIFY_HOOK_SCRIPT: &str = include_str!("../notify-hook.sh");

/// Check if this is the user's first launch of Nori.
///
/// Returns `true` if `config.toml` does not exist in the nori_home directory.
/// Note: nori_home is expected to be `~/.nori/cli` (the full path).
pub(crate) fn is_first_launch(nori_home: &Path) -> bool {
    !nori_home.join("config.toml").exists()
}

/// Deploy the bundled notify-hook.sh script to the nori_home directory.
///
/// This creates `~/.nori/cli/notify-hook.sh` with the bundled script content
/// and sets executable permissions on Unix systems.
///
/// Note: nori_home is expected to be `~/.nori/cli` (the full path).
pub(crate) fn deploy_notify_hook(nori_home: &Path) -> io::Result<()> {
    let hook_path = nori_home.join("notify-hook.sh");

    // Write the script content
    std::fs::write(&hook_path, NOTIFY_HOOK_SCRIPT)?;

    // Set executable permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&hook_path)?.permissions();
        perms.set_mode(perms.mode() | 0o111); // Add execute bits
        std::fs::set_permissions(&hook_path, perms)?;
    }

    Ok(())
}

/// Mark the first-launch onboarding as complete.
///
/// This function:
/// 1. Deploys the notify-hook.sh script to nori_home
/// 2. Sets `cli.first_launch_complete = true` in the config.toml file
/// 3. Configures `notify` to point to the deployed hook script
///
/// Uses ConfigEditsBuilder to merge with existing config instead of overwriting.
/// Note: nori_home is expected to be `~/.nori/cli` (the full path).
pub(crate) fn mark_first_launch_complete(nori_home: &Path) -> io::Result<()> {
    // Deploy the notify hook script
    deploy_notify_hook(nori_home)?;

    // Get the path to the notify hook for config
    let hook_path = nori_home.join("notify-hook.sh");
    let hook_path_str = hook_path.to_string_lossy().to_string();

    // Create a TOML array for the notify setting
    let mut notify_array = TomlArray::new();
    notify_array.push(hook_path_str);
    let notify_value = TomlItem::Value(notify_array.into());

    // Update config with first_launch_complete and notify settings
    ConfigEditsBuilder::new(nori_home)
        .set_path(&["cli", "first_launch_complete"], toml_value(true))
        .set_path(&["notify"], notify_value)
        .apply_blocking()
        .map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn is_first_launch_returns_true_when_config_missing() {
        let temp = TempDir::new().expect("create temp dir");
        assert!(is_first_launch(temp.path()));
    }

    #[test]
    fn is_first_launch_returns_false_when_config_exists() {
        // nori_home IS ~/.nori/cli, so config.toml lives directly in it
        let temp = TempDir::new().expect("create temp dir");
        std::fs::write(temp.path().join("config.toml"), "# exists").expect("write config");

        assert!(!is_first_launch(temp.path()));
    }

    #[test]
    fn mark_first_launch_complete_creates_config_file() {
        // nori_home IS ~/.nori/cli, so config.toml is created directly in it
        let temp = TempDir::new().expect("create temp dir");

        mark_first_launch_complete(temp.path()).expect("mark complete");

        let config_path = temp.path().join("config.toml");
        assert!(config_path.exists());

        let content = std::fs::read_to_string(config_path).expect("read config");
        assert!(content.contains("first_launch_complete = true"));
    }

    #[test]
    fn mark_first_launch_complete_is_idempotent() {
        let temp = TempDir::new().expect("create temp dir");

        mark_first_launch_complete(temp.path()).expect("first call");
        mark_first_launch_complete(temp.path()).expect("second call");

        assert!(!is_first_launch(temp.path()));
    }

    #[test]
    fn deploy_notify_hook_creates_script_file() {
        let temp = TempDir::new().expect("create temp dir");

        deploy_notify_hook(temp.path()).expect("deploy hook");

        let hook_path = temp.path().join("notify-hook.sh");
        assert!(hook_path.exists(), "notify-hook.sh should be created");

        let content = std::fs::read_to_string(&hook_path).expect("read hook");
        assert!(content.contains("#!/bin/bash"), "should have bash shebang");
        assert!(
            content.contains("notify-send") || content.contains("osascript"),
            "should contain OS notification commands"
        );
    }

    #[test]
    fn deploy_notify_hook_sets_executable_permission() {
        let temp = TempDir::new().expect("create temp dir");

        deploy_notify_hook(temp.path()).expect("deploy hook");

        let hook_path = temp.path().join("notify-hook.sh");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&hook_path).expect("get metadata");
            let mode = metadata.permissions().mode();
            assert!(mode & 0o111 != 0, "should be executable");
        }
    }

    #[test]
    fn mark_first_launch_complete_configures_notify_hook() {
        let temp = TempDir::new().expect("create temp dir");

        mark_first_launch_complete(temp.path()).expect("mark complete");

        let config_path = temp.path().join("config.toml");
        let content = std::fs::read_to_string(config_path).expect("read config");

        // The notify setting should point to the deployed hook
        assert!(
            content.contains("notify"),
            "config should contain notify setting"
        );
        assert!(
            content.contains("notify-hook.sh"),
            "notify should reference notify-hook.sh"
        );
    }
}
