//! Nori-branded session header component for the TUI.
//!
//! This module provides the Nori session header that appears at the start
//! of every session, displaying the Nori title, version info,
//! agent details, and Nori profile information.
//!
//! The session header uses a simple "Nori" text title (the ASCII art banner
//! is reserved for the first-launch welcome screen).

use crate::exec_command::relativize_to_home;
use crate::history_cell::CompositeHistoryCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::PlainHistoryCell;
use crate::history_cell::SessionInfoCell;
use crate::history_cell::card_inner_width;
use crate::history_cell::with_border;
use crate::nori::token_count::TokenCount;
use crate::nori::token_count::count_tokens;
use crate::nori::token_count::format_token_count;
use crate::version::CODEX_CLI_VERSION;
use codex_acp::TranscriptTokenUsage;
use codex_core::config::Config;
use codex_core::protocol::SessionConfiguredEvent;
use codex_protocol::num_format::format_si_suffix;
use ratatui::prelude::*;
use ratatui::style::Stylize;
use std::path::Path;
use std::path::PathBuf;
use unicode_width::UnicodeWidthStr;

/// Maximum inner width for the Nori session header card.
const NORI_HEADER_MAX_INNER_WIDTH: usize = 60;

/// Simple enum to identify agent type for instruction file activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKindSimple {
    Claude,
    Codex,
    Gemini,
}

/// Represents an instruction file with its activation status.
#[derive(Debug, Clone)]
pub struct InstructionFile {
    /// Path to the instruction file.
    pub path: PathBuf,
    /// Whether this file is active for the current agent.
    pub active: bool,
    /// Token count for the file (only computed for active files).
    pub token_count: Option<TokenCount>,
}

/// Detect agent kind from a model/agent string.
///
/// Returns `Some(AgentKindSimple)` if the string matches a known agent pattern,
/// or `None` if unknown.
fn detect_agent_kind(agent: &str) -> Option<AgentKindSimple> {
    let lower = agent.to_lowercase();
    if lower.starts_with("claude") {
        Some(AgentKindSimple::Claude)
    } else if lower.starts_with("codex") {
        Some(AgentKindSimple::Codex)
    } else if lower.starts_with("gemini") {
        Some(AgentKindSimple::Gemini)
    } else {
        None
    }
}

/// Discover ALL instruction files in the directory hierarchy and mark them as active/inactive
/// based on the current agent's activation algorithm.
///
/// Files are discovered from git root (or cwd if no git root) to cwd, plus user-level configs.
/// The activation algorithm varies by agent:
/// - Claude: activates .claude/CLAUDE.md, CLAUDE.md, CLAUDE.local.md (all can be active per dir)
/// - Codex: activates AGENTS.override.md OR AGENTS.md per dir (preferring override)
/// - Gemini: activates only GEMINI.md per dir (no hidden variants, no overrides)
fn discover_all_instruction_files(
    cwd: &Path,
    agent_kind: Option<AgentKindSimple>,
) -> Vec<InstructionFile> {
    // In debug builds, allow E2E tests to mock instruction files for consistent snapshots.
    // This returns a constant list to ensure banner width is consistent across machines.
    #[cfg(debug_assertions)]
    if std::env::var("NORI_MOCK_INSTRUCTION_FILES").is_ok() {
        return vec![InstructionFile {
            path: std::path::PathBuf::from("~/.claude/CLAUDE.md"),
            active: true,
            token_count: None,
        }];
    }

    discover_all_instruction_files_with_home(cwd, agent_kind, dirs::home_dir().as_deref())
}

