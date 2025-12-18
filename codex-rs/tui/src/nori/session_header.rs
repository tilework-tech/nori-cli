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
use serde::Deserialize;
use std::path::Path;
use std::path::PathBuf;
use unicode_width::UnicodeWidthStr;

/// Maximum inner width for the Nori session header card.
const NORI_HEADER_MAX_INNER_WIDTH: usize = 60;

/// Nori config file structure (partial - only what we need)
#[derive(Debug, Deserialize, Default)]
struct NoriConfig {
    #[serde(default)]
    agents: Option<NoriAgents>,
}

#[derive(Debug, Deserialize, Default)]
struct NoriAgents {
    #[serde(default, rename = "claude-code")]
    claude_code: Option<NoriAgentConfig>,
}

#[derive(Debug, Deserialize, Default)]
struct NoriAgentConfig {
    #[serde(default)]
    profile: Option<NoriProfile>,
}

#[derive(Debug, Deserialize)]
struct NoriProfile {
    #[serde(rename = "baseProfile")]
    base_profile: Option<String>,
}

/// Result of reading the nori config
struct NoriConfigInfo {
    profile: Option<String>,
    install_dir: Option<PathBuf>,
}

/// Run `nori-ai install-location` and parse the first (nearest) install directory.
fn get_nearest_install_location() -> Option<PathBuf> {
    let output = std::process::Command::new("nori-ai")
        .arg("install-location")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse output format:
    // "Nori installation directories:\n\n  /path/one\n  /path/two\n"
    // The first non-empty path after the header is the nearest
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.contains("installation directories") {
            continue;
        }
        // Found a path
        return Some(PathBuf::from(trimmed));
    }

    None
}

/// Read the Nori config from the nearest install location.
/// Uses `nori-ai install-location` to find the correct config file.
fn read_nori_config() -> NoriConfigInfo {
    let install_dir = match get_nearest_install_location() {
        Some(dir) => dir,
        None => {
            return NoriConfigInfo {
                profile: None,
                install_dir: None,
            }
        }
    };

    let config_path = install_dir.join(".nori-config.json");

    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => {
            return NoriConfigInfo {
                profile: None,
                install_dir: Some(install_dir),
            }
        }
    };

    let config: NoriConfig = match serde_json::from_str(&content) {
        Ok(c) => c,
        Err(_) => {
            return NoriConfigInfo {
                profile: None,
                install_dir: Some(install_dir),
            }
        }
    };

    // Extract profile from agents.claude-code.profile.baseProfile
    let profile = config
        .agents
        .and_then(|a| a.claude_code)
        .and_then(|c| c.profile)
        .and_then(|p| p.base_profile);

    NoriConfigInfo {
        profile,
        install_dir: Some(install_dir),
    }
}

/// Check if the nori-ai command is available in PATH
fn is_nori_ai_installed() -> bool {
    which::which("nori-ai").is_ok()
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
    profile_location: Option<PathBuf>,
    nori_ai_installed: bool,
}

impl NoriSessionHeaderCell {
    pub(crate) fn new(agent: String, directory: PathBuf) -> Self {
        let nori_ai_installed = is_nori_ai_installed();
        let nori_config = if nori_ai_installed {
            read_nori_config()
        } else {
            NoriConfigInfo {
                profile: None,
                install_dir: None,
            }
        };
        Self {
            version: CODEX_CLI_VERSION,
            agent,
            directory,
            nori_profile: nori_config.profile,
            profile_location: nori_config.install_dir,
            nori_ai_installed,
        }
    }

