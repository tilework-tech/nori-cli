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

/// Whether terminal notifications (OSC 9) are enabled or disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalNotifications {
    Enabled,
    Disabled,
}

/// Whether OS-level desktop notifications are enabled or disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OsNotifications {
    Enabled,
    Disabled,
}

/// How long after idle before sending a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NotifyAfterIdle {
    #[default]
    #[serde(rename = "5s")]
    FiveSeconds,
    #[serde(rename = "10s")]
    TenSeconds,
    #[serde(rename = "30s")]
    ThirtySeconds,
    #[serde(rename = "60s")]
    SixtySeconds,
    #[serde(rename = "disabled")]
    Disabled,
}

impl NotifyAfterIdle {
    /// Returns the duration for the idle timeout, or `None` if disabled.
    pub fn as_duration(&self) -> Option<Duration> {
        match self {
            Self::FiveSeconds => Some(Duration::from_secs(5)),
            Self::TenSeconds => Some(Duration::from_secs(10)),
            Self::ThirtySeconds => Some(Duration::from_secs(30)),
            Self::SixtySeconds => Some(Duration::from_secs(60)),
            Self::Disabled => None,
        }
    }

    /// Human-readable name for display in the TUI.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::FiveSeconds => "5 seconds",
            Self::TenSeconds => "10 seconds",
            Self::ThirtySeconds => "30 seconds",
            Self::SixtySeconds => "1 minute",
            Self::Disabled => "Disabled",
        }
    }

    /// TOML string representation for persistence.
    pub fn toml_value(&self) -> &'static str {
        match self {
            Self::FiveSeconds => "5s",
            Self::TenSeconds => "10s",
            Self::ThirtySeconds => "30s",
            Self::SixtySeconds => "60s",
            Self::Disabled => "disabled",
        }
    }

    /// All variants in order, for building picker UIs.
    pub fn all_variants() -> &'static [NotifyAfterIdle] {
        &[
            Self::FiveSeconds,
            Self::TenSeconds,
            Self::ThirtySeconds,
            Self::SixtySeconds,
            Self::Disabled,
        ]
    }
}

/// TUI-specific settings (TOML)
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TuiConfigToml {
    /// Enable animations (shimmer effects, spinners)
    pub animations: Option<bool>,

    /// Terminal notification preference (OSC 9 escape sequences)
    pub terminal_notifications: Option<TerminalNotifications>,

    /// OS-level desktop notification preference (notify-rust)
    pub os_notifications: Option<OsNotifications>,

    /// Stack footer segments vertically in the status footer.
    pub vertical_footer: Option<bool>,

    /// How long after idle before sending a notification.
    pub notify_after_idle: Option<NotifyAfterIdle>,
}

/// Resolved TUI configuration
#[derive(Debug, Clone)]
pub struct TuiConfig {
    /// Enable animations (shimmer effects, spinners)
    pub animations: bool,

    /// Terminal notification preference (OSC 9 escape sequences)
    pub terminal_notifications: TerminalNotifications,

    /// OS-level desktop notification preference (notify-rust)
    pub os_notifications: OsNotifications,

    /// Stack footer segments vertically in the status footer.
    pub vertical_footer: bool,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            animations: true,
            terminal_notifications: TerminalNotifications::Enabled,
            os_notifications: OsNotifications::Enabled,
            vertical_footer: false,
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

    /// Terminal notification preference (OSC 9 escape sequences)
    pub terminal_notifications: TerminalNotifications,

    /// OS-level desktop notification preference (notify-rust)
    pub os_notifications: OsNotifications,

    /// Stack footer segments vertically in the status footer.
    pub vertical_footer: bool,

    /// How long after idle before sending a notification.
    pub notify_after_idle: NotifyAfterIdle,

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
            terminal_notifications: TerminalNotifications::Enabled,
            os_notifications: OsNotifications::Enabled,
            vertical_footer: false,
            notify_after_idle: NotifyAfterIdle::default(),
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

    #[test]
    fn test_notify_after_idle_deserialize_all_variants() {
        #[derive(Deserialize)]
        struct Wrapper {
            value: NotifyAfterIdle,
        }

        let w: Wrapper = toml::from_str(r#"value = "5s""#).unwrap();
        assert_eq!(w.value, NotifyAfterIdle::FiveSeconds);

        let w: Wrapper = toml::from_str(r#"value = "10s""#).unwrap();
        assert_eq!(w.value, NotifyAfterIdle::TenSeconds);

        let w: Wrapper = toml::from_str(r#"value = "30s""#).unwrap();
        assert_eq!(w.value, NotifyAfterIdle::ThirtySeconds);

        let w: Wrapper = toml::from_str(r#"value = "60s""#).unwrap();
        assert_eq!(w.value, NotifyAfterIdle::SixtySeconds);

        let w: Wrapper = toml::from_str(r#"value = "disabled""#).unwrap();
        assert_eq!(w.value, NotifyAfterIdle::Disabled);
    }

    #[test]
    fn test_notify_after_idle_default() {
        assert_eq!(NotifyAfterIdle::default(), NotifyAfterIdle::FiveSeconds);
    }

    #[test]
    fn test_notify_after_idle_as_duration() {
        assert_eq!(
            NotifyAfterIdle::FiveSeconds.as_duration(),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            NotifyAfterIdle::TenSeconds.as_duration(),
            Some(Duration::from_secs(10))
        );
        assert_eq!(
            NotifyAfterIdle::ThirtySeconds.as_duration(),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            NotifyAfterIdle::SixtySeconds.as_duration(),
            Some(Duration::from_secs(60))
        );
        assert_eq!(NotifyAfterIdle::Disabled.as_duration(), None);
    }

    #[test]
    fn test_notify_after_idle_display_name() {
        assert_eq!(NotifyAfterIdle::FiveSeconds.display_name(), "5 seconds");
        assert_eq!(NotifyAfterIdle::TenSeconds.display_name(), "10 seconds");
        assert_eq!(NotifyAfterIdle::ThirtySeconds.display_name(), "30 seconds");
        assert_eq!(NotifyAfterIdle::SixtySeconds.display_name(), "1 minute");
        assert_eq!(NotifyAfterIdle::Disabled.display_name(), "Disabled");
    }

    #[test]
    fn test_notify_after_idle_toml_value() {
        assert_eq!(NotifyAfterIdle::FiveSeconds.toml_value(), "5s");
        assert_eq!(NotifyAfterIdle::TenSeconds.toml_value(), "10s");
        assert_eq!(NotifyAfterIdle::ThirtySeconds.toml_value(), "30s");
        assert_eq!(NotifyAfterIdle::SixtySeconds.toml_value(), "60s");
        assert_eq!(NotifyAfterIdle::Disabled.toml_value(), "disabled");
    }

    #[test]
    fn test_notify_after_idle_all_variants() {
        let variants = NotifyAfterIdle::all_variants();
        assert_eq!(variants.len(), 5);
        assert_eq!(variants[0], NotifyAfterIdle::FiveSeconds);
        assert_eq!(variants[4], NotifyAfterIdle::Disabled);
    }

    #[test]
    fn test_tui_config_toml_with_notify_after_idle() {
        let config: TuiConfigToml = toml::from_str(
            r#"
notify_after_idle = "30s"
"#,
        )
        .unwrap();
        assert_eq!(
            config.notify_after_idle,
            Some(NotifyAfterIdle::ThirtySeconds)
        );
    }

    #[test]
    fn test_tui_config_toml_without_notify_after_idle() {
        let config: TuiConfigToml = toml::from_str("").unwrap();
        assert_eq!(config.notify_after_idle, None);
    }
}
