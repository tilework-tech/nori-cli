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

/// Check if the nori-ai command is available in PATH
fn is_nori_ai_installed() -> bool {
    which::which("nori-ai").is_ok()
}

/// Discover instruction files (CLAUDE.md, AGENTS.md, .claude/*.md) in ancestors.
///
/// Walks from the git root (or cwd if no git root) to cwd, collecting all
/// instruction files found along the path. Returns paths ordered from root to cwd.
fn discover_instruction_files(cwd: &Path) -> Vec<PathBuf> {
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
    let search_dirs: Vec<PathBuf> = if let Some(root) = git_root {
        // Reverse the chain and filter to only include from git root onward
        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut saw_root = false;
        for p in chain.iter().rev() {
            if !saw_root {
                if p == &root {
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

    let mut found: Vec<PathBuf> = Vec::new();

    for dir in search_dirs {
        // Check for CLAUDE.md
        let claude_md = dir.join("CLAUDE.md");
        if claude_md.is_file() {
            found.push(claude_md);
        }

        // Check for AGENTS.md
        let agents_md = dir.join("AGENTS.md");
        if agents_md.is_file() {
            found.push(agents_md);
        }

        // Check for .claude/*.md files
        let claude_dir = dir.join(".claude");
        if claude_dir.is_dir()
            && let Ok(entries) = std::fs::read_dir(&claude_dir)
        {
            let mut md_files: Vec<PathBuf> = entries
                .flatten()
                .filter_map(|entry| {
                    let path = entry.path();
                    if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
                        Some(path)
                    } else {
                        None
                    }
                })
                .collect();
            // Sort for deterministic ordering
            md_files.sort();
            found.extend(md_files);
        }
    }

    found
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
    instruction_files: Vec<PathBuf>,
}

impl NoriSessionHeaderCell {
    pub(crate) fn new(agent: String, directory: PathBuf) -> Self {
        let nori_profile = read_nori_profile(&directory);
        let instruction_files = discover_instruction_files(&directory);
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
            Span::from("Nori").green().bold(),
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
            Span::from("profile:   ").dim(),
            Span::from(profile_display),
        ]));

        // Instruction files lines (agents.md: path)
        for path in &self.instruction_files {
            let path_str = format_directory(path, Some(dir_max_width));
            lines.push(Line::from(vec![
                Span::from("agents.md: ").dim(),
                Span::from(path_str),
            ]));
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
        let mut help_lines: Vec<Line<'static>> = vec![
            Line::from(""),
            Line::from(vec![
                "  🍙 ".into(),
                "Powered by Nori AI".bold(),
                " 🍙".into(),
            ]),
        ];

        // Only show install hint if nori-ai is not already installed
        if !is_nori_ai_installed() {
            help_lines.push(Line::from(""));
            help_lines.push(Line::from(vec![
                "  Run '".dim(),
                "npx nori-ai install".cyan(),
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

    // @current-session
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

    // @current-session
    #[test]
    fn read_nori_profile_returns_none_when_no_config() {
        let tmp = TempDir::new().expect("tempdir");
        let profile = read_nori_profile(tmp.path());
        assert_eq!(
            profile, None,
            "Should return None when no config file exists"
        );
    }

    // @current-session
    #[test]
    fn discover_instruction_files_finds_all_ancestors() {
        // Create a temp directory structure with instruction files:
        // /tmp/xxx/
        //   .git  (to mark git root)
        //   AGENTS.md
        //   .claude/
        //     settings.md
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
        fs::write(root.join(".claude/settings.md"), "claude settings")
            .expect("write .claude/settings.md");

        let subdir = root.join("subdir");
        fs::create_dir_all(&subdir).expect("create subdir");
        fs::write(subdir.join("CLAUDE.md"), "subdir claude").expect("write subdir CLAUDE.md");

        let nested = subdir.join("nested");
        fs::create_dir_all(&nested).expect("create nested");
        fs::write(nested.join("AGENTS.md"), "nested agents").expect("write nested AGENTS.md");

        // Call discover_instruction_files with nested as cwd
        let files = discover_instruction_files(&nested);

        // Should find files in order from root to cwd:
        // 1. root/AGENTS.md
        // 2. root/.claude/settings.md
        // 3. subdir/CLAUDE.md
        // 4. nested/AGENTS.md
        assert_eq!(files.len(), 4, "Should find 4 instruction files");

        // Verify paths contain expected files
        let file_names: Vec<String> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(file_names.contains(&"AGENTS.md".to_string()));
        assert!(file_names.contains(&"CLAUDE.md".to_string()));
        assert!(file_names.contains(&"settings.md".to_string()));
    }

    // @current-session
    #[test]
    fn discover_instruction_files_returns_empty_when_none_exist() {
        let tmp = TempDir::new().expect("tempdir");
        let files = discover_instruction_files(tmp.path());
        assert!(
            files.is_empty(),
            "Should return empty vec when no instruction files exist"
        );
    }

    // @current-session
    #[test]
    fn nori_header_renders_instruction_files() {
        let cell = NoriSessionHeaderCell {
            version: "test",
            agent: "test-agent".to_string(),
            directory: PathBuf::from("/tmp/test"),
            nori_profile: Some("test-profile".to_string()),
            instruction_files: vec![
                PathBuf::from("/home/user/project/AGENTS.md"),
                PathBuf::from("/home/user/project/.claude/rules.md"),
            ],
        };

        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        // Should show instruction files
        assert!(
            rendered.contains("agents.md:"),
            "Should show agents.md label for instruction files"
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

        // Should contain profile
        assert!(rendered.contains("profile:"), "Should show profile label");
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
                PathBuf::from("/home/user/project/AGENTS.md"),
                PathBuf::from("/home/user/project/.claude/settings.md"),
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
}