    #[cfg(test)]
    fn new_for_test(
        version: &'static str,
        agent: String,
        directory: PathBuf,
        nori_profile: Option<String>,
        profile_location: Option<PathBuf>,
        nori_ai_installed: bool,
    ) -> Self {
        Self {
            version,
            agent,
            directory,
            nori_profile,
            profile_location,
            nori_ai_installed,
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

        // Profiles section - only shown if nori-ai is installed
        if self.nori_ai_installed {
            // Empty line before Profiles section
            lines.push(Line::from(""));

            // Profiles section header (green like Nori title)
            lines.push(Line::from(Span::from("Profiles").green().bold()));

            // Current profile line
            let profile_display = self
                .nori_profile
                .clone()
                .unwrap_or_else(|| "(none)".to_string());
            lines.push(Line::from(vec![
                Span::from("current:  ").dim(),
                Span::from(profile_display),
            ]));

            // Profile location line
            if let Some(ref location) = self.profile_location {
                let location_max_width = inner_width.saturating_sub(10); // "location: " is 10 chars
                let location_display = format_directory(location, Some(location_max_width));
                lines.push(Line::from(vec![
                    Span::from("location: ").dim(),
                    Span::from(location_display),
                ]));
            }
        }

        with_border(lines)
    }
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
    fn nori_header_renders_correctly() {
        let cell = NoriSessionHeaderCell::new_for_test(
            "0.1.0",
            "test-agent".to_string(),
            PathBuf::from("/tmp/test"),
            Some("senior-swe".to_string()),
            Some(PathBuf::from("/home/user")),
            true,
        );

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

        // Should contain Profiles section with current profile
        assert!(rendered.contains("Profiles"), "Should show Profiles section");
        assert!(rendered.contains("current:"), "Should show current profile label");
    }

    #[test]
    fn nori_profile_shows_none_when_not_set() {
        // Create cell with nori_ai_installed = true but no profile
        let cell = NoriSessionHeaderCell::new_for_test(
            "test",
            "test-agent".to_string(),
            PathBuf::from("/tmp/test"),
            None,
            None,
            true, // nori_ai_installed
        );

        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        assert!(
            rendered.contains("(none)"),
            "Should show (none) when profile not set"
        );
        assert!(
            rendered.contains("Profiles"),
            "Should show Profiles section when nori-ai installed"
        );
    }

    #[test]
    fn nori_profile_shows_value_when_set() {
        let cell = NoriSessionHeaderCell::new_for_test(
            "test",
            "test-agent".to_string(),
            PathBuf::from("/tmp/test"),
            Some("senior-swe".to_string()),
            Some(PathBuf::from("/home/user")),
            true, // nori_ai_installed
        );

        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        assert!(
            rendered.contains("senior-swe"),
            "Should show profile name when set"
        );
        assert!(
            rendered.contains("location:"),
            "Should show location label"
        );
        assert!(
            rendered.contains("Profiles"),
            "Should show Profiles section header"
        );
    }

    #[test]
    fn nori_profiles_section_hidden_when_not_installed() {
        let cell = NoriSessionHeaderCell::new_for_test(
            "test",
            "test-agent".to_string(),
            PathBuf::from("/tmp/test"),
            Some("senior-swe".to_string()),
            Some(PathBuf::from("/home/user")),
            false, // nori_ai NOT installed
        );

        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        assert!(
            !rendered.contains("Profiles"),
            "Should NOT show Profiles section when nori-ai not installed"
        );
        assert!(
            !rendered.contains("senior-swe"),
            "Should NOT show profile when nori-ai not installed"
        );
    }

    #[test]
    fn nori_header_snapshot() {
        let cell = NoriSessionHeaderCell::new_for_test(
            "0.1.0",
            "claude-sonnet".to_string(),
            PathBuf::from("/home/user/project"),
            Some("senior-swe".to_string()),
            Some(PathBuf::from("/home/user")),
            true, // nori_ai_installed
        );

        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        insta::assert_snapshot!(rendered);
    }

    #[test]
    fn nori_header_snapshot_no_profiles() {
        let cell = NoriSessionHeaderCell::new_for_test(
            "0.1.0",
            "claude-sonnet".to_string(),
            PathBuf::from("/home/user/project"),
            None,
            None,
            false, // nori_ai NOT installed
        );

        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        insta::assert_snapshot!(rendered);
    }
}
