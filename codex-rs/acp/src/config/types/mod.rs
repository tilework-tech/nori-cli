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

/// Default agent for ACP-only mode
pub const DEFAULT_AGENT: &str = "claude-code";

// ============================================================================
// Agent Configuration (TOML schema)
// ============================================================================

/// A single agent definition from `[[agents]]` in config.toml.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentConfigToml {
    /// Display name shown in the agent picker (e.g. "Claude Code")
    pub name: String,
    /// Machine identifier used as a cmdline arg or in UIs (e.g. "claude-code")
    pub slug: String,
    /// How to invoke this agent
    pub distribution: AgentDistributionToml,
    /// Optional context window size override (in tokens)
    pub context_window_size: Option<i64>,
    /// Optional auth instructions (displayed on auth failures)
    pub auth_hint: Option<String>,
    /// Optional transcript base directory (relative to home)
    pub transcript_base_dir: Option<String>,
}

/// Distribution configuration for an agent.
///
/// Exactly one variant must be set. The field names correspond to the
/// package manager or distribution method.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentDistributionToml {
    /// Local binary execution
    pub local: Option<LocalDistribution>,
    /// Node.js: `npx <package> [args...]`
    pub npx: Option<PackageDistribution>,
    /// Bun: `bunx <package> [args...]`
    pub bunx: Option<PackageDistribution>,
    /// Python: `pipx run <package> [args...]`
    pub pipx: Option<PackageDistribution>,
    /// Python (uv): `uvx <package> [args...]`
    pub uvx: Option<PackageDistribution>,
    // Future: cargo (cargo-binstall / cargo install)
    // Future: binary (platform-specific archive downloads)
}

impl AgentDistributionToml {
    /// Validate that exactly one distribution variant is set.
    fn validate(&self) -> Result<(), String> {
        let count = [
            self.local.is_some(),
            self.npx.is_some(),
            self.bunx.is_some(),
            self.pipx.is_some(),
            self.uvx.is_some(),
        ]
        .iter()
        .filter(|&&b| b)
        .count();

        if count == 0 {
            return Err(
                "Agent distribution must specify exactly one of: local, npx, bunx, pipx, uvx"
                    .to_string(),
            );
        }
        if count > 1 {
            return Err(
                "Agent distribution must specify exactly one of: local, npx, bunx, pipx, uvx (found multiple)"
                    .to_string(),
            );
        }
        Ok(())
    }

    /// Validate and resolve into a clean enum variant.
    pub fn resolve(&self) -> Result<ResolvedDistribution, String> {
        self.validate()?;

        if let Some(local) = &self.local {
            return Ok(ResolvedDistribution::Local {
                command: local.command.clone(),
                args: local.args.clone(),
                env: local.env.clone(),
            });
        }
        if let Some(npx) = &self.npx {
            return Ok(ResolvedDistribution::Npx {
                package: npx.package.clone(),
                args: npx.args.clone(),
            });
        }
        if let Some(bunx) = &self.bunx {
            return Ok(ResolvedDistribution::Bunx {
                package: bunx.package.clone(),
                args: bunx.args.clone(),
            });
        }
        if let Some(pipx) = &self.pipx {
            return Ok(ResolvedDistribution::Pipx {
                package: pipx.package.clone(),
                args: pipx.args.clone(),
            });
        }
        if let Some(uvx) = &self.uvx {
            return Ok(ResolvedDistribution::Uvx {
                package: uvx.package.clone(),
                args: uvx.args.clone(),
            });
        }
        unreachable!("validate() ensures exactly one variant is set")
    }
}

/// Local binary distribution: direct command execution.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LocalDistribution {
    /// Path to the executable
    pub command: String,
    /// Arguments to pass to the command
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables to set
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Package manager distribution: `<manager> <package> [args...]`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PackageDistribution {
    /// Package name (e.g. "@google/gemini-cli", "kimi-cli")
    pub package: String,
    /// Extra arguments to pass after the package name
    #[serde(default)]
    pub args: Vec<String>,
}

/// Resolved (validated) distribution — exactly one variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedDistribution {
    /// Local binary: direct command execution
    Local {
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
    },
    /// Node.js: `npx <package> [args...]`
    Npx { package: String, args: Vec<String> },
    /// Bun: `bunx <package> [args...]`
    Bunx { package: String, args: Vec<String> },
    /// Python: `pipx run <package> [args...]`
    Pipx { package: String, args: Vec<String> },
    /// Python (uv): `uvx <package> [args...]`
    Uvx { package: String, args: Vec<String> },
    // Future: Cargo { crate_name: String, version: Option<String>, binary: Option<String> }
    // Future: Binary { url: String, platforms: HashMap<String, BinaryPlatformConfig> }
}

/// TOML-deserializable config structure (all fields optional)
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NoriConfigToml {
    /// The ACP agent to use (e.g., "claude-code", "codex", "gemini")
    /// This is persisted separately from model to track user's agent preference
    pub agent: Option<String>,

    /// Legacy field: the ACP agent to use. Prefer `agent` field.
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

    /// Session lifecycle hooks
    #[serde(default)]
    pub hooks: HooksConfigToml,

    /// Default model overrides per agent (e.g., claude-code = "haiku")
    #[serde(default)]
    pub default_models: HashMap<String, String>,

    /// Custom agent definitions
    #[serde(default)]
    pub agents: Vec<AgentConfigToml>,
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
// Auto Worktree Configuration
// ============================================================================

/// Whether to automatically create a git worktree at session start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AutoWorktree {
    /// Always create a worktree automatically.
    Automatic,
    /// Ask the user at session start whether to create a worktree.
    Ask,
    /// Never create a worktree automatically.
    #[default]
    Off,
}

