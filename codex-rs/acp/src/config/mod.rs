//! Nori CLI configuration module
//!
//! Provides a minimal, standalone configuration system for ACP-only mode.
//! Configuration is loaded from `~/.nori/cli/config.toml`.

mod loader;
mod types;

pub use loader::CONFIG_FILE;
pub use loader::NORI_HOME_DIR;
pub use loader::NORI_HOME_ENV;
pub use loader::find_nori_home;
pub use types::ApprovalPolicy;
pub use types::McpServerConfig;
pub use types::McpServerTransportConfig;
pub use types::NoriConfig;
pub use types::NoriConfigOverrides;
pub use types::NoriConfigToml;
pub use types::TuiConfig;

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    #[test]
    #[serial]
    fn test_find_nori_home_uses_env_var() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path().to_path_buf();

        // Set the environment variable
        // SAFETY: Test runs serially to avoid concurrent env modifications
        unsafe { env::set_var(NORI_HOME_ENV, &temp_path) };

        let result = find_nori_home();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), temp_path);

        // Clean up
        // SAFETY: Test runs serially
        unsafe { env::remove_var(NORI_HOME_ENV) };
    }

    #[test]
    #[serial]
    fn test_find_nori_home_uses_home_dir() {
        // Ensure env var is not set
        // SAFETY: Test runs serially
        unsafe { env::remove_var(NORI_HOME_ENV) };

        let result = find_nori_home();
        assert!(result.is_ok());

        let path = result.unwrap();
        assert!(path.ends_with(".nori/cli"));
    }

    #[test]
    fn test_nori_config_default() {
        let config = NoriConfig::default();

        assert_eq!(config.model, "claude-acp");
        assert!(config.animations);
        assert!(config.notifications);
        assert_eq!(
            config.sandbox_mode,
            codex_protocol::config_types::SandboxMode::WorkspaceWrite
        );
        assert_eq!(config.approval_policy, ApprovalPolicy::OnRequest);
    }

    #[test]
    fn test_nori_config_toml_deserialize_empty() {
        let toml_str = "";
        let config: NoriConfigToml = toml::from_str(toml_str).unwrap();

        assert!(config.model.is_none());
        assert!(config.sandbox_mode.is_none());
        assert!(config.approval_policy.is_none());
    }

    #[test]
    fn test_nori_config_toml_deserialize_full() {
        let toml_str = r#"
model = "gemini-acp"
sandbox_mode = "read-only"
approval_policy = "always"

[tui]
animations = false
notifications = false
"#;
        let config: NoriConfigToml = toml::from_str(toml_str).unwrap();

        assert_eq!(config.model, Some("gemini-acp".to_string()));
        assert_eq!(
            config.sandbox_mode,
            Some(codex_protocol::config_types::SandboxMode::ReadOnly)
        );
        assert_eq!(config.approval_policy, Some(ApprovalPolicy::Always));
        assert_eq!(config.tui.animations, Some(false));
        assert_eq!(config.tui.notifications, Some(false));
    }

    #[test]
    fn test_nori_config_load_from_path() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
model = "gemini-acp"

[tui]
animations = false
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();

        assert_eq!(config.model, "gemini-acp");
        assert!(!config.animations);
        assert!(config.notifications); // default
    }

    #[test]
    #[serial]
    fn test_nori_config_overrides_take_precedence() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
model = "gemini-acp"
"#,
        )
        .unwrap();

        // SAFETY: Test runs serially
        unsafe { env::set_var(NORI_HOME_ENV, temp_dir.path()) };

        let overrides = NoriConfigOverrides {
            model: Some("claude-acp".to_string()),
            ..Default::default()
        };

        let config = NoriConfig::load_with_overrides(overrides).unwrap();

        // Override should win
        assert_eq!(config.model, "claude-acp");

        // SAFETY: Test runs serially
        unsafe { env::remove_var(NORI_HOME_ENV) };
    }

    #[test]
    #[serial]
    fn test_nori_config_missing_file_uses_defaults() {
        let temp_dir = TempDir::new().unwrap();
        // Don't create a config file

        // SAFETY: Test runs serially
        unsafe { env::set_var(NORI_HOME_ENV, temp_dir.path()) };

        let config = NoriConfig::load().unwrap();

        assert_eq!(config.model, "claude-acp");
        assert!(config.animations);
        assert!(config.notifications);

        // SAFETY: Test runs serially
        unsafe { env::remove_var(NORI_HOME_ENV) };
    }

    #[test]
    fn test_mcp_server_config_deserialize_stdio() {
        let toml_str = r#"
[mcp_servers.my-tool]
command = "my-tool"
args = ["--arg1", "value"]
"#;
        let config: NoriConfigToml = toml::from_str(toml_str).unwrap();

        assert!(config.mcp_servers.contains_key("my-tool"));
        let server = &config.mcp_servers["my-tool"];
        assert_eq!(server.command, Some("my-tool".to_string()));
        assert_eq!(
            server.args,
            Some(vec!["--arg1".to_string(), "value".to_string()])
        );
    }
}