/// Internal function that discovers instruction files with an optional custom home directory.
/// This allows testing with a fake home directory.
fn discover_all_instruction_files_with_home(
    cwd: &Path,
    agent_kind: Option<AgentKindSimple>,
    home_dir: Option<&Path>,
) -> Vec<InstructionFile> {
    // Build chain from cwd upwards and detect git root
    let mut chain: Vec<PathBuf> = Vec::new();
    let mut current = cwd.to_path_buf();
    let mut git_root: Option<PathBuf> = None;

    loop {
        chain.push(current.clone());

        // Check for .git marker
        let git_marker = current.join(".git");
        if git_marker.exists() {
            git_root = Some(current.clone());
            break;
        }

        if !current.pop() {
            break;
        }
    }

    // Determine search directories (from git root to cwd, or just cwd if no git root)
    let search_dirs: Vec<PathBuf> = if let Some(root) = &git_root {
        // Reverse the chain and filter to only include from git root onward
        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut saw_root = false;
        for p in chain.iter().rev() {
            if !saw_root {
                if p == root {
                    saw_root = true;
                } else {
                    continue;
                }
            }
            dirs.push(p.clone());
        }
        dirs
    } else {
        // No git root, just search cwd
        vec![cwd.to_path_buf()]
    };

    let mut found: Vec<InstructionFile> = Vec::new();

    // Track which directories have override files (for Codex algorithm)
    let mut dirs_with_override: std::collections::HashSet<PathBuf> =
        std::collections::HashSet::new();

    // First pass: discover all files and detect overrides
    let mut discovered: Vec<(PathBuf, PathBuf)> = Vec::new(); // (file_path, parent_dir)

    for dir in &search_dirs {
        // Check for all instruction file types in this directory
        let candidates = [
            ("CLAUDE.md", true),
            ("CLAUDE.local.md", true),
            ("AGENTS.md", true),
            ("AGENTS.override.md", true),
            ("GEMINI.md", true),
        ];

        for (filename, _) in candidates {
            let file_path = dir.join(filename);
            if file_path.is_file() {
                if filename == "AGENTS.override.md" {
                    dirs_with_override.insert(dir.clone());
                }
                discovered.push((file_path, dir.clone()));
            }
        }

        // Check hidden .claude directory
        let claude_dir = dir.join(".claude");
        if claude_dir.is_dir() {
            let hidden_claude = claude_dir.join("CLAUDE.md");
            if hidden_claude.is_file() {
                discovered.push((hidden_claude, dir.clone()));
            }
        }
    }

    // Discover home directory config files
    // These are user-level configs that apply globally:
    // - ~/.claude/CLAUDE.md for Claude
    // - ~/.codex/AGENTS.md for Codex
    // - ~/.gemini/GEMINI.md for Gemini
    let mut home_configs: Vec<(PathBuf, PathBuf)> = Vec::new();
    if let Some(home) = home_dir {
        // Check for Claude home config: ~/.claude/CLAUDE.md
        let claude_home = home.join(".claude").join("CLAUDE.md");
        if claude_home.is_file() {
            home_configs.push((claude_home, home.join(".claude")));
        }

        // Check for Codex home config: ~/.codex/AGENTS.md
        let codex_home = home.join(".codex").join("AGENTS.md");
        if codex_home.is_file() {
            home_configs.push((codex_home, home.join(".codex")));
        }

        // Check for Gemini home config: ~/.gemini/GEMINI.md
        let gemini_home = home.join(".gemini").join("GEMINI.md");
        if gemini_home.is_file() {
            home_configs.push((gemini_home, home.join(".gemini")));
        }
    }

    // Prepend home configs to discovered list so they appear first
    let mut all_discovered = home_configs;
    all_discovered.extend(discovered);

    // Second pass: apply activation algorithm
    for (file_path, parent_dir) in all_discovered {
        let filename = file_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        let is_hidden_claude = file_path.to_string_lossy().contains(".claude/CLAUDE.md");

        let active = match agent_kind {
            Some(AgentKindSimple::Claude) => {
                // Claude activates: .claude/CLAUDE.md, CLAUDE.md, CLAUDE.local.md
                is_hidden_claude || filename == "CLAUDE.md" || filename == "CLAUDE.local.md"
            }
            Some(AgentKindSimple::Codex) => {
                // Codex activates: AGENTS.override.md OR AGENTS.md (prefer override)
                if filename == "AGENTS.override.md" {
                    true
                } else if filename == "AGENTS.md" {
                    // Only active if no override exists in this directory
                    !dirs_with_override.contains(&parent_dir)
                } else {
                    false
                }
            }
            Some(AgentKindSimple::Gemini) => {
                // Gemini activates: only GEMINI.md (no hidden, no overrides)
                filename == "GEMINI.md"
            }
            None => {
                // Unknown agent: nothing is active
                false
            }
        };

        let token_count = if active {
            std::fs::read_to_string(&file_path)
                .ok()
                .map(|contents| count_tokens(&contents, agent_kind))
        } else {
            None
        };

        found.push(InstructionFile {
            path: file_path,
            active,
            token_count,
        });
    }

    found
}

