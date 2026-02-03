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

    /// Session lifecycle hooks
    #[serde(default)]
    pub hooks: HooksConfigToml,
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
    /// Git stats: "+10 -3"
    GitStats,
    /// Context window: "Context: 34K (27%)"
    Context,
    /// Approval mode: "Approval Mode: Agent"
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
            Self::GitStats => "Git Stats",
            Self::Context => "Context Window",
            Self::ApprovalMode => "Approval Mode",
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
    pub auto_worktree: Option<bool>,
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
    pub auto_worktree: bool,

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
            vim_mode: false,
            hotkeys: HotkeyConfig::default(),
            script_timeout: ScriptTimeout::default(),
            loop_count: None,
            auto_worktree: false,
            footer_segment_config: FooterSegmentConfig::default(),
            nori_home: PathBuf::from(".nori/cli"),
            cwd: std::env::current_dir().unwrap_or_default(),
            mcp_servers: HashMap::new(),
            session_start_hooks: Vec::new(),
            session_end_hooks: Vec::new(),
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
        assert_eq!(actions.len(), 14);
        assert_eq!(actions[0], HotkeyAction::OpenTranscript);
        assert_eq!(actions[1], HotkeyAction::OpenEditor);
        assert_eq!(actions[2], HotkeyAction::MoveBackwardChar);
        assert_eq!(actions[3], HotkeyAction::MoveForwardChar);
        assert_eq!(actions[4], HotkeyAction::MoveBeginningOfLine);
        assert_eq!(actions[5], HotkeyAction::MoveEndOfLine);
        assert_eq!(actions[6], HotkeyAction::MoveBackwardWord);
        assert_eq!(actions[7], HotkeyAction::MoveForwardWord);
        assert_eq!(actions[8], HotkeyAction::DeleteBackwardChar);
        assert_eq!(actions[9], HotkeyAction::DeleteForwardChar);
        assert_eq!(actions[10], HotkeyAction::DeleteBackwardWord);
        assert_eq!(actions[11], HotkeyAction::KillToEndOfLine);
        assert_eq!(actions[12], HotkeyAction::KillToBeginningOfLine);
        assert_eq!(actions[13], HotkeyAction::Yank);
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
        assert_eq!(bindings.len(), 14);
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

    // ========================================================================
    // Editing Hotkey Tests
    // ========================================================================

    #[test]
    fn test_editing_hotkey_action_display_names() {
        use pretty_assertions::assert_eq;
        assert_eq!(
            HotkeyAction::MoveBackwardChar.display_name(),
            "Move Backward Char"
        );
        assert_eq!(
            HotkeyAction::MoveForwardChar.display_name(),
            "Move Forward Char"
        );
        assert_eq!(
            HotkeyAction::MoveBeginningOfLine.display_name(),
            "Move to Line Start"
        );
        assert_eq!(
            HotkeyAction::MoveEndOfLine.display_name(),
            "Move to Line End"
        );
        assert_eq!(
            HotkeyAction::MoveBackwardWord.display_name(),
            "Move Backward Word"
        );
        assert_eq!(
            HotkeyAction::MoveForwardWord.display_name(),
            "Move Forward Word"
        );
        assert_eq!(
            HotkeyAction::DeleteBackwardChar.display_name(),
            "Delete Backward Char"
        );
        assert_eq!(
            HotkeyAction::DeleteForwardChar.display_name(),
            "Delete Forward Char"
        );
        assert_eq!(
            HotkeyAction::DeleteBackwardWord.display_name(),
            "Delete Backward Word"
        );
        assert_eq!(
            HotkeyAction::KillToEndOfLine.display_name(),
            "Kill to Line End"
        );
        assert_eq!(
            HotkeyAction::KillToBeginningOfLine.display_name(),
            "Kill to Line Start"
        );
        assert_eq!(HotkeyAction::Yank.display_name(), "Yank");
    }

    #[test]
    fn test_editing_hotkey_action_toml_keys() {
        use pretty_assertions::assert_eq;
        assert_eq!(
            HotkeyAction::MoveBackwardChar.toml_key(),
            "move_backward_char"
        );
        assert_eq!(
            HotkeyAction::MoveForwardChar.toml_key(),
            "move_forward_char"
        );
        assert_eq!(
            HotkeyAction::MoveBeginningOfLine.toml_key(),
            "move_beginning_of_line"
        );
        assert_eq!(HotkeyAction::MoveEndOfLine.toml_key(), "move_end_of_line");
        assert_eq!(
            HotkeyAction::MoveBackwardWord.toml_key(),
            "move_backward_word"
        );
        assert_eq!(
            HotkeyAction::MoveForwardWord.toml_key(),
            "move_forward_word"
        );
        assert_eq!(
            HotkeyAction::DeleteBackwardChar.toml_key(),
            "delete_backward_char"
        );
        assert_eq!(
            HotkeyAction::DeleteForwardChar.toml_key(),
            "delete_forward_char"
        );
        assert_eq!(
            HotkeyAction::DeleteBackwardWord.toml_key(),
            "delete_backward_word"
        );
        assert_eq!(
            HotkeyAction::KillToEndOfLine.toml_key(),
            "kill_to_end_of_line"
        );
        assert_eq!(
            HotkeyAction::KillToBeginningOfLine.toml_key(),
            "kill_to_beginning_of_line"
        );
        assert_eq!(HotkeyAction::Yank.toml_key(), "yank");
    }

    #[test]
    fn test_editing_hotkey_action_default_bindings() {
        use pretty_assertions::assert_eq;
        assert_eq!(HotkeyAction::MoveBackwardChar.default_binding(), "ctrl+b");
        assert_eq!(HotkeyAction::MoveForwardChar.default_binding(), "ctrl+f");
        assert_eq!(
            HotkeyAction::MoveBeginningOfLine.default_binding(),
            "ctrl+a"
        );
        assert_eq!(HotkeyAction::MoveEndOfLine.default_binding(), "ctrl+e");
        assert_eq!(HotkeyAction::MoveBackwardWord.default_binding(), "alt+b");
        assert_eq!(HotkeyAction::MoveForwardWord.default_binding(), "alt+f");
        assert_eq!(HotkeyAction::DeleteBackwardChar.default_binding(), "ctrl+h");
        assert_eq!(HotkeyAction::DeleteForwardChar.default_binding(), "ctrl+d");
        assert_eq!(HotkeyAction::DeleteBackwardWord.default_binding(), "ctrl+w");
        assert_eq!(HotkeyAction::KillToEndOfLine.default_binding(), "ctrl+k");
        assert_eq!(
            HotkeyAction::KillToBeginningOfLine.default_binding(),
            "ctrl+u"
        );
        assert_eq!(HotkeyAction::Yank.default_binding(), "ctrl+y");
    }

    #[test]
    fn test_hotkey_config_default_includes_editing_bindings() {
        use pretty_assertions::assert_eq;
        let config = HotkeyConfig::default();
        assert_eq!(config.move_backward_char, HotkeyBinding::from_str("ctrl+b"));
        assert_eq!(config.move_forward_char, HotkeyBinding::from_str("ctrl+f"));
        assert_eq!(
            config.move_beginning_of_line,
            HotkeyBinding::from_str("ctrl+a")
        );
        assert_eq!(config.move_end_of_line, HotkeyBinding::from_str("ctrl+e"));
        assert_eq!(config.move_backward_word, HotkeyBinding::from_str("alt+b"));
        assert_eq!(config.move_forward_word, HotkeyBinding::from_str("alt+f"));
        assert_eq!(
            config.delete_backward_char,
            HotkeyBinding::from_str("ctrl+h")
        );
        assert_eq!(
            config.delete_forward_char,
            HotkeyBinding::from_str("ctrl+d")
        );
        assert_eq!(
            config.delete_backward_word,
            HotkeyBinding::from_str("ctrl+w")
        );
        assert_eq!(
            config.kill_to_end_of_line,
            HotkeyBinding::from_str("ctrl+k")
        );
        assert_eq!(
            config.kill_to_beginning_of_line,
            HotkeyBinding::from_str("ctrl+u")
        );
        assert_eq!(config.yank, HotkeyBinding::from_str("ctrl+y"));
    }

    #[test]
    fn test_hotkey_config_from_toml_editing_overrides() {
        use pretty_assertions::assert_eq;
        let toml = HotkeyConfigToml {
            open_transcript: None,
            open_editor: None,
            move_backward_char: Some(HotkeyBinding::from_str("alt+left")),
            move_forward_char: Some(HotkeyBinding::from_str("alt+right")),
            move_beginning_of_line: None,
            move_end_of_line: None,
            move_backward_word: None,
            move_forward_word: None,
            delete_backward_char: None,
            delete_forward_char: None,
            delete_backward_word: None,
            kill_to_end_of_line: None,
            kill_to_beginning_of_line: None,
            yank: None,
        };
        let config = HotkeyConfig::from_toml(&toml);
        assert_eq!(
            config.move_backward_char,
            HotkeyBinding::from_str("alt+left")
        );
        assert_eq!(
            config.move_forward_char,
            HotkeyBinding::from_str("alt+right")
        );
        // Others should keep defaults
        assert_eq!(
            config.move_beginning_of_line,
            HotkeyBinding::from_str("ctrl+a")
        );
        assert_eq!(
            config.kill_to_end_of_line,
            HotkeyBinding::from_str("ctrl+k")
        );
    }

    #[test]
    fn test_hotkey_config_from_toml_editing_unbind() {
        use pretty_assertions::assert_eq;
        let toml = HotkeyConfigToml {
            open_transcript: None,
            open_editor: None,
            move_backward_char: Some(HotkeyBinding::none()),
            move_forward_char: None,
            move_beginning_of_line: None,
            move_end_of_line: None,
            move_backward_word: None,
            move_forward_word: None,
            delete_backward_char: None,
            delete_forward_char: None,
            delete_backward_word: None,
            kill_to_end_of_line: None,
            kill_to_beginning_of_line: None,
            yank: None,
        };
        let config = HotkeyConfig::from_toml(&toml);
        assert!(config.move_backward_char.is_none());
        // Others should keep defaults
        assert_eq!(config.move_forward_char, HotkeyBinding::from_str("ctrl+f"));
    }

    #[test]
    fn test_hotkey_config_all_bindings_includes_editing() {
        let config = HotkeyConfig::default();
        let bindings = config.all_bindings();
        assert_eq!(bindings.len(), 14);
        // First two are app-level actions
        assert_eq!(bindings[0].0, HotkeyAction::OpenTranscript);
        assert_eq!(bindings[1].0, HotkeyAction::OpenEditor);
        // Then editing actions
        assert_eq!(bindings[2].0, HotkeyAction::MoveBackwardChar);
        assert_eq!(bindings[13].0, HotkeyAction::Yank);
    }

    #[test]
    fn test_tui_config_toml_with_editing_hotkeys() {
        let config: TuiConfigToml = toml::from_str(
            r#"
[hotkeys]
move_backward_char = "alt+left"
kill_to_end_of_line = "none"
"#,
        )
        .unwrap();
        assert_eq!(
            config.hotkeys.move_backward_char,
            Some(HotkeyBinding::from_str("alt+left"))
        );
        assert_eq!(
            config.hotkeys.kill_to_end_of_line,
            Some(HotkeyBinding::none())
        );
        // Unset fields should be None
        assert!(config.hotkeys.move_forward_char.is_none());
    }

    #[test]
    fn test_hotkey_config_binding_for_editing_action() {
        use pretty_assertions::assert_eq;
        let config = HotkeyConfig::default();
        assert_eq!(
            config.binding_for(HotkeyAction::MoveBackwardChar),
            &HotkeyBinding::from_str("ctrl+b")
        );
        assert_eq!(
            config.binding_for(HotkeyAction::KillToEndOfLine),
            &HotkeyBinding::from_str("ctrl+k")
        );
        assert_eq!(
            config.binding_for(HotkeyAction::Yank),
            &HotkeyBinding::from_str("ctrl+y")
        );
    }

    // ========================================================================
    // Script Timeout Configuration Tests
    // ========================================================================

    #[test]
    fn test_script_timeout_parse_seconds() {
        let timeout = ScriptTimeout::from_str("30s");
        assert_eq!(timeout.as_duration(), Duration::from_secs(30));
    }

    #[test]
    fn test_script_timeout_parse_minutes() {
        let timeout = ScriptTimeout::from_str("2m");
        assert_eq!(timeout.as_duration(), Duration::from_secs(120));
    }

    #[test]
    fn test_script_timeout_parse_5m() {
        let timeout = ScriptTimeout::from_str("5m");
        assert_eq!(timeout.as_duration(), Duration::from_secs(300));
    }

    #[test]
    fn test_script_timeout_default_is_30s() {
        let timeout = ScriptTimeout::default();
        assert_eq!(timeout.as_duration(), Duration::from_secs(30));
    }

    #[test]
    fn test_script_timeout_display_name() {
        let timeout = ScriptTimeout::from_str("30s");
        assert_eq!(timeout.display_name(), "30s");

        let timeout = ScriptTimeout::from_str("2m");
        assert_eq!(timeout.display_name(), "2m");
    }

    #[test]
    fn test_script_timeout_toml_value() {
        let timeout = ScriptTimeout::from_str("30s");
        assert_eq!(timeout.toml_value(), "30s");
    }

    #[test]
    fn test_script_timeout_deserialize_from_toml() {
        #[derive(Deserialize)]
        struct Wrapper {
            timeout: ScriptTimeout,
        }

        let w: Wrapper = toml::from_str(r#"timeout = "30s""#).unwrap();
        assert_eq!(w.timeout.as_duration(), Duration::from_secs(30));

        let w: Wrapper = toml::from_str(r#"timeout = "2m""#).unwrap();
        assert_eq!(w.timeout.as_duration(), Duration::from_secs(120));
    }

    #[test]
    fn test_script_timeout_in_tui_config_toml() {
        let config: TuiConfigToml = toml::from_str(
            r#"
script_timeout = "45s"
"#,
        )
        .unwrap();
        assert!(config.script_timeout.is_some());
        assert_eq!(
            config.script_timeout.unwrap().as_duration(),
            Duration::from_secs(45)
        );
    }

    #[test]
    fn test_script_timeout_absent_from_tui_config_toml() {
        let config: TuiConfigToml = toml::from_str("").unwrap();
        assert!(config.script_timeout.is_none());
    }

    #[test]
    fn test_script_timeout_in_nori_config() {
        let config = NoriConfig::default();
        assert_eq!(config.script_timeout.as_duration(), Duration::from_secs(30));
    }

    #[test]
    fn test_full_config_toml_with_script_timeout() {
        let config: NoriConfigToml = toml::from_str(
            r#"
model = "claude-code"

[tui]
script_timeout = "2m"
"#,
        )
        .unwrap();
        assert!(config.tui.script_timeout.is_some());
        assert_eq!(
            config.tui.script_timeout.unwrap().as_duration(),
            Duration::from_secs(120)
        );
    }

    // ========================================================================
    // Footer Segment Configuration Tests
    // ========================================================================

    #[test]
    fn test_footer_segment_deserialize_all_variants() {
        use pretty_assertions::assert_eq;
        #[derive(Deserialize)]
        struct Wrapper {
            segment: FooterSegment,
        }

        let w: Wrapper = toml::from_str(r#"segment = "prompt_summary""#).unwrap();
        assert_eq!(w.segment, FooterSegment::PromptSummary);

        let w: Wrapper = toml::from_str(r#"segment = "vim_mode""#).unwrap();
        assert_eq!(w.segment, FooterSegment::VimMode);

        let w: Wrapper = toml::from_str(r#"segment = "git_branch""#).unwrap();
        assert_eq!(w.segment, FooterSegment::GitBranch);

        let w: Wrapper = toml::from_str(r#"segment = "git_stats""#).unwrap();
        assert_eq!(w.segment, FooterSegment::GitStats);

        let w: Wrapper = toml::from_str(r#"segment = "context""#).unwrap();
        assert_eq!(w.segment, FooterSegment::Context);

        let w: Wrapper = toml::from_str(r#"segment = "approval_mode""#).unwrap();
        assert_eq!(w.segment, FooterSegment::ApprovalMode);

        let w: Wrapper = toml::from_str(r#"segment = "nori_profile""#).unwrap();
        assert_eq!(w.segment, FooterSegment::NoriProfile);

        let w: Wrapper = toml::from_str(r#"segment = "nori_version""#).unwrap();
        assert_eq!(w.segment, FooterSegment::NoriVersion);

        let w: Wrapper = toml::from_str(r#"segment = "token_usage""#).unwrap();
        assert_eq!(w.segment, FooterSegment::TokenUsage);
    }

    #[test]
    fn test_footer_segment_serialize() {
        use pretty_assertions::assert_eq;
        // TOML doesn't support bare enums, so we test within a struct
        #[derive(Serialize)]
        struct Wrapper {
            segment: FooterSegment,
        }
        let w = Wrapper {
            segment: FooterSegment::PromptSummary,
        };
        assert!(toml::to_string(&w).unwrap().contains("prompt_summary"));

        let w = Wrapper {
            segment: FooterSegment::GitBranch,
        };
        assert!(toml::to_string(&w).unwrap().contains("git_branch"));
    }

    #[test]
    fn test_footer_segment_display_name() {
        use pretty_assertions::assert_eq;
        assert_eq!(FooterSegment::PromptSummary.display_name(), "Task Summary");
        assert_eq!(FooterSegment::VimMode.display_name(), "Vim Mode");
        assert_eq!(FooterSegment::GitBranch.display_name(), "Git Branch");
        assert_eq!(FooterSegment::GitStats.display_name(), "Git Stats");
        assert_eq!(FooterSegment::Context.display_name(), "Context Window");
        assert_eq!(FooterSegment::ApprovalMode.display_name(), "Approval Mode");
        assert_eq!(FooterSegment::NoriProfile.display_name(), "Skillset");
        assert_eq!(
            FooterSegment::NoriVersion.display_name(),
            "Skillset Version"
        );
        assert_eq!(FooterSegment::TokenUsage.display_name(), "Token Usage");
    }

    #[test]
    fn test_footer_segment_all_variants() {
        let variants = FooterSegment::all_variants();
        assert_eq!(variants.len(), 9);
        assert_eq!(variants[0], FooterSegment::PromptSummary);
        assert_eq!(variants[1], FooterSegment::VimMode);
        assert_eq!(variants[2], FooterSegment::GitBranch);
        assert_eq!(variants[3], FooterSegment::GitStats);
        assert_eq!(variants[4], FooterSegment::Context);
        assert_eq!(variants[5], FooterSegment::ApprovalMode);
        assert_eq!(variants[6], FooterSegment::NoriProfile);
        assert_eq!(variants[7], FooterSegment::NoriVersion);
        assert_eq!(variants[8], FooterSegment::TokenUsage);
    }

    #[test]
    fn test_footer_segment_default_order() {
        use pretty_assertions::assert_eq;
        let order = FooterSegment::default_order();
        assert_eq!(order, FooterSegment::all_variants());
    }

    #[test]
    fn test_footer_segment_config_default_all_enabled() {
        let config = FooterSegmentConfig::default();
        for segment in FooterSegment::all_variants() {
            assert!(
                config.is_enabled(*segment),
                "Segment {:?} should be enabled by default",
                segment
            );
        }
    }

    #[test]
    fn test_footer_segment_config_disable_segment() {
        let mut config = FooterSegmentConfig::default();
        config.set_enabled(FooterSegment::GitBranch, false);
        assert!(!config.is_enabled(FooterSegment::GitBranch));
        assert!(config.is_enabled(FooterSegment::Context));
    }

    #[test]
    fn test_footer_segment_config_from_toml_empty() {
        let toml = FooterSegmentConfigToml::default();
        let config = FooterSegmentConfig::from_toml(&toml);
        // All segments enabled by default
        for segment in FooterSegment::all_variants() {
            assert!(config.is_enabled(*segment));
        }
    }

    #[test]
    fn test_footer_segment_config_from_toml_some_disabled() {
        let toml = FooterSegmentConfigToml {
            prompt_summary: Some(false),
            git_branch: Some(false),
            token_usage: Some(false),
            ..Default::default()
        };
        let config = FooterSegmentConfig::from_toml(&toml);
        assert!(!config.is_enabled(FooterSegment::PromptSummary));
        assert!(!config.is_enabled(FooterSegment::GitBranch));
        assert!(!config.is_enabled(FooterSegment::TokenUsage));
        assert!(config.is_enabled(FooterSegment::Context));
        assert!(config.is_enabled(FooterSegment::ApprovalMode));
    }

    #[test]
    fn test_tui_config_toml_with_footer_segments() {
        let config: TuiConfigToml = toml::from_str(
            r#"
[footer_segments]
git_branch = false
token_usage = false
"#,
        )
        .unwrap();
        assert_eq!(config.footer_segments.git_branch, Some(false));
        assert_eq!(config.footer_segments.token_usage, Some(false));
        assert_eq!(config.footer_segments.context, None);
    }

    #[test]
    fn test_full_config_toml_with_footer_segments() {
        let config: NoriConfigToml = toml::from_str(
            r#"
model = "claude-code"

[tui]
vertical_footer = true

[tui.footer_segments]
prompt_summary = false
vim_mode = false
nori_profile = true
"#,
        )
        .unwrap();
        assert_eq!(config.tui.footer_segments.prompt_summary, Some(false));
        assert_eq!(config.tui.footer_segments.vim_mode, Some(false));
        assert_eq!(config.tui.footer_segments.nori_profile, Some(true));
        assert_eq!(config.tui.footer_segments.git_branch, None);
    }
}
