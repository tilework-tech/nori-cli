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
    profile: Option<NoriProfile>,
}

#[derive(Debug, Deserialize)]
struct NoriProfile {
    #[serde(rename = "baseProfile")]
    base_profile: Option<String>,
}

/// Read the current Nori profile from ~/.nori-config.json
fn read_nori_profile() -> Option<String> {
    let home = dirs::home_dir()?;
    let config_path = home.join(".nori-config.json");

    let content = std::fs::read_to_string(config_path).ok()?;
    let config: NoriConfig = serde_json::from_str(&content).ok()?;

    config.profile.and_then(|p| p.base_profile)
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
}

impl NoriSessionHeaderCell {
    pub(crate) fn new(agent: String, directory: PathBuf) -> Self {
        Self {
            version: CODEX_CLI_VERSION,
            agent,
            directory,
            nori_profile: read_nori_profile(),
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
        };

        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        insta::assert_snapshot!(rendered);
    }
}