impl AutoWorktree {
    /// Human-readable name for display in the TUI.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Ask => "Ask",
            Self::Off => "Off",
        }
    }

    /// TOML string representation for persistence.
    pub fn toml_value(&self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Ask => "ask",
            Self::Off => "off",
        }
    }

    /// All variants in order, for building picker UIs.
    pub fn all_variants() -> &'static [AutoWorktree] {
        &[Self::Automatic, Self::Ask, Self::Off]
    }

    /// Returns true if a worktree should be created (either automatically or
    /// after asking and getting confirmation).
    pub fn is_enabled(&self) -> bool {
        matches!(self, Self::Automatic | Self::Ask)
    }
}

impl Serialize for AutoWorktree {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.toml_value())
    }
}

impl<'de> Deserialize<'de> for AutoWorktree {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct AutoWorktreeVisitor;

        impl<'de> serde::de::Visitor<'de> for AutoWorktreeVisitor {
            type Value = AutoWorktree;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a boolean or one of \"automatic\", \"ask\", \"off\"")
            }

            fn visit_bool<E>(self, value: bool) -> Result<AutoWorktree, E>
            where
                E: serde::de::Error,
            {
                Ok(if value {
                    AutoWorktree::Automatic
                } else {
                    AutoWorktree::Off
                })
            }

            fn visit_str<E>(self, value: &str) -> Result<AutoWorktree, E>
            where
                E: serde::de::Error,
            {
                match value {
                    "automatic" => Ok(AutoWorktree::Automatic),
                    "ask" => Ok(AutoWorktree::Ask),
                    "off" => Ok(AutoWorktree::Off),
                    _ => Err(E::unknown_variant(value, &["automatic", "ask", "off"])),
                }
            }
        }

        deserializer.deserialize_any(AutoWorktreeVisitor)
    }
}

// ============================================================================
// Script Timeout Configuration
// ============================================================================

/// A freeform duration string for script execution timeouts (e.g. "30s", "2m").
///
/// Supported suffixes: `s` (seconds), `m` (minutes). The raw string is
/// preserved for display and TOML round-tripping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptTimeout(String, Duration);

impl ScriptTimeout {
    /// Default timeout: 30 seconds.
    const DEFAULT_SECS: u64 = 30;

    /// Parse a duration string like "30s" or "2m".
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        let duration = Self::parse_duration(s).unwrap_or(Duration::from_secs(Self::DEFAULT_SECS));
        Self(s.to_string(), duration)
    }

    fn parse_duration(s: &str) -> Option<Duration> {
        let s = s.trim();
        if let Some(num) = s.strip_suffix('s') {
            num.parse::<u64>().ok().map(Duration::from_secs)
        } else if let Some(num) = s.strip_suffix('m') {
            num.parse::<u64>().ok().map(|m| Duration::from_secs(m * 60))
        } else {
            s.parse::<u64>().ok().map(Duration::from_secs)
        }
    }

    /// The resolved duration.
    pub fn as_duration(&self) -> Duration {
        self.1
    }

    /// Human-readable name for display in the TUI.
    pub fn display_name(&self) -> &str {
        &self.0
    }

    /// TOML string representation for persistence.
    pub fn toml_value(&self) -> &str {
        &self.0
    }

    /// Common timeout values for building picker UIs.
    pub fn all_common_values() -> Vec<ScriptTimeout> {
        vec![
            ScriptTimeout::from_str("10s"),
            ScriptTimeout::from_str("30s"),
            ScriptTimeout::from_str("1m"),
            ScriptTimeout::from_str("2m"),
            ScriptTimeout::from_str("5m"),
        ]
    }
}

impl Default for ScriptTimeout {
    fn default() -> Self {
        Self("30s".to_string(), Duration::from_secs(Self::DEFAULT_SECS))
    }
}

