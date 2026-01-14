//! Type definitions for Nori configuration

use codex_protocol::config_types::SandboxMode;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// History persistence policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HistoryPersistence {
    /// Save all history entries to disk.
    #[default]
    SaveAll,
    /// Do not write history to disk.
    None,
}

/// Default model for ACP-only mode
pub const DEFAULT_MODEL: &str = "claude-code";

/// TOML-deserializable config structure (all fields optional)
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NoriConfigToml {
    /// The ACP agent to use (e.g., "claude-code", "codex", "gemini")
    /// This is persisted separately from model to track user's agent preference
    pub agent: Option<String>,

    /// The ACP agent model to use (e.g., "claude-code", "codex", "gemini")
    pub model: Option<String>,

    /// Sandbox mode for command execution
    pub sandbox_mode: Option<SandboxMode>,

    /// Approval policy for commands
    pub approval_policy: Option<ApprovalPolicy>,

    /// History persistence policy
    pub history_persistence: Option<HistoryPersistence>,

    /// TUI settings
    #[serde(default)]
    pub tui: TuiConfigToml,

    /// MCP server configurations (optional)
    #[serde(default)]
    pub mcp_servers: HashMap<String, McpServerConfigToml>,
}

/// TUI-specific settings (TOML)
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TuiConfigToml {
    /// Enable animations (shimmer effects, spinners)
    pub animations: Option<bool>,

    /// Enable desktop notifications
    pub notifications: Option<bool>,
}

/// Resolved TUI configuration
#[derive(Debug, Clone)]
pub struct TuiConfig {
    /// Enable animations (shimmer effects, spinners)
    pub animations: bool,

    /// Enable desktop notifications
    pub notifications: bool,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            animations: true,
            notifications: true,
        }
    }
}

/// Approval policy for command execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalPolicy {
    /// Always ask for approval
    Always,
    /// Ask on potentially dangerous operations
    OnRequest,
    /// Never ask (dangerous)
    Never,
}

impl Default for ApprovalPolicy {
    fn default() -> Self {
        Self::OnRequest
    }
}

/// CLI overrides for config values
#[derive(Debug, Clone, Default)]
pub struct NoriConfigOverrides {
    /// Override the model selection
    pub model: Option<String>,

    /// Override sandbox mode
    pub sandbox_mode: Option<SandboxMode>,

    /// Override approval policy
    pub approval_policy: Option<ApprovalPolicy>,

    /// Override current working directory
    pub cwd: Option<PathBuf>,
}

/// Resolved configuration with defaults applied
#[derive(Debug, Clone)]
pub struct NoriConfig {
    /// The ACP agent to use (e.g., "claude-code", "codex", "gemini")
    /// Persisted to track user's agent preference across sessions
    pub agent: String,

    /// The ACP agent model to use
    pub model: String,

    /// Sandbox mode for command execution
    pub sandbox_mode: SandboxMode,

    /// Approval policy for commands
    pub approval_policy: ApprovalPolicy,

    /// History persistence policy
    pub history_persistence: HistoryPersistence,

    /// Enable TUI animations
    pub animations: bool,

    /// Enable TUI notifications
    pub notifications: bool,

    /// Nori home directory (~/.nori/cli)
    pub nori_home: PathBuf,

    /// Current working directory
    pub cwd: PathBuf,

    /// MCP server configurations
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

impl Default for NoriConfig {
    fn default() -> Self {
        Self {
            agent: DEFAULT_MODEL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            sandbox_mode: SandboxMode::WorkspaceWrite,
            approval_policy: ApprovalPolicy::OnRequest,
            history_persistence: HistoryPersistence::default(),
            animations: true,
            notifications: true,
            nori_home: PathBuf::from(".nori/cli"),
            cwd: std::env::current_dir().unwrap_or_default(),
            mcp_servers: HashMap::new(),
        }
    }
}

// ============================================================================
// MCP Server Configuration
// ============================================================================

/// MCP server configuration (TOML representation)
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct McpServerConfigToml {
    // Stdio transport fields
    /// Command to execute
    pub command: Option<String>,
    /// Arguments to pass to the command
    #[serde(default)]
    pub args: Option<Vec<String>>,
    /// Environment variables to set
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    /// Environment variable names to inherit
    #[serde(default)]
    pub env_vars: Option<Vec<String>>,
    /// Working directory for the command
    pub cwd: Option<PathBuf>,

    // HTTP transport fields
    /// URL for HTTP-based MCP server
    pub url: Option<String>,
    /// Environment variable containing bearer token
    pub bearer_token_env_var: Option<String>,
    /// HTTP headers to include
    #[serde(default)]
    pub http_headers: Option<HashMap<String, String>>,
    /// HTTP headers sourced from environment variables
    #[serde(default)]
    pub env_http_headers: Option<HashMap<String, String>>,

    // Shared fields
    /// Whether this server is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Startup timeout in seconds
    pub startup_timeout_sec: Option<f64>,
    /// Tool call timeout in seconds
    pub tool_timeout_sec: Option<f64>,
    /// Allow-list of tool names
    pub enabled_tools: Option<Vec<String>>,
    /// Deny-list of tool names
    pub disabled_tools: Option<Vec<String>>,
}

fn default_true() -> bool {
    true
}

/// Resolved MCP server configuration
#[derive(Debug, Clone, PartialEq)]
pub struct McpServerConfig {
    /// Transport configuration
    pub transport: McpServerTransportConfig,

