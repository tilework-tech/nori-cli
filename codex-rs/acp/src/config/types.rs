//! Type definitions for Nori configuration

use codex_protocol::config_types::SandboxMode;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt;
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

// ============================================================================
// Hotkey Configuration
// ============================================================================

/// A configurable hotkey action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyAction {
    /// Open the transcript pager overlay.
    OpenTranscript,
    /// Open an external editor for composing.
    OpenEditor,
}

impl HotkeyAction {
    /// Human-readable name for display in the TUI.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::OpenTranscript => "Open Transcript",
            Self::OpenEditor => "Open Editor",
        }
    }

    /// Description for the hotkey picker.
    pub fn description(&self) -> &'static str {
        match self {
            Self::OpenTranscript => "Open the transcript pager (alternate screen)",
            Self::OpenEditor => "Open an external editor to compose a message",
        }
    }

    /// The TOML key name for this action under `[tui.hotkeys]`.
    pub fn toml_key(&self) -> &'static str {
        match self {
            Self::OpenTranscript => "open_transcript",
            Self::OpenEditor => "open_editor",
        }
    }

    /// The default binding string for this action.
    pub fn default_binding(&self) -> &'static str {
        match self {
            Self::OpenTranscript => "ctrl+t",
            Self::OpenEditor => "ctrl+g",
        }
    }

    /// All hotkey actions, in display order.
    pub fn all_actions() -> &'static [HotkeyAction] {
        &[Self::OpenTranscript, Self::OpenEditor]
    }
}

impl fmt::Display for HotkeyAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

/// A hotkey binding represented as a string (e.g. "ctrl+t", "alt+g", "none").
///
/// The string format is: `[modifier+]key` where modifier is `ctrl`, `alt`, or `shift`,
/// and key is a single character, `enter`, `esc`, `f1`-`f12`, etc.
/// The special value `"none"` means the action is unbound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyBinding(Option<String>);

impl HotkeyBinding {
    /// Create a binding from a key string like "ctrl+t".
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        if s == "none" {
            Self(None)
        } else {
            Self(Some(s.to_lowercase()))
        }
    }

    /// Create an unbound (none) binding.
    pub fn none() -> Self {
        Self(None)
    }

    /// Returns true if this binding is unbound.
    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }

    /// Returns the binding string, or "none" if unbound.
    pub fn as_str(&self) -> &str {
        match &self.0 {
            Some(s) => s,
            None => "none",
        }
    }

    /// Human-readable display string (e.g. "ctrl + t" or "unbound").
    pub fn display_name(&self) -> String {
        match &self.0 {
            Some(s) => s.replace('+', " + "),
            None => "unbound".to_string(),
        }
    }

    /// TOML string for persistence.
    pub fn toml_value(&self) -> String {
        match &self.0 {
            Some(s) => s.clone(),
            None => "none".to_string(),
        }
    }
}

impl Serialize for HotkeyBinding {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.toml_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for HotkeyBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(HotkeyBinding::from_str(&s))
    }
}

/// TOML-deserializable hotkey configuration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HotkeyConfigToml {
    /// Hotkey for opening the transcript pager.
    pub open_transcript: Option<HotkeyBinding>,
    /// Hotkey for opening an external editor.
    pub open_editor: Option<HotkeyBinding>,
}

/// Resolved hotkey configuration with defaults applied.
#[derive(Debug, Clone)]
pub struct HotkeyConfig {
    /// Hotkey for opening the transcript pager.
    pub open_transcript: HotkeyBinding,
    /// Hotkey for opening an external editor.
    pub open_editor: HotkeyBinding,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            open_transcript: HotkeyBinding::from_str(
                HotkeyAction::OpenTranscript.default_binding(),
            ),
            open_editor: HotkeyBinding::from_str(HotkeyAction::OpenEditor.default_binding()),
        }
    }
}

impl HotkeyConfig {
    /// Resolve from TOML config, applying defaults for missing values.
    pub fn from_toml(toml: &HotkeyConfigToml) -> Self {
        let defaults = Self::default();
        Self {
            open_transcript: toml
                .open_transcript
                .clone()
                .unwrap_or(defaults.open_transcript),
            open_editor: toml.open_editor.clone().unwrap_or(defaults.open_editor),
        }
    }

    /// Get the binding for a given action.
    pub fn binding_for(&self, action: HotkeyAction) -> &HotkeyBinding {
        match action {
            HotkeyAction::OpenTranscript => &self.open_transcript,
            HotkeyAction::OpenEditor => &self.open_editor,
        }
    }

