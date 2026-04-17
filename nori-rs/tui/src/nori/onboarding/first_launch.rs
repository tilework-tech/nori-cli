//! First-launch detection for Nori CLI.
//!
//! Detects whether this is the user's first time running Nori by checking
//! for the existence of `~/.nori/cli/config.toml`. This file is created
//! after the first-launch onboarding flow completes.
//!
//! Note: The nori_home path (`~/.nori/cli`) is provided by `nori_acp::config::find_nori_home()`.

use std::io;
use std::path::Path;

use codex_core::config::edit::ConfigEditsBuilder;
use codex_core::config::edit::toml_value;

/// Check if this is the user's first launch of Nori.
///
/// Returns `true` if `config.toml` does not exist in the nori_home directory.
/// Note: nori_home is expected to be `~/.nori/cli` (the full path).
pub(crate) fn is_first_launch(nori_home: &Path) -> bool {
    !nori_home.join("config.toml").exists()
}

/// Mark the first-launch onboarding as complete.
///
/// Sets `cli.first_launch_complete = true` in the config.toml file.
/// Uses ConfigEditsBuilder to merge with existing config instead of overwriting.
/// Note: nori_home is expected to be `~/.nori/cli` (the full path).
pub(crate) fn mark_first_launch_complete(nori_home: &Path) -> io::Result<()> {
    ConfigEditsBuilder::new(nori_home)
        .set_path(&["cli", "first_launch_complete"], toml_value(true))
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
}
