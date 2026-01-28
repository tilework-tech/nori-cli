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
use crate::version::CODEX_CLI_VERSION;
use codex_core::config::Config;
use codex_core::protocol::SessionConfiguredEvent;
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

        found.push(InstructionFile {
            path: file_path,
            active,
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
            // Try new format: agents.claude-code.profile.baseProfile
            if let Some(profile) = json
                .get("agents")
                .and_then(|a| a.get("claude-code"))
                .and_then(|c| c.get("profile"))
                .and_then(|p| p.get("baseProfile"))
                .and_then(|b| b.as_str())
            {
                return Some(profile.to_string());
            }
            // Try old format: profile.baseProfile
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

/// The Nori-branded session header cell.
#[derive(Debug)]
pub(crate) struct NoriSessionHeaderCell {
    version: &'static str,
    agent: String,
    directory: PathBuf,
    nori_profile: Option<String>,
    instruction_files: Vec<InstructionFile>,
}

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

        // Instruction Files section
        if !self.instruction_files.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::from("Instruction Files").bold()));

            for file in &self.instruction_files {
                let path_str = format_directory(&file.path, Some(inner_width.saturating_sub(2)));
                let span = if file.active {
                    Span::from(format!("  {path_str}"))
                } else {
                    Span::from(format!("  {path_str}")).dim()
                };
                lines.push(Line::from(span));
            }
        }

        with_border(lines)
    }
}