    /// Set the binding for a given action.
    pub fn set_binding(&mut self, action: HotkeyAction, binding: HotkeyBinding) {
        match action {
            HotkeyAction::OpenTranscript => self.open_transcript = binding,
            HotkeyAction::OpenEditor => self.open_editor = binding,
        }
    }

    /// Return all (action, binding) pairs.
    pub fn all_bindings(&self) -> Vec<(HotkeyAction, &HotkeyBinding)> {
        vec![
            (HotkeyAction::OpenTranscript, &self.open_transcript),
            (HotkeyAction::OpenEditor, &self.open_editor),
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

    /// Configurable hotkey bindings.
    #[serde(default)]
    pub hotkeys: HotkeyConfigToml,
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

    /// Configurable hotkey bindings.
    pub hotkeys: HotkeyConfig,

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
            hotkeys: HotkeyConfig::default(),
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

    // ========================================================================
    // Hotkey Configuration Tests
    // ========================================================================

    #[test]
    fn test_hotkey_binding_from_str_ctrl_t() {
        let binding = HotkeyBinding::from_str("ctrl+t");
        assert_eq!(binding.as_str(), "ctrl+t");
        assert!(!binding.is_none());
    }

    #[test]
    fn test_hotkey_binding_from_str_none() {
        let binding = HotkeyBinding::from_str("none");
        assert!(binding.is_none());
        assert_eq!(binding.as_str(), "none");
    }

    #[test]
    fn test_hotkey_binding_from_str_normalizes_case() {
        let binding = HotkeyBinding::from_str("Ctrl+T");
        assert_eq!(binding.as_str(), "ctrl+t");
    }

    #[test]
    fn test_hotkey_binding_display_name() {
        let binding = HotkeyBinding::from_str("ctrl+t");
        assert_eq!(binding.display_name(), "ctrl + t");

        let unbound = HotkeyBinding::none();
        assert_eq!(unbound.display_name(), "unbound");
    }

    #[test]
    fn test_hotkey_binding_toml_value() {
        let binding = HotkeyBinding::from_str("ctrl+g");
        assert_eq!(binding.toml_value(), "ctrl+g");

        let unbound = HotkeyBinding::none();
        assert_eq!(unbound.toml_value(), "none");
    }

    #[test]
    fn test_hotkey_binding_serde_roundtrip() {
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            key: HotkeyBinding,
        }

        let w = Wrapper {
            key: HotkeyBinding::from_str("ctrl+t"),
        };
        let toml_str = toml::to_string(&w).unwrap();
        let parsed: Wrapper = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.key, HotkeyBinding::from_str("ctrl+t"));
    }

    #[test]
    fn test_hotkey_binding_serde_none_roundtrip() {
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            key: HotkeyBinding,
        }

