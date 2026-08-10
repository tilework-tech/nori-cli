//! Nori-branded session header component for the TUI.
//!
//! This module provides the Nori session header that appears at the start
//! of every session, displaying the Nori title, version info,
//! agent details, and active skillset information.
//!
//! The session header uses a simple "Nori" text title (the ASCII art banner
//! is reserved for the first-launch welcome screen).

use crate::exec_command::relativize_to_home;
use crate::git_marker::is_git_marker;
use crate::history_cell::CompositeHistoryCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::PlainHistoryCell;
use crate::history_cell::SessionInfoCell;
use crate::history_cell::card_inner_width;
use crate::history_cell::with_border;
use crate::nori::token_count::TokenCount;
use crate::nori::token_count::count_tokens;
use crate::nori::token_count::format_token_count;
use crate::system_info::NoriVersionSource;
use crate::system_info::read_active_skillset;
use crate::ui_types::format_si_suffix;
use crate::version::CODEX_CLI_VERSION;
use nori_config::NoriConfig;
use nori_harness::ConversationId;
use nori_harness::TranscriptTokenUsage;
use ratatui::prelude::*;
use ratatui::style::Stylize;
use std::path::Path;
use std::path::PathBuf;
use unicode_width::UnicodeWidthStr;

mod status_card;
use status_card::STATUS_LABEL_WIDTH;
pub(crate) use status_card::StatusCardInfo;
use status_card::context_value;
use status_card::git_row_spans;
use status_card::status_row;

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

    discover_all_instruction_files_with_paths(
        cwd,
        agent_kind,
        dirs::home_dir().as_deref(),
        default_managed_policy_dir().as_deref(),
    )
}

/// Default platform-specific directory containing the managed-policy CLAUDE.md
/// that Claude Code loads in addition to user/project files.
fn default_managed_policy_dir() -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        Some(PathBuf::from("/etc/claude-code"))
    } else if cfg!(target_os = "macos") {
        Some(PathBuf::from("/Library/Application Support/ClaudeCode"))
    } else if cfg!(target_os = "windows") {
        Some(PathBuf::from(r"C:\Program Files\ClaudeCode"))
    } else {
        None
    }
}

/// Internal function that discovers instruction files with an optional custom home directory.
/// Used by tests that want to inject a fake home directory but don't care about the
/// managed-policy path.
#[cfg(test)]
fn discover_all_instruction_files_with_home(
    cwd: &Path,
    agent_kind: Option<AgentKindSimple>,
    home_dir: Option<&Path>,
) -> Vec<InstructionFile> {
    discover_all_instruction_files_with_paths(cwd, agent_kind, home_dir, None)
}

