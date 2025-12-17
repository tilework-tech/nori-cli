//! First-launch detection for Nori CLI.
//!
//! Detects whether this is the user's first time running Nori by checking
//! for the existence of `~/.nori/cli/config.toml`. This file is created
//! after the first-launch onboarding flow completes.

use std::io;
use std::path::Path;
use std::path::PathBuf;

/// Returns the path to the Nori configuration directory.
///
/// Uses `NORI_HOME` environment variable if set, otherwise defaults to `~/.nori`.
#[allow(dead_code)]
pub(crate) fn find_nori_home() -> io::Result<PathBuf> {
    if let Ok(val) = std::env::var("NORI_HOME")
        && !val.is_empty()
    {
        return Ok(PathBuf::from(val));
    }

    let home = dirs::home_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Could not find home directory"))?;

    Ok(home.join(".nori"))
}

/// Check if this is the user's first launch of Nori.
///
/// Returns `true` if `~/.nori/cli/config.toml` does not exist.
pub(crate) fn is_first_launch(nori_home: &Path) -> bool {
    !nori_home.join("cli").join("config.toml").exists()
}

/// Mark the first-launch onboarding as complete.
///
/// Creates `~/.nori/cli/config.toml` with a minimal configuration.
pub(crate) fn mark_first_launch_complete(nori_home: &Path) -> io::Result<()> {
    let cli_dir = nori_home.join("cli");
    std::fs::create_dir_all(&cli_dir)?;

    let config_path = cli_dir.join("config.toml");
    let config_content = r#"# Nori CLI configuration
# Created on first launch

[cli]
first_launch_complete = true
"#;

    std::fs::write(config_path, config_content)
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
        let temp = TempDir::new().expect("create temp dir");
        let cli_dir = temp.path().join("cli");
        std::fs::create_dir_all(&cli_dir).expect("create cli dir");
        std::fs::write(cli_dir.join("config.toml"), "# exists").expect("write config");

        assert!(!is_first_launch(temp.path()));
    }

    #[test]
    fn mark_first_launch_complete_creates_config_file() {
        let temp = TempDir::new().expect("create temp dir");

        mark_first_launch_complete(temp.path()).expect("mark complete");

        let config_path = temp.path().join("cli").join("config.toml");
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
    fn find_nori_home_uses_env_var_when_set() {
        let temp = TempDir::new().expect("create temp dir");
        let temp_path = temp.path().to_string_lossy().to_string();

        // Temporarily set NORI_HOME
        // SAFETY: This test runs in a single-threaded context and the env var
        // is restored immediately after use.
        unsafe {
            std::env::set_var("NORI_HOME", &temp_path);
        }
        let result = find_nori_home().expect("find home");
        unsafe {
            std::env::remove_var("NORI_HOME");
        }

        assert_eq!(result, temp.path());
    }
}