impl Serialize for ScriptTimeout {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ScriptTimeout {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(ScriptTimeout::from_str(&s))
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
    /// Move cursor one character backward.
    MoveBackwardChar,
    /// Move cursor one character forward.
    MoveForwardChar,
    /// Move cursor to beginning of line.
    MoveBeginningOfLine,
    /// Move cursor to end of line.
    MoveEndOfLine,
    /// Move cursor one word backward.
    MoveBackwardWord,
    /// Move cursor one word forward.
    MoveForwardWord,
    /// Delete one character backward.
    DeleteBackwardChar,
    /// Delete one character forward.
    DeleteForwardChar,
    /// Delete one word backward.
    DeleteBackwardWord,
    /// Kill text to end of line.
    KillToEndOfLine,
    /// Kill text to beginning of line.
    KillToBeginningOfLine,
    /// Yank (paste) killed text.
    Yank,
    /// Search prompt history (reverse search).
    HistorySearch,
}

impl HotkeyAction {
    /// Human-readable name for display in the TUI.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::OpenTranscript => "Open Transcript",
            Self::OpenEditor => "Open Editor",
            Self::MoveBackwardChar => "Move Backward Char",
            Self::MoveForwardChar => "Move Forward Char",
            Self::MoveBeginningOfLine => "Move to Line Start",
            Self::MoveEndOfLine => "Move to Line End",
            Self::MoveBackwardWord => "Move Backward Word",
            Self::MoveForwardWord => "Move Forward Word",
            Self::DeleteBackwardChar => "Delete Backward Char",
            Self::DeleteForwardChar => "Delete Forward Char",
            Self::DeleteBackwardWord => "Delete Backward Word",
            Self::KillToEndOfLine => "Kill to Line End",
            Self::KillToBeginningOfLine => "Kill to Line Start",
            Self::Yank => "Yank",
            Self::HistorySearch => "History Search",
        }
    }

    /// Description for the hotkey picker.
    pub fn description(&self) -> &'static str {
        match self {
            Self::OpenTranscript => "Open the transcript pager (alternate screen)",
            Self::OpenEditor => "Open an external editor to compose a message",
            Self::MoveBackwardChar => "Move cursor one character backward",
            Self::MoveForwardChar => "Move cursor one character forward",
            Self::MoveBeginningOfLine => "Move cursor to beginning of line",
            Self::MoveEndOfLine => "Move cursor to end of line",
            Self::MoveBackwardWord => "Move cursor one word backward",
            Self::MoveForwardWord => "Move cursor one word forward",
            Self::DeleteBackwardChar => "Delete one character backward",
            Self::DeleteForwardChar => "Delete one character forward",
            Self::DeleteBackwardWord => "Delete one word backward",
            Self::KillToEndOfLine => "Kill text to end of line",
            Self::KillToBeginningOfLine => "Kill text to beginning of line",
            Self::Yank => "Yank (paste) killed text",
            Self::HistorySearch => "Search prompt history (reverse search)",
        }
    }

    /// The TOML key name for this action under `[tui.hotkeys]`.
    pub fn toml_key(&self) -> &'static str {
        match self {
            Self::OpenTranscript => "open_transcript",
            Self::OpenEditor => "open_editor",
            Self::MoveBackwardChar => "move_backward_char",
            Self::MoveForwardChar => "move_forward_char",
            Self::MoveBeginningOfLine => "move_beginning_of_line",
            Self::MoveEndOfLine => "move_end_of_line",
            Self::MoveBackwardWord => "move_backward_word",
            Self::MoveForwardWord => "move_forward_word",
            Self::DeleteBackwardChar => "delete_backward_char",
            Self::DeleteForwardChar => "delete_forward_char",
            Self::DeleteBackwardWord => "delete_backward_word",
            Self::KillToEndOfLine => "kill_to_end_of_line",
            Self::KillToBeginningOfLine => "kill_to_beginning_of_line",
            Self::Yank => "yank",
            Self::HistorySearch => "history_search",
        }
    }

    /// The default binding string for this action.
    pub fn default_binding(&self) -> &'static str {
        match self {
            Self::OpenTranscript => "ctrl+t",
            Self::OpenEditor => "ctrl+g",
            Self::MoveBackwardChar => "ctrl+b",
            Self::MoveForwardChar => "ctrl+f",
            Self::MoveBeginningOfLine => "ctrl+a",
            Self::MoveEndOfLine => "ctrl+e",
            Self::MoveBackwardWord => "alt+b",
            Self::MoveForwardWord => "alt+f",
            Self::DeleteBackwardChar => "ctrl+h",
            Self::DeleteForwardChar => "ctrl+d",
            Self::DeleteBackwardWord => "ctrl+w",
            Self::KillToEndOfLine => "ctrl+k",
            Self::KillToBeginningOfLine => "ctrl+u",
            Self::Yank => "ctrl+y",
            Self::HistorySearch => "ctrl+r",
        }
    }

    /// All hotkey actions, in display order.
    pub fn all_actions() -> &'static [HotkeyAction] {
        &[
            Self::OpenTranscript,
            Self::OpenEditor,
            Self::MoveBackwardChar,
            Self::MoveForwardChar,
            Self::MoveBeginningOfLine,
            Self::MoveEndOfLine,
            Self::MoveBackwardWord,
            Self::MoveForwardWord,
            Self::DeleteBackwardChar,
            Self::DeleteForwardChar,
            Self::DeleteBackwardWord,
            Self::KillToEndOfLine,
            Self::KillToBeginningOfLine,
            Self::Yank,
            Self::HistorySearch,
        ]
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
    /// Hotkey for moving cursor one character backward.
    pub move_backward_char: Option<HotkeyBinding>,
    /// Hotkey for moving cursor one character forward.
    pub move_forward_char: Option<HotkeyBinding>,
    /// Hotkey for moving cursor to beginning of line.
    pub move_beginning_of_line: Option<HotkeyBinding>,
    /// Hotkey for moving cursor to end of line.
    pub move_end_of_line: Option<HotkeyBinding>,
    /// Hotkey for moving cursor one word backward.
    pub move_backward_word: Option<HotkeyBinding>,
    /// Hotkey for moving cursor one word forward.
    pub move_forward_word: Option<HotkeyBinding>,
    /// Hotkey for deleting one character backward.
    pub delete_backward_char: Option<HotkeyBinding>,
    /// Hotkey for deleting one character forward.
    pub delete_forward_char: Option<HotkeyBinding>,
    /// Hotkey for deleting one word backward.
    pub delete_backward_word: Option<HotkeyBinding>,
    /// Hotkey for killing text to end of line.
    pub kill_to_end_of_line: Option<HotkeyBinding>,
    /// Hotkey for killing text to beginning of line.
    pub kill_to_beginning_of_line: Option<HotkeyBinding>,
    /// Hotkey for yanking (pasting) killed text.
    pub yank: Option<HotkeyBinding>,
    /// Hotkey for searching prompt history (reverse search).
    pub history_search: Option<HotkeyBinding>,
}