        let w = Wrapper {
            key: HotkeyBinding::none(),
        };
        let toml_str = toml::to_string(&w).unwrap();
        let parsed: Wrapper = toml::from_str(&toml_str).unwrap();
        assert!(parsed.key.is_none());
    }

    #[test]
    fn test_hotkey_binding_deserialize_from_toml_string() {
        #[derive(Deserialize)]
        struct Wrapper {
            key: HotkeyBinding,
        }

        let w: Wrapper = toml::from_str(r#"key = "alt+x""#).unwrap();
        assert_eq!(w.key.as_str(), "alt+x");

        let w: Wrapper = toml::from_str(r#"key = "none""#).unwrap();
        assert!(w.key.is_none());
    }

    #[test]
    fn test_hotkey_action_display_names() {
        assert_eq!(
            HotkeyAction::OpenTranscript.display_name(),
            "Open Transcript"
        );
        assert_eq!(HotkeyAction::OpenEditor.display_name(), "Open Editor");
    }

    #[test]
    fn test_hotkey_action_toml_keys() {
        assert_eq!(HotkeyAction::OpenTranscript.toml_key(), "open_transcript");
        assert_eq!(HotkeyAction::OpenEditor.toml_key(), "open_editor");
    }

    #[test]
    fn test_hotkey_action_default_bindings() {
        assert_eq!(HotkeyAction::OpenTranscript.default_binding(), "ctrl+t");
        assert_eq!(HotkeyAction::OpenEditor.default_binding(), "ctrl+g");
    }

    #[test]
    fn test_hotkey_action_all_actions() {
        let actions = HotkeyAction::all_actions();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0], HotkeyAction::OpenTranscript);
        assert_eq!(actions[1], HotkeyAction::OpenEditor);
    }

    #[test]
    fn test_hotkey_config_default_uses_standard_bindings() {
        let config = HotkeyConfig::default();
        assert_eq!(config.open_transcript, HotkeyBinding::from_str("ctrl+t"));
        assert_eq!(config.open_editor, HotkeyBinding::from_str("ctrl+g"));
    }

    #[test]
    fn test_hotkey_config_from_toml_uses_defaults_when_empty() {
        let toml = HotkeyConfigToml::default();
        let config = HotkeyConfig::from_toml(&toml);
        assert_eq!(config.open_transcript, HotkeyBinding::from_str("ctrl+t"));
        assert_eq!(config.open_editor, HotkeyBinding::from_str("ctrl+g"));
    }

    #[test]
    fn test_hotkey_config_from_toml_uses_custom_bindings() {
        let toml = HotkeyConfigToml {
            open_transcript: Some(HotkeyBinding::from_str("alt+t")),
            open_editor: Some(HotkeyBinding::from_str("ctrl+e")),
        };
        let config = HotkeyConfig::from_toml(&toml);
        assert_eq!(config.open_transcript, HotkeyBinding::from_str("alt+t"));
        assert_eq!(config.open_editor, HotkeyBinding::from_str("ctrl+e"));
    }

    #[test]
    fn test_hotkey_config_from_toml_partial_override() {
        let toml = HotkeyConfigToml {
            open_transcript: Some(HotkeyBinding::from_str("alt+t")),
            open_editor: None,
        };
        let config = HotkeyConfig::from_toml(&toml);
        assert_eq!(config.open_transcript, HotkeyBinding::from_str("alt+t"));
        assert_eq!(config.open_editor, HotkeyBinding::from_str("ctrl+g")); // default
    }

    #[test]
    fn test_hotkey_config_from_toml_unbind_action() {
        let toml = HotkeyConfigToml {
            open_transcript: Some(HotkeyBinding::none()),
            open_editor: None,
        };
        let config = HotkeyConfig::from_toml(&toml);
        assert!(config.open_transcript.is_none());
        assert_eq!(config.open_editor, HotkeyBinding::from_str("ctrl+g"));
    }

    #[test]
    fn test_hotkey_config_binding_for_action() {
        let config = HotkeyConfig::default();
        assert_eq!(
            config.binding_for(HotkeyAction::OpenTranscript),
            &HotkeyBinding::from_str("ctrl+t")
        );
        assert_eq!(
            config.binding_for(HotkeyAction::OpenEditor),
            &HotkeyBinding::from_str("ctrl+g")
        );
    }

    #[test]
    fn test_hotkey_config_set_binding() {
        let mut config = HotkeyConfig::default();
        config.set_binding(HotkeyAction::OpenTranscript, HotkeyBinding::from_str("f1"));
        assert_eq!(config.open_transcript, HotkeyBinding::from_str("f1"));
    }

    #[test]
    fn test_hotkey_config_all_bindings() {
        let config = HotkeyConfig::default();
        let bindings = config.all_bindings();
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].0, HotkeyAction::OpenTranscript);
        assert_eq!(bindings[1].0, HotkeyAction::OpenEditor);
    }

    #[test]
    fn test_tui_config_toml_with_hotkeys() {
        let config: TuiConfigToml = toml::from_str(
            r#"
[hotkeys]
open_transcript = "alt+t"
open_editor = "ctrl+e"
"#,
        )
        .unwrap();
        assert_eq!(
            config.hotkeys.open_transcript,
            Some(HotkeyBinding::from_str("alt+t"))
        );
        assert_eq!(
            config.hotkeys.open_editor,
            Some(HotkeyBinding::from_str("ctrl+e"))
        );
    }

    #[test]
    fn test_tui_config_toml_without_hotkeys() {
        let config: TuiConfigToml = toml::from_str("").unwrap();
        assert!(config.hotkeys.open_transcript.is_none());
        assert!(config.hotkeys.open_editor.is_none());
    }

    #[test]
    fn test_full_config_toml_with_hotkeys() {
        let config: NoriConfigToml = toml::from_str(
            r#"
model = "claude-code"

[tui]
vertical_footer = true

[tui.hotkeys]
open_transcript = "ctrl+y"
open_editor = "none"
"#,
        )
        .unwrap();
        assert_eq!(
            config.tui.hotkeys.open_transcript,
            Some(HotkeyBinding::from_str("ctrl+y"))
        );
        assert_eq!(config.tui.hotkeys.open_editor, Some(HotkeyBinding::none()));
    }
}