/// Read the current Nori profile by searching for .nori-config.json in ancestors.
///
/// Walks from the given directory upward through parent directories, returning
/// the profile from the nearest ancestor containing a .nori-config.json file.
fn read_nori_profile(cwd: &Path) -> Option<String> {
    let mut current_dir = cwd.to_path_buf();

    loop {
        let config_path = current_dir.join(".nori-config.json");
        if config_path.exists()
            && let Ok(contents) = std::fs::read_to_string(&config_path)
            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents)
        {
            // Try new format: activeSkillset
            if let Some(profile) = json.get("activeSkillset").and_then(|v| v.as_str()) {
                return Some(profile.to_string());
            }
            // Fall back to: agents.claude-code.profile.baseProfile
            if let Some(profile) = json
                .get("agents")
                .and_then(|a| a.get("claude-code"))
                .and_then(|c| c.get("profile"))
                .and_then(|p| p.get("baseProfile"))
                .and_then(|b| b.as_str())
            {
                return Some(profile.to_string());
            }
            // Fall back to oldest format: profile.baseProfile
            if let Some(profile) = json
                .get("profile")
                .and_then(|p| p.get("baseProfile"))
                .and_then(|b| b.as_str())
            {
                return Some(profile.to_string());
            }
        }

        // Move to parent directory
        if !current_dir.pop() {
            break;
        }
    }

    None
}

/// Check if either nori-skillsets or nori-ai command is available in PATH.
/// Prefers nori-skillsets (new installer) over nori-ai (legacy installer).
fn is_nori_installed() -> bool {
    which::which("nori-skillsets").is_ok() || which::which("nori-ai").is_ok()
}

/// Format a directory path for display, relativizing to home if possible.
fn format_directory(directory: &Path, max_width: Option<usize>) -> String {
    let formatted = if let Some(rel) = relativize_to_home(directory) {
        if rel.as_os_str().is_empty() {
            "~".to_string()
        } else {
            format!("~{}{}", std::path::MAIN_SEPARATOR, rel.display())
        }
    } else {
        directory.display().to_string()
    };

    if let Some(max_width) = max_width {
        if max_width == 0 {
            return String::new();
        }
        if UnicodeWidthStr::width(formatted.as_str()) > max_width {
            return crate::text_formatting::center_truncate_path(&formatted, max_width);
        }
    }

    formatted
}

/// Controls how much detail the instruction files section shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisplayMode {
    /// Session header at start: only active files listed (no per-file token counts),
    /// with just the total token count at the bottom.
    Compact,
    /// /status command: all files listed (inactive shown dim), per-file token counts
    /// for active files, and total at the bottom.
    Full,
}

/// The Nori-branded session header cell.
#[derive(Debug)]
pub(crate) struct NoriSessionHeaderCell {
    version: &'static str,
    agent: String,
    directory: PathBuf,
    nori_profile: Option<String>,
    instruction_files: Vec<InstructionFile>,
    display_mode: DisplayMode,
    /// Optional task summary (first prompt summary).
    prompt_summary: Option<String>,
    /// Optional approval mode label (e.g., "Agent", "Read Only", "Full Access").
    approval_mode_label: Option<String>,
    /// Optional token usage breakdown from transcript.
    token_breakdown: Option<TranscriptTokenUsage>,
    /// Optional context window percentage (0-100).
    context_window_percent: Option<i64>,
}

/// Maximum length for task summary in status card.
const MAX_TASK_SUMMARY_LENGTH: usize = 50;

impl NoriSessionHeaderCell {
    pub(crate) fn new(agent: String, directory: PathBuf) -> Self {
        let nori_profile = read_nori_profile(&directory);
        let agent_kind = detect_agent_kind(&agent);
        let instruction_files = discover_all_instruction_files(&directory, agent_kind);
        Self {
            version: CODEX_CLI_VERSION,
            agent,
            directory,
            nori_profile,
            instruction_files,
            display_mode: DisplayMode::Full,
            prompt_summary: None,
            approval_mode_label: None,
            token_breakdown: None,
            context_window_percent: None,
        }
    }

    pub(crate) fn with_display_mode(mut self, mode: DisplayMode) -> Self {
        self.display_mode = mode;
        self
    }