/// Resolved hotkey configuration with defaults applied.
#[derive(Debug, Clone)]
pub struct HotkeyConfig {
    /// Hotkey for opening the transcript pager.
    pub open_transcript: HotkeyBinding,
    /// Hotkey for opening an external editor.
    pub open_editor: HotkeyBinding,
    /// Hotkey for moving cursor one character backward.
    pub move_backward_char: HotkeyBinding,
    /// Hotkey for moving cursor one character forward.
    pub move_forward_char: HotkeyBinding,
    /// Hotkey for moving cursor to beginning of line.
    pub move_beginning_of_line: HotkeyBinding,
    /// Hotkey for moving cursor to end of line.
    pub move_end_of_line: HotkeyBinding,
    /// Hotkey for moving cursor one word backward.
    pub move_backward_word: HotkeyBinding,
    /// Hotkey for moving cursor one word forward.
    pub move_forward_word: HotkeyBinding,
    /// Hotkey for deleting one character backward.
    pub delete_backward_char: HotkeyBinding,
    /// Hotkey for deleting one character forward.
    pub delete_forward_char: HotkeyBinding,
    /// Hotkey for deleting one word backward.
    pub delete_backward_word: HotkeyBinding,
    /// Hotkey for killing text to end of line.
    pub kill_to_end_of_line: HotkeyBinding,
    /// Hotkey for killing text to beginning of line.
    pub kill_to_beginning_of_line: HotkeyBinding,
    /// Hotkey for yanking (pasting) killed text.
    pub yank: HotkeyBinding,
    /// Hotkey for searching prompt history (reverse search).
    pub history_search: HotkeyBinding,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            open_transcript: HotkeyBinding::from_str(
                HotkeyAction::OpenTranscript.default_binding(),
            ),
            open_editor: HotkeyBinding::from_str(HotkeyAction::OpenEditor.default_binding()),
            move_backward_char: HotkeyBinding::from_str(
                HotkeyAction::MoveBackwardChar.default_binding(),
            ),
            move_forward_char: HotkeyBinding::from_str(
                HotkeyAction::MoveForwardChar.default_binding(),
            ),
            move_beginning_of_line: HotkeyBinding::from_str(
                HotkeyAction::MoveBeginningOfLine.default_binding(),
            ),
            move_end_of_line: HotkeyBinding::from_str(
                HotkeyAction::MoveEndOfLine.default_binding(),
            ),
            move_backward_word: HotkeyBinding::from_str(
                HotkeyAction::MoveBackwardWord.default_binding(),
            ),
            move_forward_word: HotkeyBinding::from_str(
                HotkeyAction::MoveForwardWord.default_binding(),
            ),
            delete_backward_char: HotkeyBinding::from_str(
                HotkeyAction::DeleteBackwardChar.default_binding(),
            ),
            delete_forward_char: HotkeyBinding::from_str(
                HotkeyAction::DeleteForwardChar.default_binding(),
            ),
            delete_backward_word: HotkeyBinding::from_str(
                HotkeyAction::DeleteBackwardWord.default_binding(),
            ),
            kill_to_end_of_line: HotkeyBinding::from_str(
                HotkeyAction::KillToEndOfLine.default_binding(),
            ),
            kill_to_beginning_of_line: HotkeyBinding::from_str(
                HotkeyAction::KillToBeginningOfLine.default_binding(),
            ),
            yank: HotkeyBinding::from_str(HotkeyAction::Yank.default_binding()),
            history_search: HotkeyBinding::from_str(HotkeyAction::HistorySearch.default_binding()),
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
            move_backward_char: toml
                .move_backward_char
                .clone()
                .unwrap_or(defaults.move_backward_char),
            move_forward_char: toml
                .move_forward_char
                .clone()
                .unwrap_or(defaults.move_forward_char),
            move_beginning_of_line: toml
                .move_beginning_of_line
                .clone()
                .unwrap_or(defaults.move_beginning_of_line),
            move_end_of_line: toml
                .move_end_of_line
                .clone()
                .unwrap_or(defaults.move_end_of_line),
            move_backward_word: toml
                .move_backward_word
                .clone()
                .unwrap_or(defaults.move_backward_word),
            move_forward_word: toml
                .move_forward_word
                .clone()
                .unwrap_or(defaults.move_forward_word),
            delete_backward_char: toml
                .delete_backward_char
                .clone()
                .unwrap_or(defaults.delete_backward_char),
            delete_forward_char: toml
                .delete_forward_char
                .clone()
                .unwrap_or(defaults.delete_forward_char),
            delete_backward_word: toml
                .delete_backward_word
                .clone()
                .unwrap_or(defaults.delete_backward_word),
            kill_to_end_of_line: toml
                .kill_to_end_of_line
                .clone()
                .unwrap_or(defaults.kill_to_end_of_line),
            kill_to_beginning_of_line: toml
                .kill_to_beginning_of_line
                .clone()
                .unwrap_or(defaults.kill_to_beginning_of_line),
            yank: toml.yank.clone().unwrap_or(defaults.yank),
            history_search: toml
                .history_search
                .clone()
                .unwrap_or(defaults.history_search),
        }
    }

    /// Get the binding for a given action.
    pub fn binding_for(&self, action: HotkeyAction) -> &HotkeyBinding {
        match action {
            HotkeyAction::OpenTranscript => &self.open_transcript,
            HotkeyAction::OpenEditor => &self.open_editor,
            HotkeyAction::MoveBackwardChar => &self.move_backward_char,
            HotkeyAction::MoveForwardChar => &self.move_forward_char,
            HotkeyAction::MoveBeginningOfLine => &self.move_beginning_of_line,
            HotkeyAction::MoveEndOfLine => &self.move_end_of_line,
            HotkeyAction::MoveBackwardWord => &self.move_backward_word,
            HotkeyAction::MoveForwardWord => &self.move_forward_word,
            HotkeyAction::DeleteBackwardChar => &self.delete_backward_char,
            HotkeyAction::DeleteForwardChar => &self.delete_forward_char,
            HotkeyAction::DeleteBackwardWord => &self.delete_backward_word,
            HotkeyAction::KillToEndOfLine => &self.kill_to_end_of_line,
            HotkeyAction::KillToBeginningOfLine => &self.kill_to_beginning_of_line,
            HotkeyAction::Yank => &self.yank,
            HotkeyAction::HistorySearch => &self.history_search,
        }
    }

    /// Set the binding for a given action.
    pub fn set_binding(&mut self, action: HotkeyAction, binding: HotkeyBinding) {
        match action {
            HotkeyAction::OpenTranscript => self.open_transcript = binding,
            HotkeyAction::OpenEditor => self.open_editor = binding,
            HotkeyAction::MoveBackwardChar => self.move_backward_char = binding,
            HotkeyAction::MoveForwardChar => self.move_forward_char = binding,
            HotkeyAction::MoveBeginningOfLine => self.move_beginning_of_line = binding,
            HotkeyAction::MoveEndOfLine => self.move_end_of_line = binding,
            HotkeyAction::MoveBackwardWord => self.move_backward_word = binding,
            HotkeyAction::MoveForwardWord => self.move_forward_word = binding,
            HotkeyAction::DeleteBackwardChar => self.delete_backward_char = binding,
            HotkeyAction::DeleteForwardChar => self.delete_forward_char = binding,
            HotkeyAction::DeleteBackwardWord => self.delete_backward_word = binding,
            HotkeyAction::KillToEndOfLine => self.kill_to_end_of_line = binding,
            HotkeyAction::KillToBeginningOfLine => self.kill_to_beginning_of_line = binding,
            HotkeyAction::Yank => self.yank = binding,
            HotkeyAction::HistorySearch => self.history_search = binding,
        }
    }

    /// Return all (action, binding) pairs.
    pub fn all_bindings(&self) -> Vec<(HotkeyAction, &HotkeyBinding)> {
        vec![
            (HotkeyAction::OpenTranscript, &self.open_transcript),
            (HotkeyAction::OpenEditor, &self.open_editor),
            (HotkeyAction::MoveBackwardChar, &self.move_backward_char),
            (HotkeyAction::MoveForwardChar, &self.move_forward_char),
            (
                HotkeyAction::MoveBeginningOfLine,
                &self.move_beginning_of_line,
            ),
            (HotkeyAction::MoveEndOfLine, &self.move_end_of_line),
            (HotkeyAction::MoveBackwardWord, &self.move_backward_word),
            (HotkeyAction::MoveForwardWord, &self.move_forward_word),
            (HotkeyAction::DeleteBackwardChar, &self.delete_backward_char),
            (HotkeyAction::DeleteForwardChar, &self.delete_forward_char),
            (HotkeyAction::DeleteBackwardWord, &self.delete_backward_word),
            (HotkeyAction::KillToEndOfLine, &self.kill_to_end_of_line),
            (
                HotkeyAction::KillToBeginningOfLine,
                &self.kill_to_beginning_of_line,
            ),
            (HotkeyAction::Yank, &self.yank),
            (HotkeyAction::HistorySearch, &self.history_search),
        ]
    }
}

