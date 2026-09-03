//! Production-view specimens for the TUI storybooks.
//!
//! The storybook must show what a session shows, so it renders the real status
//! views and owns only the fixture data. Gated behind the `storybook` feature
//! so none of it reaches the shipped binary.

use std::path::PathBuf;

use nori_protocol::acp::v1 as acp;
use ratatui::text::Line;

use crate::history_cell::HistoryCell;
use crate::nori::agent_config_state::AgentConfigState;
use crate::nori::session_header::AgentStatus;
use crate::nori::session_header::AgentStatusHandle;
use crate::nori::session_header::ContextStatus;
use crate::nori::session_header::DisplayMode;
use crate::nori::session_header::GitStatus;
use crate::nori::session_header::InstructionFile;
use crate::nori::session_header::NoriSessionHeaderCell;
use crate::nori::session_header::SkillsetStatus;
use crate::nori::session_header::StatusViewModel;
use crate::nori::token_count::TokenCount;

/// Which status rendering to show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusSpecimen {
    /// The compact block shown at session start.
    #[default]
    Compact,
    /// The full `/status` card.
    Full,
}

impl StatusSpecimen {
    /// Cycle to the other rendering.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Compact => Self::Full,
            Self::Full => Self::Compact,
        }
    }

    /// The label shown in the storybook chrome.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Full => "Full",
        }
    }
}

/// Render a status specimen through the production view.
#[must_use]
pub fn status_specimen_lines(specimen: StatusSpecimen, width: u16) -> Vec<Line<'static>> {
    let display_mode = match specimen {
        StatusSpecimen::Compact => DisplayMode::Compact,
        StatusSpecimen::Full => DisplayMode::Full,
    };
    NoriSessionHeaderCell::new(fixture_model())
        .with_display_mode(display_mode)
        .display_lines(width)
}

/// A representative session: a Claude agent that has advertised a mode, a
/// model, a thought level, and a boolean toggle, in a git checkout with a
/// skillset and two loaded instruction files.
fn fixture_model() -> StatusViewModel {
    let agent = "claude-code";
    let config = AgentConfigState::from_options(&[
        select(
            "mode",
            "Mode",
            "plan",
            &[("plan", "Plan"), ("build", "Build")],
        )
        .category(acp::SessionConfigOptionCategory::Mode),
        select(
            "model",
            "Model",
            "opus-5",
            &[("opus-5", "Opus 5"), ("sonnet-5", "Sonnet 5")],
        )
        .category(acp::SessionConfigOptionCategory::Model),
        select("effort", "Effort", "xhigh", &[("xhigh", "xhigh")])
            .category(acp::SessionConfigOptionCategory::ThoughtLevel),
        acp::SessionConfigOption::new(
            "fast-mode",
            "Fast mode",
            acp::SessionConfigKind::Boolean(acp::SessionConfigBoolean::new(false)),
        ),
    ]);

    let mut model = StatusViewModel::new(
        AgentStatusHandle::new(AgentStatus::from_config(agent, &config)),
        PathBuf::from("/home/user/org/workspace/cli"),
    );
    model.version = "0.1.0";
    model.approval_mode_label = Some("Agent".to_string());
    model.session_title = Some("Fix terminal hierarchy".to_string());
    model.prompt_summary = Some("Rework the status card".to_string());
    model.skillset = SkillsetStatus {
        name: Some("senior-swe".to_string()),
        version: Some("1.2.3".to_string()),
        version_source: None,
    };
    model.git = GitStatus {
        branch: Some("main".to_string()),
        has_untracked: true,
        ..GitStatus::default()
    };
    model.context = ContextStatus {
        tokens: Some(164_000),
        window_tokens: Some(1_000_000),
        percent_used: Some(16),
    };
    model.instruction_files = vec![
        instruction_file("/home/user/.claude/CLAUDE.md", 2950),
        instruction_file("/home/user/org/workspace/cli/CLAUDE.md", 2164),
    ];
    model
}

fn select(
    id: &str,
    name: &str,
    current: &str,
    values: &[(&str, &str)],
) -> acp::SessionConfigOption {
    acp::SessionConfigOption::select(
        id.to_string(),
        name.to_string(),
        current.to_string(),
        values
            .iter()
            .map(|(value, label)| {
                acp::SessionConfigSelectOption::new(value.to_string(), label.to_string())
            })
            .collect::<Vec<_>>(),
    )
}

fn instruction_file(path: &str, tokens: i64) -> InstructionFile {
    InstructionFile {
        path: PathBuf::from(path),
        active: true,
        token_count: Some(TokenCount {
            count: tokens,
            approximate: true,
        }),
    }
}
