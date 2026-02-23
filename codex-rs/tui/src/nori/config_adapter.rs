//! Nori configuration adapter
//!
//! This module provides integration between the Nori config system
//! (from codex-acp) and the TUI. When the `nori-config` feature is enabled,
//! the TUI loads configuration from `~/.nori/cli/config.toml` instead of
//! `~/.codex/config.toml`.

#![allow(dead_code)]

use codex_acp::config::NORI_HOME_ENV;
use codex_acp::config::NoriConfig;
use codex_acp::config::NoriConfigOverrides;
use codex_acp::config::find_nori_home;
use std::path::PathBuf;

/// Get the Nori home directory path (canonicalized).
///
/// This is used to redirect config loading to `~/.nori/cli` instead of `~/.codex`.
/// The path is canonicalized to match the behavior of `find_codex_home()` and ensure
/// consistency between where trust is saved and where config is loaded.
pub fn get_nori_home() -> anyhow::Result<PathBuf> {
    let nori_home = find_nori_home()?;
    // Canonicalize if the directory exists; otherwise return the original path
    Ok(std::fs::canonicalize(&nori_home).unwrap_or(nori_home))
}

/// Set up the environment to use Nori config location.
///
/// This sets the CODEX_HOME environment variable to point to the Nori home
/// directory, which causes the existing config loading code to use the
/// Nori location instead of the default Codex location.
///
/// # Safety
/// This modifies environment variables, which is inherently thread-unsafe.
/// Call this early in program startup before spawning threads.
pub fn setup_nori_config_environment() -> anyhow::Result<()> {
    let nori_home = find_nori_home()?;

    // Create the directory and all parent directories if they don't exist
    // create_dir_all is idempotent - safe to call even if directory exists
    std::fs::create_dir_all(&nori_home).map_err(|e| {
        anyhow::anyhow!(
            "Failed to create Nori config directory '{}': {}",
            nori_home.display(),
            e
        )
    })?;

    // Canonicalize the path after creating the directory to ensure consistency
    // between where trust is saved (using nori_home) and where config is loaded
    // (using canonicalized CODEX_HOME via find_codex_home()).
    // This handles systems with symlinks (e.g., macOS /var -> /private/var).
    let canonical_home = std::fs::canonicalize(&nori_home).unwrap_or(nori_home);

    // Set CODEX_HOME to redirect config loading to Nori location
    // SAFETY: Called early in main before spawning threads
    unsafe {
        std::env::set_var("CODEX_HOME", &canonical_home);
    }

    tracing::debug!(
        "Nori config: using {} (via CODEX_HOME)",
        canonical_home.display()
    );

    Ok(())
}

/// Check if the NORI_HOME environment variable is set.
pub fn is_nori_home_env_set() -> bool {
    std::env::var(NORI_HOME_ENV).is_ok()
}

/// Load Nori configuration directly.
///
/// This bypasses the codex-core config system and loads directly from
/// `~/.nori/cli/config.toml`.
pub fn load_nori_config() -> anyhow::Result<NoriConfig> {
    NoriConfig::load()
}

/// Load Nori configuration with CLI overrides.
pub fn load_nori_config_with_overrides(
    overrides: NoriConfigOverrides,
) -> anyhow::Result<NoriConfig> {
    NoriConfig::load_with_overrides(overrides)
}

/// Returns the user's persisted agent preference from `~/.nori/cli/config.toml`.
///
/// This is the `agent` field in config, defaulting to DEFAULT_AGENT ("claude-code").
/// Returns `None` if config loading fails.
pub fn get_persisted_agent() -> Option<String> {
    match NoriConfig::load() {
        Ok(config) => Some(config.agent),
        Err(e) => {
            tracing::warn!("Failed to load NoriConfig for agent preference: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn test_get_nori_home() {
        let result = get_nori_home();
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.ends_with(".nori/cli"));
    }

    #[test]
    #[serial]
    fn test_setup_nori_config_environment() {
        // Clear any existing CODEX_HOME
        unsafe { std::env::remove_var("CODEX_HOME") };

        let result = setup_nori_config_environment();
        assert!(result.is_ok());

        // Verify CODEX_HOME was set
        let codex_home = std::env::var("CODEX_HOME");
        assert!(codex_home.is_ok());
        let path = PathBuf::from(codex_home.unwrap());
        assert!(path.ends_with(".nori/cli"));

        // Clean up
        unsafe { std::env::remove_var("CODEX_HOME") };
    }
}