// ============================================================================
// Footer Segment Configuration
// ============================================================================

/// Individual footer segments that can be enabled/disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FooterSegment {
    /// Task summary: "Task: <summary>"
    PromptSummary,
    /// Vim mode indicator: "NORMAL" or "INSERT"
    VimMode,
    /// Git branch: "⎇ branch-name"
    GitBranch,
    /// Worktree name: "Worktree: name"
    WorktreeName,
    /// Git stats: "+10 -3"
    GitStats,
    /// Context window: "Context: 34K (27%)"
    Context,
    /// Approval mode: "Approvals: Agent"
    ApprovalMode,
    /// Nori profile: "Skillset: name"
    NoriProfile,
    /// Nori version: "Skillsets v19.1.1"
    NoriVersion,
    /// Token usage: "Tokens: 77K total (32K cached)"
    TokenUsage,
}

impl FooterSegment {
    /// Human-readable name for display in the TUI.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::PromptSummary => "Task Summary",
            Self::VimMode => "Vim Mode",
            Self::GitBranch => "Git Branch",
            Self::WorktreeName => "Worktree Name",
            Self::GitStats => "Git Stats",
            Self::Context => "Context Window",
            Self::ApprovalMode => "Approvals",
            Self::NoriProfile => "Skillset",
            Self::NoriVersion => "Skillset Version",
            Self::TokenUsage => "Token Usage",
        }
    }

    /// The TOML key name for this segment under `[tui.footer_segments]`.
    pub fn toml_key(&self) -> &'static str {
        match self {
            Self::PromptSummary => "prompt_summary",
            Self::VimMode => "vim_mode",
            Self::GitBranch => "git_branch",
            Self::WorktreeName => "worktree_name",
            Self::GitStats => "git_stats",
            Self::Context => "context",
            Self::ApprovalMode => "approval_mode",
            Self::NoriProfile => "nori_profile",
            Self::NoriVersion => "nori_version",
            Self::TokenUsage => "token_usage",
        }
    }

    /// All footer segment variants, in display order.
    pub fn all_variants() -> &'static [FooterSegment] {
        &[
            Self::PromptSummary,
            Self::VimMode,
            Self::GitBranch,
            Self::WorktreeName,
            Self::GitStats,
            Self::Context,
            Self::ApprovalMode,
            Self::NoriProfile,
            Self::NoriVersion,
            Self::TokenUsage,
        ]
    }

    /// Default order of footer segments (same as all_variants).
    pub fn default_order() -> &'static [FooterSegment] {
        Self::all_variants()
    }
}

