//! Configuration loading for Nori CLI

use super::types::ApprovalPolicy;
use super::types::DEFAULT_MODEL;
use super::types::McpServerConfig;
use super::types::NoriConfig;
use super::types::NoriConfigOverrides;
use super::types::NoriConfigToml;
use anyhow::Context;
use anyhow::Result;
use codex_protocol::config_types::SandboxMode;
use std::collections::HashMap;
use std::path::PathBuf;

/// Environment variable to override the Nori home directory
pub const NORI_HOME_ENV: &str = "NORI_HOME";

/// Default Nori home directory path (relative to home)
pub const NORI_HOME_DIR: &str = ".nori/cli";

/// Config file name
pub const CONFIG_FILE: &str = "config.toml";

/// Find the Nori home directory (~/.nori/cli or $NORI_HOME)
pub fn find_nori_home() -> Result<PathBuf> {
    if let Ok(env_home) = std::env::var(NORI_HOME_ENV) {
        return Ok(PathBuf::from(env_home));
    }

    let home = dirs::home_dir().context("Could not determine home directory")?;

    Ok(home.join(NORI_HOME_DIR))
}

impl NoriConfig {
    /// Load configuration from ~/.nori/cli/config.toml
    pub fn load() -> Result<Self> {
        Self::load_with_overrides(NoriConfigOverrides::default())
    }

    /// Load configuration with CLI overrides
    pub fn load_with_overrides(overrides: NoriConfigOverrides) -> Result<Self> {
        let nori_home = find_nori_home()?;
        let config_path = nori_home.join(CONFIG_FILE);

        let toml_config = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read {}", config_path.display()))?;
            toml::from_str::<NoriConfigToml>(&content)
                .with_context(|| format!("Failed to parse {}", config_path.display()))?
        } else {
            NoriConfigToml::default()
        };

        Self::from_toml(toml_config, nori_home, overrides)
    }

    /// Load configuration from a specific path (for testing)
    pub fn load_from_path(config_path: &PathBuf) -> Result<Self> {
        let nori_home = config_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));

        let toml_config = if config_path.exists() {
            let content = std::fs::read_to_string(config_path)
                .with_context(|| format!("Failed to read {}", config_path.display()))?;
            toml::from_str::<NoriConfigToml>(&content)
                .with_context(|| format!("Failed to parse {}", config_path.display()))?
        } else {
            NoriConfigToml::default()
        };

        Self::from_toml(toml_config, nori_home, NoriConfigOverrides::default())
    }

    /// Build resolved config from TOML + overrides
    fn from_toml(
        toml: NoriConfigToml,
        nori_home: PathBuf,
        overrides: NoriConfigOverrides,
    ) -> Result<Self> {
        let cwd = overrides
            .cwd
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();

        // Resolve MCP servers
        let mcp_servers = resolve_mcp_servers(toml.mcp_servers)?;

        Ok(Self {
            model: overrides
                .model
                .or(toml.model)
                .unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            sandbox_mode: overrides
                .sandbox_mode
                .or(toml.sandbox_mode)
                .unwrap_or(SandboxMode::WorkspaceWrite),
            approval_policy: overrides
                .approval_policy
                .or(toml.approval_policy)
                .unwrap_or(ApprovalPolicy::OnRequest),
            animations: toml.tui.animations.unwrap_or(true),
            notifications: toml.tui.notifications.unwrap_or(true),
            nori_home,
            cwd,
            mcp_servers,
        })
    }
}

/// Resolve MCP server configurations from TOML
fn resolve_mcp_servers(
    toml_servers: HashMap<String, super::types::McpServerConfigToml>,
) -> Result<HashMap<String, McpServerConfig>> {
    let mut resolved = HashMap::new();

    for (name, server_toml) in toml_servers {
        match server_toml.resolve() {
            Ok(config) => {
                resolved.insert(name, config);
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Invalid MCP server configuration '{name}': {e}"
                ));
            }
        }
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_load_with_mcp_servers() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
model = "claude-acp"

[mcp_servers.filesystem]
command = "npx"
args = ["@modelcontextprotocol/server-filesystem", "/tmp"]

[mcp_servers.web]
url = "https://mcp.example.com"
bearer_token_env_var = "MCP_TOKEN"
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();

        assert_eq!(config.mcp_servers.len(), 2);
        assert!(config.mcp_servers.contains_key("filesystem"));
        assert!(config.mcp_servers.contains_key("web"));
    }

    #[test]
    fn test_load_invalid_mcp_server() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[mcp_servers.invalid]
# Missing both command and url
enabled = true
"#,
        )
        .unwrap();

        let result = NoriConfig::load_from_path(&config_path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid"));
    }

    #[test]
    fn test_cwd_override_with_from_toml() {
        let toml = NoriConfigToml::default();
        let nori_home = PathBuf::from("/tmp/nori");
        let custom_cwd = PathBuf::from("/custom/path");

        let overrides = NoriConfigOverrides {
            cwd: Some(custom_cwd.clone()),
            ..Default::default()
        };

        let config = NoriConfig::from_toml(toml, nori_home, overrides).unwrap();
        assert_eq!(config.cwd, custom_cwd);
    }
}
