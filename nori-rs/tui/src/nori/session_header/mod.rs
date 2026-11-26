//! Nori-branded session header component for the TUI.
//!
//! This module provides the Nori session header that appears at the start
//! of every session, displaying the Nori title, version info,
//! agent details, and active skillset information.
//!
//! The session header uses a simple "Nori" text title (the ASCII art banner
//! is reserved for the first-launch welcome screen).

use crate::exec_command::relativize_to_home;
use crate::history_cell::CompositeHistoryCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::PlainHistoryCell;
use crate::history_cell::SessionInfoCell;
use nori_config::NoriConfig;
use ratatui::prelude::*;
use ratatui::style::Stylize;
use std::path::Path;
use unicode_width::UnicodeWidthStr;

#[cfg(test)]
mod profile;
mod startup_face;
mod status_card;
pub(crate) use startup_face::StartupFace;
pub(crate) mod status_view;

pub use nori_harness::instruction_files::AgentKindSimple;
pub use nori_harness::instruction_files::InstructionFile;
use nori_harness::instruction_files::detect_agent_kind;
use nori_harness::instruction_files::discover_all_instruction_files;
#[cfg(test)]
use nori_harness::instruction_files::discover_all_instruction_files_with_paths;
pub(crate) use status_view::AgentStatus;
pub(crate) use status_view::AgentStatusHandle;
pub(crate) use status_view::CloudSessionInfo;
pub(crate) use status_view::ContextStatus;
pub(crate) use status_view::GitStatus;
pub(crate) use status_view::SkillsetStatus;
pub(crate) use status_view::StatusFooterValues;
pub(crate) use status_view::StatusViewModel;
pub(crate) use status_view::active_instruction_file_contents;
pub(crate) use status_view::local_instruction_files;

/// Maximum content width for the compact Nori status block.
const NORI_HEADER_MAX_INNER_WIDTH: usize = 100;

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

/// Controls the amount of status information shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisplayMode {
    /// Session header at start: compact system and agent summary only.
    Compact,
    /// `/status` command: complete metadata plus the local instruction-file outline.
    Full,
}

/// The Nori-branded session header cell: a pure view over [`StatusViewModel`].
#[derive(Debug)]
pub(crate) struct NoriSessionHeaderCell {
    model: StatusViewModel,
    display_mode: DisplayMode,
    /// The welcome header's face and wordmark. Other headers keep the plain
    /// `› Nori CLI` title.
    face: Option<StartupFace>,
}

impl NoriSessionHeaderCell {
    pub(crate) fn new(model: StatusViewModel) -> Self {
        Self {
            model,
            display_mode: DisplayMode::Full,
            face: None,
        }
    }

    pub(crate) fn with_face(mut self, face: StartupFace) -> Self {
        self.face = Some(face);
        self
    }

    pub(crate) fn with_display_mode(mut self, mode: DisplayMode) -> Self {
        self.display_mode = mode;
        self
    }
}

impl HistoryCell for NoriSessionHeaderCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if width < 4 {
            return Vec::new();
        }
        let inner_width = usize::from(width.saturating_sub(2)).min(NORI_HEADER_MAX_INNER_WIDTH);

        // The prompt marker is the sole Nori accent. The title and version use
        // ordinary text hierarchy and the block deliberately has no surface or border.
        let title = match &self.face {
            Some(face) => face.title_line(self.model.version),
            None => Line::from(vec![
                Span::from("  › ").green(),
                Span::from("Nori CLI").bold(),
                Span::from(format!(" v{}", self.model.version)).dim(),
            ]),
        };
        let mut lines: Vec<Line<'static>> = vec![title, Line::from("")];

        lines.extend(match self.display_mode {
            DisplayMode::Compact => status_card::compact_lines(&self.model, inner_width),
            DisplayMode::Full => status_card::full_lines(&self.model, inner_width),
        });

        lines
    }
}

/// Format an instruction file path for display, relativizing to the home directory.
pub(crate) fn format_instruction_path(path: &Path) -> String {
    format_directory(path, None)
}

/// Create the Nori status output cell for the `/status` command.
///
/// This displays the full, unbordered status block showing:
/// - The `/status` command echo
/// - Nori branding with version
/// - Session metadata (directory, ids, skillset, approvals, git, context)
/// - The agent and every configuration option it advertises
/// - Active local instruction files (suppressed for cloud sessions)
pub(crate) fn new_nori_status_output(model: StatusViewModel) -> CompositeHistoryCell {
    let command = PlainHistoryCell::new(vec![Line::from(vec!["/".green(), "status".into()])]);
    let header = NoriSessionHeaderCell::new(model);

    CompositeHistoryCell::new(vec![Box::new(command), Box::new(header)])
}

/// Create the Nori session info cell to be displayed at session start.
///
/// The welcome card's title is `face`: the animated startup face, or the
/// settled wordmark.
pub(crate) fn new_nori_session_info(
    config: &NoriConfig,
    model: String,
    is_first_event: bool,
    view_model: StatusViewModel,
    face: StartupFace,
) -> SessionInfoCell {
    SessionInfoCell::new(if is_first_event {
        // Header box rendered as history (so it appears at the very top)
        let header = NoriSessionHeaderCell::new(view_model)
            .with_display_mode(DisplayMode::Compact)
            .with_face(face);

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