impl fmt::Display for FooterSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

/// TOML-deserializable footer segment configuration.
/// Each field is optional - if not specified, the segment is enabled by default.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FooterSegmentConfigToml {
    /// Enable/disable task summary segment.
    pub prompt_summary: Option<bool>,
    /// Enable/disable vim mode indicator.
    pub vim_mode: Option<bool>,
    /// Enable/disable git branch segment.
    pub git_branch: Option<bool>,
    /// Enable/disable worktree name segment.
    pub worktree_name: Option<bool>,
    /// Enable/disable git stats segment.
    pub git_stats: Option<bool>,
    /// Enable/disable context window segment.
    pub context: Option<bool>,
    /// Enable/disable approval mode segment.
    pub approval_mode: Option<bool>,
    /// Enable/disable nori profile segment.
    pub nori_profile: Option<bool>,
    /// Enable/disable nori version segment.
    pub nori_version: Option<bool>,
    /// Enable/disable token usage segment.
    pub token_usage: Option<bool>,
}

/// Resolved footer segment configuration with defaults applied.
#[derive(Debug, Clone)]
pub struct FooterSegmentConfig {
    /// Enable/disable task summary segment.
    pub prompt_summary: bool,
    /// Enable/disable vim mode indicator.
    pub vim_mode: bool,
    /// Enable/disable git branch segment.
    pub git_branch: bool,
    /// Enable/disable worktree name segment.
    pub worktree_name: bool,
    /// Enable/disable git stats segment.
    pub git_stats: bool,
    /// Enable/disable context window segment.
    pub context: bool,
    /// Enable/disable approval mode segment.
    pub approval_mode: bool,
    /// Enable/disable nori profile segment.
    pub nori_profile: bool,
    /// Enable/disable nori version segment.
    pub nori_version: bool,
    /// Enable/disable token usage segment.
    pub token_usage: bool,
}

impl Default for FooterSegmentConfig {
    fn default() -> Self {
        Self {
            prompt_summary: true,
            vim_mode: true,
            git_branch: true,
            worktree_name: true,
            git_stats: true,
            context: true,
            approval_mode: true,
            nori_profile: true,
            nori_version: true,
            token_usage: true,
        }
    }
}

impl FooterSegmentConfig {
    /// Resolve from TOML config, applying defaults for missing values.
    pub fn from_toml(toml: &FooterSegmentConfigToml) -> Self {
        Self {
            prompt_summary: toml.prompt_summary.unwrap_or(true),
            vim_mode: toml.vim_mode.unwrap_or(true),
            git_branch: toml.git_branch.unwrap_or(true),
            worktree_name: toml.worktree_name.unwrap_or(true),
            git_stats: toml.git_stats.unwrap_or(true),
            context: toml.context.unwrap_or(true),
            approval_mode: toml.approval_mode.unwrap_or(true),
            nori_profile: toml.nori_profile.unwrap_or(true),
            nori_version: toml.nori_version.unwrap_or(true),
            token_usage: toml.token_usage.unwrap_or(true),
        }
    }

    /// Check if a segment is enabled.
    pub fn is_enabled(&self, segment: FooterSegment) -> bool {
        match segment {
            FooterSegment::PromptSummary => self.prompt_summary,
            FooterSegment::VimMode => self.vim_mode,
            FooterSegment::GitBranch => self.git_branch,
            FooterSegment::WorktreeName => self.worktree_name,
            FooterSegment::GitStats => self.git_stats,
            FooterSegment::Context => self.context,
            FooterSegment::ApprovalMode => self.approval_mode,
            FooterSegment::NoriProfile => self.nori_profile,
            FooterSegment::NoriVersion => self.nori_version,
            FooterSegment::TokenUsage => self.token_usage,
        }
    }