    /// Create a new header cell with optional status card fields.
    pub(crate) fn new_with_status_info(
        agent: String,
        directory: PathBuf,
        prompt_summary: Option<String>,
        approval_mode_label: Option<String>,
        token_breakdown: Option<TranscriptTokenUsage>,
        context_window_percent: Option<i64>,
    ) -> Self {
        let nori_profile = read_nori_profile(&directory);
        let agent_kind = detect_agent_kind(&agent);
        let instruction_files = discover_all_instruction_files(&directory, agent_kind);
        Self {
            version: CODEX_CLI_VERSION,
            agent,
            directory,
            nori_profile,
            instruction_files,
            display_mode: DisplayMode::Full,
            prompt_summary,
            approval_mode_label,
            token_breakdown,
            context_window_percent,
        }
    }
}

impl HistoryCell for NoriSessionHeaderCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let Some(inner_width) = card_inner_width(width, NORI_HEADER_MAX_INNER_WIDTH) else {
            return Vec::new();
        };

        let mut lines: Vec<Line<'static>> = Vec::new();

        // Simple "Nori" title (ASCII art is reserved for the first-launch welcome screen)
        lines.push(Line::from(vec![
            Span::from("Nori CLI").green().bold(),
            Span::from(format!(" v{}", self.version)).dim(),
        ]));

        // Empty line after title
        lines.push(Line::from(""));

        // Task summary line (if provided) - truncated to one line
        if let Some(summary) = &self.prompt_summary {
            let truncated = truncate_summary(summary, MAX_TASK_SUMMARY_LENGTH);
            lines.push(Line::from(vec![
                Span::from("Task: ").dim(),
                Span::from(truncated).dim(),
            ]));
            lines.push(Line::from(""));
        }

        // Directory line
        let dir_max_width = inner_width.saturating_sub(11); // "directory: " is 11 chars
        let dir = format_directory(&self.directory, Some(dir_max_width));
        lines.push(Line::from(vec![
            Span::from("directory: ").dim(),
            Span::from(dir),
        ]));

        // Agent line
        lines.push(Line::from(vec![
            Span::from("agent:     ").dim(),
            Span::from(self.agent.clone()),
        ]));

        // Profile line
        let profile_display = self
            .nori_profile
            .clone()
            .unwrap_or_else(|| "(none)".to_string());
        lines.push(Line::from(vec![
            Span::from("skillset:  ").dim(),
            Span::from(profile_display),
        ]));

        // Approval mode line (if provided)
        if let Some(approval_mode) = &self.approval_mode_label {
            lines.push(Line::from(vec![
                Span::from("approvals: ").dim(),
                Span::from(approval_mode.clone()).magenta(),
            ]));
        }

        // Instruction Files section
        if !self.instruction_files.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::from("Instruction Files").bold()));

            let mut total_count: i64 = 0;
            let mut any_approximate = false;

            for file in &self.instruction_files {
                if file.active {
                    if let Some(tc) = &file.token_count {
                        total_count += tc.count;
                        if tc.approximate {
                            any_approximate = true;
                        }
                        if self.display_mode == DisplayMode::Full {
                            let tc_str = format_token_count(tc);
                            // 2 for leading indent + 2 for gap between path and token count
                            let path_budget = inner_width.saturating_sub(2 + 2 + tc_str.width());
                            let path_str = format_directory(&file.path, Some(path_budget));
                            let path_width = path_str.width();
                            let gap = inner_width.saturating_sub(2 + path_width + tc_str.width());
                            let padding = " ".repeat(gap);
                            lines.push(Line::from(vec![
                                Span::from(format!("  {path_str}{padding}")),
                                Span::from(tc_str).dim(),
                            ]));
                        } else {
                            let path_str =
                                format_directory(&file.path, Some(inner_width.saturating_sub(2)));
                            lines.push(Line::from(format!("  {path_str}")));
                        }
                    } else {
                        let path_str =
                            format_directory(&file.path, Some(inner_width.saturating_sub(2)));
                        lines.push(Line::from(format!("  {path_str}")));
                    }
                } else if self.display_mode == DisplayMode::Full {
                    let path_str =
                        format_directory(&file.path, Some(inner_width.saturating_sub(2)));
                    lines.push(Line::from(Span::from(format!("  {path_str}")).dim()));
                }
            }

            // Total line for active files
            if total_count > 0 {
                let total_tc = TokenCount {
                    count: total_count,
                    approximate: any_approximate,
                };
                let total_str = format_token_count(&total_tc);
                let label = "  total";
                let gap = inner_width.saturating_sub(label.width() + total_str.width());
                let padding = " ".repeat(gap);
                lines.push(Line::from(vec![
                    Span::from(format!("{label}{padding}")).dim(),
                    Span::from(total_str).dim(),
                ]));
            }
        }

        // Tokens section: show if we have token data or context window percentage
        let has_tokens = self.token_breakdown.as_ref().is_some_and(|t| t.total() > 0);
        let has_context = self.context_window_percent.is_some();

        if has_tokens || has_context {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::from("Tokens").bold()));

            // Context window line
            if let Some(pct) = self.context_window_percent {
                if let Some(token_breakdown) = &self.token_breakdown {
                    let context_tokens = token_breakdown
                        .input_tokens
                        .saturating_add(token_breakdown.cached_tokens);
                    let context_fmt = format_si_suffix(context_tokens);
                    lines.push(Line::from(vec![
                        Span::from("  Context: ").dim(),
                        Span::from(format!("{context_fmt} ({pct}%)")),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::from("  Context: ").dim(),
                        Span::from(format!("{pct}%")),
                    ]));
                }
            }

            // Total tokens line (only if we have token data)
            if let Some(token_breakdown) = &self.token_breakdown {
                let total = token_breakdown.total();
                if total > 0 {
                    let total_fmt = format_si_suffix(total);
                    let mut token_spans = vec![
                        Span::from("  Tokens: ").dim(),
                        Span::from(format!("{total_fmt} total")).dim(),
                    ];

                    if token_breakdown.cached_tokens > 0 {
                        let cached_fmt = format_si_suffix(token_breakdown.cached_tokens);
                        token_spans.push(Span::from(format!(" ({cached_fmt} cached)")).dim());
                    }

                    lines.push(Line::from(token_spans));
                }
            }
        }

        with_border(lines)
    }
}