    /// Whether this server is enabled
    pub enabled: bool,

    /// Startup timeout
    pub startup_timeout: Option<Duration>,

    /// Tool call timeout
    pub tool_timeout: Option<Duration>,

    /// Allow-list of tools
    pub enabled_tools: Option<Vec<String>>,

    /// Deny-list of tools
    pub disabled_tools: Option<Vec<String>>,
}

/// MCP server transport configuration
#[derive(Debug, Clone, PartialEq)]
pub enum McpServerTransportConfig {
    /// Stdio-based MCP server (subprocess)
    Stdio {
        command: String,
        args: Vec<String>,
        env: Option<HashMap<String, String>>,
        env_vars: Vec<String>,
        cwd: Option<PathBuf>,
    },
    /// HTTP-based MCP server
    StreamableHttp {
        url: String,
        bearer_token_env_var: Option<String>,
        http_headers: Option<HashMap<String, String>>,
        env_http_headers: Option<HashMap<String, String>>,
    },
}

impl McpServerConfigToml {
    /// Convert TOML representation to resolved config
    pub fn resolve(&self) -> Result<McpServerConfig, String> {
        let transport = if let Some(command) = &self.command {
            if self.url.is_some() {
                return Err("Cannot specify both 'command' and 'url'".to_string());
            }
            McpServerTransportConfig::Stdio {
                command: command.clone(),
                args: self.args.clone().unwrap_or_default(),
                env: self.env.clone(),
                env_vars: self.env_vars.clone().unwrap_or_default(),
                cwd: self.cwd.clone(),
            }
        } else if let Some(url) = &self.url {
            McpServerTransportConfig::StreamableHttp {
                url: url.clone(),
                bearer_token_env_var: self.bearer_token_env_var.clone(),
                http_headers: self.http_headers.clone(),
                env_http_headers: self.env_http_headers.clone(),
            }
        } else {
            return Err("Must specify either 'command' or 'url'".to_string());
        };

        Ok(McpServerConfig {
            transport,
            enabled: self.enabled,
            startup_timeout: self.startup_timeout_sec.map(Duration::from_secs_f64),
            tool_timeout: self.tool_timeout_sec.map(Duration::from_secs_f64),
            enabled_tools: self.enabled_tools.clone(),
            disabled_tools: self.disabled_tools.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_approval_policy_deserialize() {
        #[derive(Deserialize)]
        struct Wrapper {
            policy: ApprovalPolicy,
        }

        let w: Wrapper = toml::from_str(r#"policy = "always""#).unwrap();
        assert_eq!(w.policy, ApprovalPolicy::Always);

        let w: Wrapper = toml::from_str(r#"policy = "on-request""#).unwrap();
        assert_eq!(w.policy, ApprovalPolicy::OnRequest);

        let w: Wrapper = toml::from_str(r#"policy = "never""#).unwrap();
        assert_eq!(w.policy, ApprovalPolicy::Never);
    }

    #[test]
    fn test_mcp_server_resolve_stdio() {
        let toml = McpServerConfigToml {
            command: Some("my-tool".to_string()),
            args: Some(vec!["--verbose".to_string()]),
            enabled: true,
            ..Default::default()
        };

        let config = toml.resolve().unwrap();
        assert!(matches!(
            config.transport,
            McpServerTransportConfig::Stdio { .. }
        ));
        assert!(config.enabled);
    }

    #[test]
    fn test_mcp_server_resolve_http() {
        let toml = McpServerConfigToml {
            url: Some("https://example.com/mcp".to_string()),
            bearer_token_env_var: Some("API_TOKEN".to_string()),
            enabled: true,
            ..Default::default()
        };

        let config = toml.resolve().unwrap();
        assert!(matches!(
            config.transport,
            McpServerTransportConfig::StreamableHttp { .. }
        ));
    }

    #[test]
    fn test_mcp_server_resolve_error_both() {
        let toml = McpServerConfigToml {
            command: Some("my-tool".to_string()),
            url: Some("https://example.com/mcp".to_string()),
            ..Default::default()
        };

        assert!(toml.resolve().is_err());
    }

    #[test]
    fn test_mcp_server_resolve_error_neither() {
        let toml = McpServerConfigToml::default();
        assert!(toml.resolve().is_err());
    }

    #[test]
    fn test_history_persistence_deserialize() {
        #[derive(Deserialize)]
        struct Wrapper {
            persistence: HistoryPersistence,
        }

        let w: Wrapper = toml::from_str(r#"persistence = "save-all""#).unwrap();
        assert_eq!(w.persistence, HistoryPersistence::SaveAll);

        let w: Wrapper = toml::from_str(r#"persistence = "none""#).unwrap();
        assert_eq!(w.persistence, HistoryPersistence::None);
    }

    #[test]
    fn test_history_persistence_default() {
        assert_eq!(HistoryPersistence::default(), HistoryPersistence::SaveAll);
    }
}