    /// Set whether a segment is enabled.
    pub fn set_enabled(&mut self, segment: FooterSegment, enabled: bool) {
        match segment {
            FooterSegment::PromptSummary => self.prompt_summary = enabled,
            FooterSegment::VimMode => self.vim_mode = enabled,
            FooterSegment::GitBranch => self.git_branch = enabled,
            FooterSegment::WorktreeName => self.worktree_name = enabled,
            FooterSegment::GitStats => self.git_stats = enabled,
            FooterSegment::Context => self.context = enabled,
            FooterSegment::ApprovalMode => self.approval_mode = enabled,
            FooterSegment::NoriProfile => self.nori_profile = enabled,
            FooterSegment::NoriVersion => self.nori_version = enabled,
            FooterSegment::TokenUsage => self.token_usage = enabled,
        }
    }

    /// Return all (segment, enabled) pairs in default order.
    pub fn all_settings(&self) -> Vec<(FooterSegment, bool)> {
        FooterSegment::all_variants()
            .iter()
            .map(|s| (*s, self.is_enabled(*s)))
            .collect()
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

    /// Enable vim-style navigation mode in the textarea.
    pub vim_mode: Option<bool>,

    /// Configurable hotkey bindings.
    #[serde(default)]
    pub hotkeys: HotkeyConfigToml,

    /// Footer segment visibility settings.
    #[serde(default)]
    pub footer_segments: FooterSegmentConfigToml,

    /// Timeout for custom prompt script execution.
    pub script_timeout: Option<ScriptTimeout>,

    /// Number of times to re-run the first prompt in fresh sessions.
    /// `None` or absent means disabled.
    pub loop_count: Option<i32>,

    /// Automatically create a git worktree at session start.
    pub auto_worktree: Option<AutoWorktree>,

    /// Enable per-session skillset isolation.
    pub skillset_per_session: Option<bool>,
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
#[derive(Default)]
pub enum ApprovalPolicy {
    /// Always ask for approval
    Always,
    /// Ask on potentially dangerous operations
    #[default]
    OnRequest,
    /// Never ask (dangerous)
    Never,
}

/// CLI overrides for config values
#[derive(Debug, Clone, Default)]
pub struct NoriConfigOverrides {
    /// Override the agent selection
    pub agent: Option<String>,

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

    /// The active ACP agent slug (CLI override > config model > persisted agent)
    pub active_agent: String,

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

    /// Enable vim-style navigation mode in the textarea.
    pub vim_mode: bool,

    /// Configurable hotkey bindings.
    pub hotkeys: HotkeyConfig,

    /// Timeout for custom prompt script execution.
    pub script_timeout: ScriptTimeout,

    /// Number of times to re-run the first prompt in fresh sessions.
    /// `None` means disabled (default).
    pub loop_count: Option<i32>,

    /// Automatically create a git worktree at session start.
    pub auto_worktree: AutoWorktree,

    /// Enable per-session skillset isolation.
    pub skillset_per_session: bool,

    /// Footer segment visibility configuration.
    pub footer_segment_config: FooterSegmentConfig,

    /// Nori home directory (~/.nori/cli)
    pub nori_home: PathBuf,

    /// Current working directory
    pub cwd: PathBuf,

    /// MCP server configurations
    pub mcp_servers: HashMap<String, McpServerConfig>,

    /// Scripts to run when a session starts.
    pub session_start_hooks: Vec<PathBuf>,

    /// Scripts to run when a session ends.
    pub session_end_hooks: Vec<PathBuf>,

    /// Scripts to run before a user prompt is sent to the agent.
    pub pre_user_prompt_hooks: Vec<PathBuf>,

    /// Scripts to run after a user prompt is sent to the agent.
    pub post_user_prompt_hooks: Vec<PathBuf>,

    /// Scripts to run before a tool call is executed.
    pub pre_tool_call_hooks: Vec<PathBuf>,

    /// Scripts to run after a tool call completes.
    pub post_tool_call_hooks: Vec<PathBuf>,

    /// Scripts to run before the agent produces a response.
    pub pre_agent_response_hooks: Vec<PathBuf>,

    /// Scripts to run after the agent finishes its response.
    pub post_agent_response_hooks: Vec<PathBuf>,

    /// Async (fire-and-forget) scripts to run when a session starts.
    pub async_session_start_hooks: Vec<PathBuf>,

    /// Async (fire-and-forget) scripts to run when a session ends.
    pub async_session_end_hooks: Vec<PathBuf>,

    /// Async (fire-and-forget) scripts to run before a user prompt is sent.
    pub async_pre_user_prompt_hooks: Vec<PathBuf>,

    /// Async (fire-and-forget) scripts to run after a user prompt is sent.
    pub async_post_user_prompt_hooks: Vec<PathBuf>,

    /// Async (fire-and-forget) scripts to run before a tool call is executed.
    pub async_pre_tool_call_hooks: Vec<PathBuf>,

    /// Async (fire-and-forget) scripts to run after a tool call completes.
    pub async_post_tool_call_hooks: Vec<PathBuf>,

    /// Async (fire-and-forget) scripts to run before the agent produces a response.
    pub async_pre_agent_response_hooks: Vec<PathBuf>,

    /// Async (fire-and-forget) scripts to run after the agent finishes its response.
    pub async_post_agent_response_hooks: Vec<PathBuf>,

    /// Default model overrides per agent (e.g., "claude-code" -> "haiku")
    pub default_models: HashMap<String, String>,

    /// Custom agent definitions from config
    pub agents: Vec<AgentConfigToml>,
}

impl Default for NoriConfig {
    fn default() -> Self {
        Self {
            agent: DEFAULT_AGENT.to_string(),
            active_agent: DEFAULT_AGENT.to_string(),
            sandbox_mode: SandboxMode::WorkspaceWrite,
            approval_policy: ApprovalPolicy::OnRequest,
            history_persistence: HistoryPersistence::default(),
            animations: true,
            terminal_notifications: TerminalNotifications::Enabled,
            os_notifications: OsNotifications::Enabled,
            vertical_footer: false,
            notify_after_idle: NotifyAfterIdle::default(),
            vim_mode: false,
            hotkeys: HotkeyConfig::default(),
            script_timeout: ScriptTimeout::default(),
            loop_count: None,
            auto_worktree: AutoWorktree::Off,
            skillset_per_session: false,
            footer_segment_config: FooterSegmentConfig::default(),
            nori_home: PathBuf::from(".nori/cli"),
            cwd: std::env::current_dir().unwrap_or_default(),
            mcp_servers: HashMap::new(),
            session_start_hooks: Vec::new(),
            session_end_hooks: Vec::new(),
            pre_user_prompt_hooks: Vec::new(),
            post_user_prompt_hooks: Vec::new(),
            pre_tool_call_hooks: Vec::new(),
            post_tool_call_hooks: Vec::new(),
            pre_agent_response_hooks: Vec::new(),
            post_agent_response_hooks: Vec::new(),
            async_session_start_hooks: Vec::new(),
            async_session_end_hooks: Vec::new(),
            async_pre_user_prompt_hooks: Vec::new(),
            async_post_user_prompt_hooks: Vec::new(),
            async_pre_tool_call_hooks: Vec::new(),
            async_post_tool_call_hooks: Vec::new(),
            async_pre_agent_response_hooks: Vec::new(),
            async_post_agent_response_hooks: Vec::new(),
            default_models: HashMap::new(),
            agents: Vec::new(),
        }
    }
}

// ============================================================================
// Session Hooks Configuration
// ============================================================================

/// TOML-deserializable hooks configuration.
///
/// Scripts are executed sequentially at session lifecycle boundaries.
/// Each entry is a path to a script file. The interpreter is determined
/// by file extension: `.sh` → bash, `.py` → python3, `.js` → node.
/// Files with no recognized extension are executed directly.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HooksConfigToml {
    /// Scripts to run when a session starts.
    #[serde(default)]
    pub session_start: Option<Vec<String>>,

    /// Scripts to run when a session ends.
    #[serde(default)]
    pub session_end: Option<Vec<String>>,

    /// Scripts to run before a user prompt is sent to the agent.
    #[serde(default)]
    pub pre_user_prompt: Option<Vec<String>>,

    /// Scripts to run after a user prompt is sent to the agent.
    #[serde(default)]
    pub post_user_prompt: Option<Vec<String>>,

    /// Scripts to run before a tool call is executed.
    #[serde(default)]
    pub pre_tool_call: Option<Vec<String>>,

    /// Scripts to run after a tool call completes.
    #[serde(default)]
    pub post_tool_call: Option<Vec<String>>,

    /// Scripts to run before the agent produces a response.
    #[serde(default)]
    pub pre_agent_response: Option<Vec<String>>,

    /// Scripts to run after the agent finishes its response.
    #[serde(default)]
    pub post_agent_response: Option<Vec<String>>,

    /// Async (fire-and-forget) scripts to run when a session starts.
    #[serde(default)]
    pub async_session_start: Option<Vec<String>>,

    /// Async (fire-and-forget) scripts to run when a session ends.
    #[serde(default)]
    pub async_session_end: Option<Vec<String>>,

    /// Async (fire-and-forget) scripts to run before a user prompt is sent.
    #[serde(default)]
    pub async_pre_user_prompt: Option<Vec<String>>,

    /// Async (fire-and-forget) scripts to run after a user prompt is sent.
    #[serde(default)]
    pub async_post_user_prompt: Option<Vec<String>>,

    /// Async (fire-and-forget) scripts to run before a tool call is executed.
    #[serde(default)]
    pub async_pre_tool_call: Option<Vec<String>>,

    /// Async (fire-and-forget) scripts to run after a tool call completes.
    #[serde(default)]
    pub async_post_tool_call: Option<Vec<String>>,

    /// Async (fire-and-forget) scripts to run before the agent produces a response.
    #[serde(default)]
    pub async_pre_agent_response: Option<Vec<String>>,

    /// Async (fire-and-forget) scripts to run after the agent finishes its response.
    #[serde(default)]
    pub async_post_agent_response: Option<Vec<String>>,
}

/// Expand a leading `~` to the user's home directory.
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(path)
}

/// Resolve a list of hook path strings into `PathBuf`s with tilde expansion.
pub fn resolve_hook_paths(paths: Option<Vec<String>>) -> Vec<PathBuf> {
    paths
        .unwrap_or_default()
        .into_iter()
        .map(|s| expand_tilde(&s))
        .collect()
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
mod tests;