/// Internal function that discovers instruction files with optional custom home and managed-policy
/// directories. Tests can inject paths to avoid touching real filesystem locations.
fn discover_all_instruction_files_with_paths(
    cwd: &Path,
    agent_kind: Option<AgentKindSimple>,
    home_dir: Option<&Path>,
    managed_policy_dir: Option<&Path>,
) -> Vec<InstructionFile> {
    // Build the full chain from cwd up to filesystem root, recording whether we
    // saw a `.git` marker and at which level.
    let mut chain: Vec<PathBuf> = Vec::new();
    let mut current = cwd.to_path_buf();
    let mut git_root: Option<PathBuf> = None;

    loop {
        chain.push(current.clone());

        if git_root.is_none() && is_git_marker(&current.join(".git")) {
            git_root = Some(current.clone());
        }

        if !current.pop() {
            break;
        }
    }

    // Choose the set of directories to search based on the agent's documented behavior.
    //
    // - Claude Code: walks all the way up to filesystem root, regardless of `.git`.
    //   See https://code.claude.com/docs/en/memory.
    // - Codex / Gemini: walk up only as far as the git root; if no git root exists,
    //   look at cwd only.
    // - Unknown agent: same as Codex/Gemini (conservative).
    let search_dirs: Vec<PathBuf> = match agent_kind {
        Some(AgentKindSimple::Claude) => chain.iter().rev().cloned().collect(),
        Some(AgentKindSimple::Codex) | Some(AgentKindSimple::Gemini) | None => {
            if let Some(root) = &git_root {
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
                vec![cwd.to_path_buf()]
            }
        }
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

    // Discover managed-policy CLAUDE.md (Claude Code only).
    // This is a system-level CLAUDE.md that the Claude Code agent loads in addition
    // to user/project files (e.g. /etc/claude-code/CLAUDE.md on Linux).
    let mut policy_configs: Vec<(PathBuf, PathBuf)> = Vec::new();
    if matches!(agent_kind, Some(AgentKindSimple::Claude))
        && let Some(policy_dir) = managed_policy_dir
    {
        let policy_file = policy_dir.join("CLAUDE.md");
        if policy_file.is_file() {
            policy_configs.push((policy_file, policy_dir.to_path_buf()));
        }
    }

    // Build the final list, lowest-precedence first: managed-policy, then home, then ancestor walk.
    let mut all_discovered = policy_configs;
    all_discovered.extend(home_configs);
    all_discovered.extend(discovered);

    // Deduplicate by path so a file discovered both via the ancestor walk and the
    // home-config / managed-policy passes appears exactly once.
    let mut seen_paths: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    all_discovered.retain(|(path, _)| seen_paths.insert(path.clone()));

    // Second pass: apply activation algorithm
    for (file_path, parent_dir) in all_discovered {
        let filename = file_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        let active = match agent_kind {
            Some(AgentKindSimple::Claude) => {
                // Claude activates: CLAUDE.md, CLAUDE.local.md (basename match covers
                // both `<dir>/CLAUDE.md` and `<dir>/.claude/CLAUDE.md`).
                filename == "CLAUDE.md" || filename == "CLAUDE.local.md"
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

/// Identity of the cloud session the TUI is attached to. The top-level cloud
/// launch path supplies this identity; ACP capabilities do not.
#[derive(Debug, Clone)]
pub(crate) struct CloudSessionInfo {
    /// The human-readable session id, e.g. `nori-fast-kazunoko-aac8`.
    pub id: String,
    /// The broker-reported session title, when known (e.g. "Fix login flakes").
    pub title: Option<String>,
}

/// The Nori-branded session header cell.
#[derive(Debug)]
pub(crate) struct NoriSessionHeaderCell {
    version: &'static str,
    agent: String,
    directory: PathBuf,
    skillset: Option<String>,
    instruction_files: Vec<InstructionFile>,
    display_mode: DisplayMode,
    /// Optional task summary (first prompt summary).
    prompt_summary: Option<String>,
    /// Optional approval mode label (e.g., "Agent", "Read Only", "Full Access").
    approval_mode_label: Option<String>,
    /// Optional token usage breakdown from transcript.
    token_breakdown: Option<TranscriptTokenUsage>,
    /// Footer-derived values (git, ACP mode, skillset version, context window)
    /// so the `/status` card is a superset of the footer's categories.
    status_info: StatusCardInfo,
    /// The local conversation id, rendered on the `session:` row for every
    /// agent. Absent on the session-start welcome card before it is assigned.
    conversation_id: Option<ConversationId>,
    /// The parent conversation id after a branch-at-head fork, rendered on the
    /// `forked from:` row so the previous (resumable) session stays visible.
    forked_from: Option<ConversationId>,
    /// Cloud session identity when attached through cloud mode.
    /// When present, the `session:` line appends the broker title; on the
    /// compact welcome card it also suppresses the misleading local `directory:`
    /// value (the cwd is on the remote VM, not local).
    cloud_session: Option<CloudSessionInfo>,
}

/// Maximum length for task summary in status card.
const MAX_TASK_SUMMARY_LENGTH: usize = 50;

impl NoriSessionHeaderCell {
    pub(crate) fn new(agent: String, directory: PathBuf) -> Self {
        let skillset = read_active_skillset(&directory);
        let agent_kind = detect_agent_kind(&agent);
        let instruction_files = discover_all_instruction_files(&directory, agent_kind);
        Self {
            version: CODEX_CLI_VERSION,
            agent,
            directory,
            skillset,
            instruction_files,
            display_mode: DisplayMode::Full,
            prompt_summary: None,
            approval_mode_label: None,
            token_breakdown: None,
            status_info: StatusCardInfo::default(),
            conversation_id: None,
            forked_from: None,
            cloud_session: None,
        }
    }

    pub(crate) fn with_display_mode(mut self, mode: DisplayMode) -> Self {
        self.display_mode = mode;
        self
    }

    pub(crate) fn with_cloud_session(mut self, cloud_session: Option<CloudSessionInfo>) -> Self {
        self.cloud_session = cloud_session;
        self
    }

    /// Create a new header cell with optional status card fields.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_with_status_info(
        agent: String,
        directory: PathBuf,
        prompt_summary: Option<String>,
        approval_mode_label: Option<String>,
        token_breakdown: Option<TranscriptTokenUsage>,
        cloud_session: Option<CloudSessionInfo>,
        conversation_id: Option<ConversationId>,
        forked_from: Option<ConversationId>,
        status_info: StatusCardInfo,
    ) -> Self {
        let skillset = read_active_skillset(&directory);
        let agent_kind = detect_agent_kind(&agent);
        let instruction_files = discover_all_instruction_files(&directory, agent_kind);
        Self {
            version: CODEX_CLI_VERSION,
            agent,
            directory,
            skillset,
            instruction_files,
            display_mode: DisplayMode::Full,
            prompt_summary,
            approval_mode_label,
            token_breakdown,
            status_info,
            conversation_id,
            forked_from,
            cloud_session,
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

        // Directory line. The `/status` (Full) card always shows the local cwd.
        // The compact welcome card suppresses it on a cloud session because the
        // cwd lives on the remote VM and would be misleading.
        let show_directory =
            matches!(self.display_mode, DisplayMode::Full) || self.cloud_session.is_none();
        if show_directory {
            let dir_max_width = inner_width.saturating_sub(STATUS_LABEL_WIDTH);
            let dir = format_directory(&self.directory, Some(dir_max_width));
            lines.push(status_row("directory:", vec![Span::from(dir)]));
        }

        // Session line: the local conversation id (or the cloud id when the
        // conversation id is not yet known), with the broker title in parens on
        // a cloud session.
        let session_base = match (self.conversation_id, &self.cloud_session) {
            (Some(id), _) => Some(id.to_string()),
            (None, Some(cloud)) => Some(cloud.id.clone()),
            (None, None) => None,
        };
        if let Some(base) = session_base {
            let session_display = match self.cloud_session.as_ref().and_then(|c| c.title.as_ref()) {
                Some(title) => format!("{base} ({title})"),
                None => base,
            };
            lines.push(status_row("session:", vec![Span::from(session_display)]));
        }

        // Forked-from line: the parent conversation after a branch-at-head fork,
        // which stays resumable via `nori resume <id>`.
        if let Some(forked_from) = self.forked_from {
            lines.push(status_row(
                "forked from:",
                vec![Span::from(forked_from.to_string())],
            ));
        }

        // Agent line
        lines.push(status_row("agent:", vec![Span::from(self.agent.clone())]));

        // Skillset line, with the detected skillsets version appended when known.
        let skillset_display = match &self.skillset {
            Some(name) => match &self.status_info.nori_version {
                Some(version) => {
                    let label = self
                        .status_info
                        .nori_version_source
                        .map(NoriVersionSource::label)
                        .unwrap_or("Skillsets");
                    format!("{name} ({label} v{version})")
                }
                None => name.clone(),
            },
            None => "(none)".to_string(),
        };
        lines.push(status_row("skillset:", vec![Span::from(skillset_display)]));

        // Approval mode line (if provided)
        if let Some(approval_mode) = &self.approval_mode_label {
            lines.push(status_row(
                "approvals:",
                vec![Span::from(approval_mode.clone()).magenta()],
            ));
        }

        // ACP mode line (plan/build) when the agent exposes a mode.
        if let Some(mode) = &self.status_info.acp_mode_label {
            lines.push(status_row("mode:", vec![Span::from(mode.clone())]));
        }

        // Git line: branch, worktree, stats, and untracked marker on one row.
        if let Some(git_spans) = git_row_spans(&self.status_info) {
            lines.push(status_row("git:", git_spans));
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
        let has_context = self.status_info.context_window_percent.is_some();

        if has_tokens || has_context {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::from("Tokens").bold()));

            // Consolidated, codex-style context window line.
            if let Some(context) = context_value(&self.status_info) {
                lines.push(Line::from(vec![
                    Span::from("  Context: ").dim(),
                    Span::from(context),
                ]));
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

/// Format an instruction file path for display, relativizing to the home directory.
pub(crate) fn format_instruction_path(path: &Path) -> String {
    format_directory(path, None)
}

/// Discover and return all active instruction files for the given agent and directory.
///
/// Each returned tuple contains the file path and its contents.
/// Files that cannot be read are silently skipped.
pub(crate) fn active_instruction_file_contents(agent: &str, cwd: &Path) -> Vec<(PathBuf, String)> {
    let agent_kind = detect_agent_kind(agent);
    let files = discover_all_instruction_files(cwd, agent_kind);
    files
        .into_iter()
        .filter(|f| f.active)
        .filter_map(|f| {
            std::fs::read_to_string(&f.path)
                .ok()
                .map(|contents| (f.path, contents))
        })
        .collect()
}

/// Create the Nori status output cell for the /status command.
///
/// This displays a simplified version of the session header showing:
/// - The /status command echo
/// - Nori branding with version
/// - Directory, agent, and skillset info
/// - Optional: task summary, approval mode, token usage
#[allow(clippy::too_many_arguments)]
pub(crate) fn new_nori_status_output(
    agent: &str,
    directory: PathBuf,
    prompt_summary: Option<String>,
    approval_mode_label: Option<String>,
    token_breakdown: Option<TranscriptTokenUsage>,
    cloud_session: Option<CloudSessionInfo>,
    conversation_id: Option<ConversationId>,
    forked_from: Option<ConversationId>,
    status_info: StatusCardInfo,
) -> CompositeHistoryCell {
    let command = PlainHistoryCell::new(vec!["/status".magenta().into()]);
    let header = NoriSessionHeaderCell::new_with_status_info(
        agent.to_string(),
        directory,
        prompt_summary,
        approval_mode_label,
        token_breakdown,
        cloud_session,
        conversation_id,
        forked_from,
        status_info,
    );

    CompositeHistoryCell::new(vec![Box::new(command), Box::new(header)])
}

/// Create the Nori session info cell to be displayed at session start.
/// `cloud_session` carries the identity supplied by cloud mode; the welcome
/// card then shows it in place of the local cwd.
pub(crate) fn new_nori_session_info(
    config: &NoriConfig,
    model: String,
    is_first_event: bool,
    cloud_session: Option<CloudSessionInfo>,
) -> SessionInfoCell {
    SessionInfoCell::new(if is_first_event {
        // Header box rendered as history (so it appears at the very top)
        let header = NoriSessionHeaderCell::new(model, config.cwd.clone())
            .with_display_mode(DisplayMode::Compact)
            .with_cloud_session(cloud_session);

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
    } else if config.active_agent == model {
        CompositeHistoryCell::new(vec![])
    } else {
        let lines = vec![
            "model changed:".magenta().bold().into(),
            format!("requested: {}", config.active_agent).into(),
            format!("used: {model}").into(),
        ];
        CompositeHistoryCell::new(vec![Box::new(PlainHistoryCell::new(lines))])
    })
}

#[cfg(test)]
mod tests;