/// Create the Nori status output cell for the /status command.
///
/// This displays a simplified version of the session header showing:
/// - The /status command echo
/// - Nori branding with version
/// - Directory, agent, and profile info
pub(crate) fn new_nori_status_output(model: &str, directory: PathBuf) -> CompositeHistoryCell {
    let command = PlainHistoryCell::new(vec!["/status".magenta().into()]);
    let header = NoriSessionHeaderCell::new(model.to_string(), directory);

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
        let header = NoriSessionHeaderCell::new(model, config.cwd.clone());

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
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn render_lines(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn read_nori_profile_finds_ancestor_config() {
        // Create a temp directory structure:
        // /tmp/xxx/
        //   .nori-config.json  (with profile)
        //   subdir/
        //     nested/  <- cwd
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path();

        // Create nested directory structure
        let nested = root.join("subdir/nested");
        fs::create_dir_all(&nested).expect("create nested dirs");

        // Create .nori-config.json at root with profile
        let config_content = r#"{
            "profile": {
                "baseProfile": "test-profile"
            }
        }"#;
        fs::write(root.join(".nori-config.json"), config_content).expect("write config");

        // Call read_nori_profile with nested directory as cwd
        let profile = read_nori_profile(&nested);

        assert_eq!(
            profile,
            Some("test-profile".to_string()),
            "Should find profile in ancestor .nori-config.json"
        );
    }

    #[test]
    fn read_nori_profile_returns_none_when_no_config() {
        let tmp = TempDir::new().expect("tempdir");
        let profile = read_nori_profile(tmp.path());
        assert_eq!(
            profile, None,
            "Should return None when no config file exists"
        );
    }

    #[test]
    fn discover_finds_all_ancestors_with_new_function() {
        // Create a temp directory structure with instruction files:
        // /tmp/xxx/
        //   .git  (to mark git root)
        //   AGENTS.md
        //   .claude/
        //     CLAUDE.md  (only specific files are found, not arbitrary .md)
        //   subdir/
        //     CLAUDE.md
        //     nested/  <- cwd
        //       AGENTS.md
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path();

        // Create .git to mark root
        fs::write(root.join(".git"), "gitdir: /path/to/git").expect("write .git");

        // Create instruction files at various levels
        fs::write(root.join("AGENTS.md"), "root agents").expect("write root AGENTS.md");
        fs::create_dir_all(root.join(".claude")).expect("create .claude dir");
        fs::write(root.join(".claude/CLAUDE.md"), "claude hidden")
            .expect("write .claude/CLAUDE.md");

        let subdir = root.join("subdir");
        fs::create_dir_all(&subdir).expect("create subdir");
        fs::write(subdir.join("CLAUDE.md"), "subdir claude").expect("write subdir CLAUDE.md");

        let nested = subdir.join("nested");
        fs::create_dir_all(&nested).expect("create nested");
        fs::write(nested.join("AGENTS.md"), "nested agents").expect("write nested AGENTS.md");

        // Call discover_all_instruction_files_with_home with None home to avoid real home configs
        let files = discover_all_instruction_files_with_home(&nested, None, None);

        // Should find files in order from root to cwd:
        // 1. root/AGENTS.md
        // 2. root/.claude/CLAUDE.md
        // 3. subdir/CLAUDE.md
        // 4. nested/AGENTS.md
        assert_eq!(files.len(), 4, "Should find 4 instruction files");

        // Verify paths contain expected files
        let file_names: Vec<String> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(file_names.contains(&"AGENTS.md".to_string()));
        assert!(file_names.contains(&"CLAUDE.md".to_string()));
    }

    #[test]
    fn discover_returns_empty_when_none_exist() {
        let tmp = TempDir::new().expect("tempdir");
        // Use None home to avoid picking up real home directory configs
        let files = discover_all_instruction_files_with_home(tmp.path(), None, None);
        assert!(
            files.is_empty(),
            "Should return empty vec when no instruction files exist"
        );
    }

    #[test]
    fn nori_header_renders_instruction_files() {
        let cell = NoriSessionHeaderCell {
            version: "test",
            agent: "test-agent".to_string(),
            directory: PathBuf::from("/tmp/test"),
            nori_profile: Some("test-profile".to_string()),
            instruction_files: vec![
                InstructionFile {
                    path: PathBuf::from("/home/user/project/AGENTS.md"),
                    active: true,
                },
                InstructionFile {
                    path: PathBuf::from("/home/user/project/.claude/rules.md"),
                    active: false,
                },
            ],
        };

        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        // Should show instruction files section
        assert!(
            rendered.contains("Instruction Files"),
            "Should show 'Instruction Files' section header"
        );
    }

    #[test]
    fn nori_header_renders_correctly() {
        let cell = NoriSessionHeaderCell::new("test-agent".to_string(), PathBuf::from("/tmp/test"));

        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        // Should contain simple "Nori" title (not ASCII art)
        assert!(
            rendered.contains("Nori"),
            "Header should contain Nori title"
        );

        // Should contain version in the title line
        assert!(rendered.contains(" v"), "Should show version prefix");

        // Should contain directory
        assert!(
            rendered.contains("directory:"),
            "Should show directory label"
        );

        // Should contain agent
        assert!(rendered.contains("agent:"), "Should show agent label");
        assert!(rendered.contains("test-agent"), "Should show agent name");

        // Should contain skillset
        assert!(rendered.contains("skillset:"), "Should show skillset label");
    }

    #[test]
    fn nori_profile_shows_none_when_not_set() {
        // Create cell without a real config file
        let cell = NoriSessionHeaderCell {
            version: "test",
            agent: "test-agent".to_string(),
            directory: PathBuf::from("/tmp/test"),
            nori_profile: None,
            instruction_files: Vec::new(),
        };

        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        assert!(
            rendered.contains("(none)"),
            "Should show (none) when profile not set"
        );
    }

    #[test]
    fn nori_profile_shows_value_when_set() {
        let cell = NoriSessionHeaderCell {
            version: "test",
            agent: "test-agent".to_string(),
            directory: PathBuf::from("/tmp/test"),
            nori_profile: Some("senior-swe".to_string()),
            instruction_files: Vec::new(),
        };

        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        assert!(
            rendered.contains("senior-swe"),
            "Should show profile name when set"
        );
    }

    #[test]
    fn nori_header_snapshot() {
        let cell = NoriSessionHeaderCell {
            version: "0.1.0",
            agent: "claude-sonnet".to_string(),
            directory: PathBuf::from("/home/user/project"),
            nori_profile: Some("senior-swe".to_string()),
            instruction_files: vec![
                InstructionFile {
                    path: PathBuf::from("/home/user/project/AGENTS.md"),
                    active: false,
                },
                InstructionFile {
                    path: PathBuf::from("/home/user/project/.claude/settings.md"),
                    active: true,
                },
            ],
        };

        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        insta::assert_snapshot!(rendered);
    }

    #[test]
    fn nori_status_output_shows_status_command_and_nori_branding() {
        let status_cell = new_nori_status_output("claude-sonnet", PathBuf::from("/tmp/project"));

        let lines = status_cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        // Should show /status command echo
        assert!(
            rendered.contains("/status"),
            "Status output should show /status command"
        );

        // Should contain Nori branding
        assert!(
            rendered.contains("Nori"),
            "Status output should contain Nori branding"
        );

        // Should NOT contain OpenAI Codex branding
        assert!(
            !rendered.contains("OpenAI"),
            "Status output should NOT contain OpenAI branding"
        );
        assert!(
            !rendered.contains("Codex"),
            "Status output should NOT contain Codex branding"
        );

        // Should show directory and agent info
        assert!(
            rendered.contains("directory:"),
            "Status output should show directory"
        );
        assert!(
            rendered.contains("agent:"),
            "Status output should show agent"
        );
        assert!(
            rendered.contains("claude-sonnet"),
            "Status output should show agent name"
        );
    }

    // =========================================================================
    // NEW TESTS: Agent-specific instruction file discovery and activation
    // =========================================================================

    #[test]
    fn detect_agent_kind_from_model_string() {
        // Test Claude variants
        assert_eq!(
            detect_agent_kind("claude-code"),
            Some(AgentKindSimple::Claude)
        );
        assert_eq!(
            detect_agent_kind("claude-sonnet"),
            Some(AgentKindSimple::Claude)
        );
        assert_eq!(
            detect_agent_kind("claude-opus-4"),
            Some(AgentKindSimple::Claude)
        );

        // Test Codex variants
        assert_eq!(detect_agent_kind("codex"), Some(AgentKindSimple::Codex));
        assert_eq!(
            detect_agent_kind("codex-mini"),
            Some(AgentKindSimple::Codex)
        );

        // Test Gemini variants
        assert_eq!(detect_agent_kind("gemini"), Some(AgentKindSimple::Gemini));
        assert_eq!(
            detect_agent_kind("gemini-cli"),
            Some(AgentKindSimple::Gemini)
        );
        assert_eq!(
            detect_agent_kind("gemini-2.0-flash"),
            Some(AgentKindSimple::Gemini)
        );

        // Test unknown
        assert_eq!(detect_agent_kind("gpt-4"), None);
        assert_eq!(detect_agent_kind("unknown-model"), None);
    }

    #[test]
    fn discover_all_instruction_file_types() {
        // Create a temp directory structure with ALL instruction file types:
        // /tmp/xxx/
        //   .git
        //   CLAUDE.md
        //   CLAUDE.local.md
        //   .claude/CLAUDE.md
        //   AGENTS.md
        //   AGENTS.override.md
        //   GEMINI.md
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path();

        // Create .git to mark root
        fs::write(root.join(".git"), "gitdir").expect("write .git");

        // Create all instruction file types
        fs::write(root.join("CLAUDE.md"), "claude").expect("write CLAUDE.md");
        fs::write(root.join("CLAUDE.local.md"), "claude local").expect("write CLAUDE.local.md");
        fs::create_dir_all(root.join(".claude")).expect("create .claude");
        fs::write(root.join(".claude/CLAUDE.md"), "hidden claude")
            .expect("write .claude/CLAUDE.md");
        fs::write(root.join("AGENTS.md"), "agents").expect("write AGENTS.md");
        fs::write(root.join("AGENTS.override.md"), "agents override")
            .expect("write AGENTS.override.md");
        fs::write(root.join("GEMINI.md"), "gemini").expect("write GEMINI.md");

        let files = discover_all_instruction_files_with_home(root, None, None);

        // Should find all 7 files
        let paths: Vec<String> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert!(
            paths.contains(&"CLAUDE.md".to_string()),
            "Should find CLAUDE.md"
        );
        assert!(
            paths.contains(&"CLAUDE.local.md".to_string()),
            "Should find CLAUDE.local.md"
        );
        assert!(
            paths.iter().any(|p| p == "CLAUDE.md"),
            "Should find .claude/CLAUDE.md"
        );
        assert!(
            paths.contains(&"AGENTS.md".to_string()),
            "Should find AGENTS.md"
        );
        assert!(
            paths.contains(&"AGENTS.override.md".to_string()),
            "Should find AGENTS.override.md"
        );
        assert!(
            paths.contains(&"GEMINI.md".to_string()),
            "Should find GEMINI.md"
        );

        // Check we found the hidden variant by checking full path
        let has_hidden_claude = files
            .iter()
            .any(|f| f.path.to_string_lossy().contains(".claude/CLAUDE.md"));
        assert!(
            has_hidden_claude,
            "Should find .claude/CLAUDE.md hidden variant"
        );
    }

    #[test]
    fn claude_activation_algorithm_activates_all_claude_files() {
        // Claude should activate: .claude/CLAUDE.md, CLAUDE.md, CLAUDE.local.md
        // (all three per directory, not exclusive)
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path();

        fs::write(root.join(".git"), "gitdir").expect("write .git");
        fs::write(root.join("CLAUDE.md"), "claude").expect("write CLAUDE.md");
        fs::write(root.join("CLAUDE.local.md"), "claude local").expect("write CLAUDE.local.md");
        fs::create_dir_all(root.join(".claude")).expect("create .claude");
        fs::write(root.join(".claude/CLAUDE.md"), "hidden claude")
            .expect("write .claude/CLAUDE.md");
        fs::write(root.join("AGENTS.md"), "agents").expect("write AGENTS.md");
        fs::write(root.join("GEMINI.md"), "gemini").expect("write GEMINI.md");

        // Use None home to avoid picking up real home directory configs
        let files =
            discover_all_instruction_files_with_home(root, Some(AgentKindSimple::Claude), None);

        // All Claude files should be active
        let claude_files: Vec<_> = files
            .iter()
            .filter(|f| {
                let name = f.path.file_name().unwrap().to_string_lossy();
                name.contains("CLAUDE")
            })
            .collect();

        assert_eq!(claude_files.len(), 3, "Should find 3 Claude files");
        for f in &claude_files {
            assert!(f.active, "Claude file {:?} should be active", f.path);
        }

        // AGENTS.md and GEMINI.md should NOT be active
        let agents_file = files
            .iter()
            .find(|f| f.path.file_name().unwrap().to_string_lossy() == "AGENTS.md")
            .expect("Should find AGENTS.md");
        assert!(
            !agents_file.active,
            "AGENTS.md should NOT be active for Claude agent"
        );

        let gemini_file = files
            .iter()
            .find(|f| f.path.file_name().unwrap().to_string_lossy() == "GEMINI.md")
            .expect("Should find GEMINI.md");
        assert!(
            !gemini_file.active,
            "GEMINI.md should NOT be active for Claude agent"
        );
    }

    #[test]
    fn codex_activation_prefers_override_over_regular() {
        // Codex should activate: AGENTS.override.md OR AGENTS.md (preferring override)
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path();

        fs::write(root.join(".git"), "gitdir").expect("write .git");
        fs::write(root.join("AGENTS.md"), "agents").expect("write AGENTS.md");
        fs::write(root.join("AGENTS.override.md"), "agents override")
            .expect("write AGENTS.override.md");
        fs::write(root.join("CLAUDE.md"), "claude").expect("write CLAUDE.md");

        let files =
            discover_all_instruction_files_with_home(root, Some(AgentKindSimple::Codex), None);

        // AGENTS.override.md should be active (preferred over AGENTS.md)
        let override_file = files
            .iter()
            .find(|f| f.path.file_name().unwrap().to_string_lossy() == "AGENTS.override.md")
            .expect("Should find AGENTS.override.md");
        assert!(override_file.active, "AGENTS.override.md should be active");

        // AGENTS.md should NOT be active when override exists
        let agents_file = files
            .iter()
            .find(|f| f.path.file_name().unwrap().to_string_lossy() == "AGENTS.md")
            .expect("Should find AGENTS.md");
        assert!(
            !agents_file.active,
            "AGENTS.md should NOT be active when override exists"
        );

        // CLAUDE.md should NOT be active
        let claude_file = files
            .iter()
            .find(|f| f.path.file_name().unwrap().to_string_lossy() == "CLAUDE.md")
            .expect("Should find CLAUDE.md");
        assert!(
            !claude_file.active,
            "CLAUDE.md should NOT be active for Codex agent"
        );
    }

    #[test]
    fn codex_activation_falls_back_to_regular_when_no_override() {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path();

        fs::write(root.join(".git"), "gitdir").expect("write .git");
        fs::write(root.join("AGENTS.md"), "agents").expect("write AGENTS.md");
        // No AGENTS.override.md

        let files =
            discover_all_instruction_files_with_home(root, Some(AgentKindSimple::Codex), None);

        let agents_file = files
            .iter()
            .find(|f| f.path.file_name().unwrap().to_string_lossy() == "AGENTS.md")
            .expect("Should find AGENTS.md");
        assert!(
            agents_file.active,
            "AGENTS.md should be active when no override exists"
        );
    }

    #[test]
    fn gemini_activation_only_activates_gemini_files() {
        // Gemini should only activate GEMINI.md files (no hidden variants, no overrides)
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path();

        fs::write(root.join(".git"), "gitdir").expect("write .git");
        fs::write(root.join("GEMINI.md"), "gemini").expect("write GEMINI.md");
        fs::write(root.join("CLAUDE.md"), "claude").expect("write CLAUDE.md");
        fs::write(root.join("AGENTS.md"), "agents").expect("write AGENTS.md");

        let files =
            discover_all_instruction_files_with_home(root, Some(AgentKindSimple::Gemini), None);

        let gemini_file = files
            .iter()
            .find(|f| f.path.file_name().unwrap().to_string_lossy() == "GEMINI.md")
            .expect("Should find GEMINI.md");
        assert!(gemini_file.active, "GEMINI.md should be active");

        // Other files should NOT be active
        for f in &files {
            let name = f.path.file_name().unwrap().to_string_lossy();
            if name != "GEMINI.md" {
                assert!(!f.active, "{name} should NOT be active for Gemini agent");
            }
        }
    }

    #[test]
    fn discovery_traverses_directory_hierarchy() {
        // Test that discovery walks from git root to cwd
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path();

        fs::write(root.join(".git"), "gitdir").expect("write .git");
        fs::write(root.join("CLAUDE.md"), "root claude").expect("write root CLAUDE.md");

        let subdir = root.join("subdir");
        fs::create_dir_all(&subdir).expect("create subdir");
        fs::write(subdir.join("CLAUDE.md"), "subdir claude").expect("write subdir CLAUDE.md");

        let nested = subdir.join("nested");
        fs::create_dir_all(&nested).expect("create nested");
        fs::write(nested.join("CLAUDE.local.md"), "nested local")
            .expect("write nested CLAUDE.local.md");

        // Discover from nested directory (use None home to avoid real home configs)
        let files =
            discover_all_instruction_files_with_home(&nested, Some(AgentKindSimple::Claude), None);

        // Should find files from all levels
        assert_eq!(files.len(), 3, "Should find 3 files across hierarchy");

        // All should be active for Claude
        for f in &files {
            assert!(f.active, "File {:?} should be active for Claude", f.path);
        }
    }

    #[test]
    fn header_renders_instruction_files_section() {
        let files = vec![
            InstructionFile {
                path: PathBuf::from("/home/user/.claude/CLAUDE.md"),
                active: true,
            },
            InstructionFile {
                path: PathBuf::from("/home/user/project/CLAUDE.md"),
                active: true,
            },
            InstructionFile {
                path: PathBuf::from("/home/user/project/AGENTS.md"),
                active: false,
            },
        ];

        let cell = NoriSessionHeaderCell {
            version: "test",
            agent: "claude-code".to_string(),
            directory: PathBuf::from("/home/user/project"),
            nori_profile: Some("test-profile".to_string()),
            instruction_files: files,
        };

        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        // Should have "Instruction Files" section header
        assert!(
            rendered.contains("Instruction Files"),
            "Should show 'Instruction Files' section header"
        );

        // Should show file paths
        assert!(
            rendered.contains("CLAUDE.md"),
            "Should show CLAUDE.md in output"
        );
    }

    // =========================================================================
    // HOME CONFIG DISCOVERY TESTS
    // =========================================================================

    #[test]
    fn discover_finds_claude_home_config() {
        // Test that discovery finds ~/.claude/CLAUDE.md for Claude agents
        // Structure:
        //   fake_home/
        //     .claude/
        //       CLAUDE.md  <- user-level config
        //   project/
        //     .git
        //     CLAUDE.md  <- project-level config
        let tmp = TempDir::new().expect("tempdir");
        let fake_home = tmp.path().join("fake_home");
        let project = tmp.path().join("project");

        // Create fake home with .claude/CLAUDE.md
        fs::create_dir_all(fake_home.join(".claude")).expect("create .claude");
        fs::write(fake_home.join(".claude/CLAUDE.md"), "user claude config")
            .expect("write user CLAUDE.md");

        // Create project with .git and CLAUDE.md
        fs::create_dir_all(&project).expect("create project");
        fs::write(project.join(".git"), "gitdir").expect("write .git");
        fs::write(project.join("CLAUDE.md"), "project claude config")
            .expect("write project CLAUDE.md");

        // Discover files with custom home
        let files = discover_all_instruction_files_with_home(
            &project,
            Some(AgentKindSimple::Claude),
            Some(&fake_home),
        );

        // Should find both user-level and project-level config
        assert!(
            files.len() >= 2,
            "Should find at least 2 files (user and project): found {}",
            files.len()
        );

        // Should find the home config
        let has_home_config = files
            .iter()
            .any(|f| f.path.to_string_lossy().contains(".claude/CLAUDE.md"));
        assert!(has_home_config, "Should find ~/.claude/CLAUDE.md");

        // Home config should be active for Claude
        let home_file = files
            .iter()
            .find(|f| {
                f.path.to_string_lossy().contains("fake_home")
                    && f.path.to_string_lossy().contains(".claude/CLAUDE.md")
            })
            .expect("Should find home CLAUDE.md");
        assert!(
            home_file.active,
            "Home CLAUDE.md should be active for Claude"
        );
    }

    #[test]
    fn discover_finds_codex_home_config() {
        // Test that discovery finds ~/.codex/AGENTS.md for Codex agents
        let tmp = TempDir::new().expect("tempdir");
        let fake_home = tmp.path().join("fake_home");
        let project = tmp.path().join("project");

        // Create fake home with .codex/AGENTS.md
        fs::create_dir_all(fake_home.join(".codex")).expect("create .codex");
        fs::write(fake_home.join(".codex/AGENTS.md"), "user codex config")
            .expect("write user AGENTS.md");

        // Create project with .git
        fs::create_dir_all(&project).expect("create project");
        fs::write(project.join(".git"), "gitdir").expect("write .git");

        // Discover files with custom home
        let files = discover_all_instruction_files_with_home(
            &project,
            Some(AgentKindSimple::Codex),
            Some(&fake_home),
        );

        // Should find the home config
        let has_home_config = files
            .iter()
            .any(|f| f.path.to_string_lossy().contains(".codex/AGENTS.md"));
        assert!(has_home_config, "Should find ~/.codex/AGENTS.md");

        // Home config should be active for Codex
        let home_file = files
            .iter()
            .find(|f| f.path.to_string_lossy().contains(".codex/AGENTS.md"))
            .expect("Should find home AGENTS.md");
        assert!(
            home_file.active,
            "Home AGENTS.md should be active for Codex"
        );
    }

    #[test]
    fn discover_finds_gemini_home_config() {
        // Test that discovery finds ~/.gemini/GEMINI.md for Gemini agents
        let tmp = TempDir::new().expect("tempdir");
        let fake_home = tmp.path().join("fake_home");
        let project = tmp.path().join("project");

        // Create fake home with .gemini/GEMINI.md
        fs::create_dir_all(fake_home.join(".gemini")).expect("create .gemini");
        fs::write(fake_home.join(".gemini/GEMINI.md"), "user gemini config")
            .expect("write user GEMINI.md");

        // Create project with .git
        fs::create_dir_all(&project).expect("create project");
        fs::write(project.join(".git"), "gitdir").expect("write .git");

        // Discover files with custom home
        let files = discover_all_instruction_files_with_home(
            &project,
            Some(AgentKindSimple::Gemini),
            Some(&fake_home),
        );

        // Should find the home config
        let has_home_config = files
            .iter()
            .any(|f| f.path.to_string_lossy().contains(".gemini/GEMINI.md"));
        assert!(has_home_config, "Should find ~/.gemini/GEMINI.md");

        // Home config should be active for Gemini
        let home_file = files
            .iter()
            .find(|f| f.path.to_string_lossy().contains(".gemini/GEMINI.md"))
            .expect("Should find home GEMINI.md");
        assert!(
            home_file.active,
            "Home GEMINI.md should be active for Gemini"
        );
    }

    #[test]
    fn discover_home_config_is_inactive_for_other_agents() {
        // Test that Claude home config is inactive when running as Codex agent
        let tmp = TempDir::new().expect("tempdir");
        let fake_home = tmp.path().join("fake_home");
        let project = tmp.path().join("project");

        // Create fake home with all agent configs
        fs::create_dir_all(fake_home.join(".claude")).expect("create .claude");
        fs::write(fake_home.join(".claude/CLAUDE.md"), "user claude config")
            .expect("write user CLAUDE.md");
        fs::create_dir_all(fake_home.join(".codex")).expect("create .codex");
        fs::write(fake_home.join(".codex/AGENTS.md"), "user codex config")
            .expect("write user AGENTS.md");

        // Create project with .git
        fs::create_dir_all(&project).expect("create project");
        fs::write(project.join(".git"), "gitdir").expect("write .git");

        // Discover files as Codex agent
        let files = discover_all_instruction_files_with_home(
            &project,
            Some(AgentKindSimple::Codex),
            Some(&fake_home),
        );

        // Claude home config should exist but be inactive
        let claude_file = files
            .iter()
            .find(|f| f.path.to_string_lossy().contains(".claude/CLAUDE.md"));
        if let Some(f) = claude_file {
            assert!(
                !f.active,
                "Claude home config should be inactive for Codex agent"
            );
        }

        // Codex home config should be active
        let codex_file = files
            .iter()
            .find(|f| f.path.to_string_lossy().contains(".codex/AGENTS.md"))
            .expect("Should find Codex home config");
        assert!(codex_file.active, "Codex home config should be active");
    }

    #[test]
    fn discover_home_config_order_is_first() {
        // Test that home config appears first in the list (before project configs)
        let tmp = TempDir::new().expect("tempdir");
        let fake_home = tmp.path().join("fake_home");
        let project = tmp.path().join("project");

        // Create fake home with .claude/CLAUDE.md
        fs::create_dir_all(fake_home.join(".claude")).expect("create .claude");
        fs::write(fake_home.join(".claude/CLAUDE.md"), "user claude config")
            .expect("write user CLAUDE.md");

        // Create project with .git and CLAUDE.md
        fs::create_dir_all(&project).expect("create project");
        fs::write(project.join(".git"), "gitdir").expect("write .git");
        fs::write(project.join("CLAUDE.md"), "project claude config")
            .expect("write project CLAUDE.md");

        // Discover files with custom home
        let files = discover_all_instruction_files_with_home(
            &project,
            Some(AgentKindSimple::Claude),
            Some(&fake_home),
        );

        assert!(files.len() >= 2, "Should find at least 2 files");

        // First file should be the home config
        let first_file = &files[0];
        assert!(
            first_file.path.to_string_lossy().contains("fake_home"),
            "First file should be the home config, got: {:?}",
            first_file.path
        );
    }
}