/// Truncate a summary string to fit on one line.
fn truncate_summary(summary: &str, max_len: usize) -> String {
    if summary.chars().count() <= max_len {
        summary.to_string()
    } else {
        let truncated_chars = max_len.saturating_sub(3);
        let truncated: String = summary.chars().take(truncated_chars).collect();
        format!("{truncated}...")
    }
}

/// Create the Nori status output cell for the /status command.
///
/// This displays a simplified version of the session header showing:
/// - The /status command echo
/// - Nori branding with version
/// - Directory, agent, and profile info
/// - Optional: task summary, approval mode, token usage
pub(crate) fn new_nori_status_output(
    agent: &str,
    directory: PathBuf,
    prompt_summary: Option<String>,
    approval_mode_label: Option<String>,
    token_breakdown: Option<TranscriptTokenUsage>,
    context_window_percent: Option<i64>,
) -> CompositeHistoryCell {
    let command = PlainHistoryCell::new(vec!["/status".magenta().into()]);
    let header = NoriSessionHeaderCell::new_with_status_info(
        agent.to_string(),
        directory,
        prompt_summary,
        approval_mode_label,
        token_breakdown,
        context_window_percent,
    );

    CompositeHistoryCell::new(vec![Box::new(command), Box::new(header)])
}

/// Create the Nori session info cell to be displayed at session start.
pub(crate) fn new_nori_session_info(
    config: &Config,
    event: SessionConfiguredEvent,
    is_first_event: bool,
) -> SessionInfoCell {
    let SessionConfiguredEvent { model, .. } = event;

    SessionInfoCell::new(if is_first_event {
        // Header box rendered as history (so it appears at the very top)
        let header = NoriSessionHeaderCell::new(model, config.cwd.clone())
            .with_display_mode(DisplayMode::Compact);

        // Help lines below the header
        let mut help_lines: Vec<Line<'static>> = vec![];

        // Only show install hint if nori-ai is not already installed
        if !is_nori_installed() {
            help_lines.push(Line::from(""));
            help_lines.push(Line::from(vec![
                "  Run '".dim(),
                "npx nori-skillsets init".cyan(),
                "' to set up Nori AI enhancements".dim(),
            ]));
        }

        CompositeHistoryCell::new(vec![
            Box::new(header),
            Box::new(PlainHistoryCell::new(help_lines)),
        ])
    } else if config.model == model {
        CompositeHistoryCell::new(vec![])
    } else {
        let lines = vec![
            "model changed:".magenta().bold().into(),
            format!("requested: {}", config.model).into(),
            format!("used: {model}").into(),
        ];
        CompositeHistoryCell::new(vec![Box::new(PlainHistoryCell::new(lines))])
    })
}

#[cfg(test)]
mod tests;
