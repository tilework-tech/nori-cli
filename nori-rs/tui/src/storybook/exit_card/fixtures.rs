//! Synthetic exit-card fixtures. Every value here is invented.

use crate::session_stats::SessionStats;
use crate::ui_types::TokenUsage;

use super::ExitCardModel;

const SESSION_ID: &str = "019a7c3e-5b21-7f4d-9e08-6c2a1d4b8f37";

/// Which tool list the card shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExitToolsFixture {
    /// Six common tools.
    #[default]
    Few,
    /// Fourteen tools, two of them long MCP names.
    Many,
    /// Long names, including one with no separator to break after.
    LongNames,
    /// No tool calls at all.
    None,
}

impl ExitToolsFixture {
    pub const ALL: [Self; 4] = [Self::Few, Self::Many, Self::LongNames, Self::None];

    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Few => Self::Many,
            Self::Many => Self::LongNames,
            Self::LongNames => Self::None,
            Self::None => Self::Few,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Few => "6 tools",
            Self::Many => "14 tools, long MCP names",
            Self::LongNames => "long names, one unbreakable",
            Self::None => "no tools",
        }
    }

    fn tools(self) -> &'static [(&'static str, u32)] {
        match self {
            Self::Few => &[
                ("Read", 37),
                ("Bash", 29),
                ("Edit", 18),
                ("Grep", 12),
                ("Glob", 6),
                ("Write", 3),
            ],
            Self::Many => &[
                ("Read", 37),
                ("Bash", 29),
                ("Edit", 18),
                ("Grep", 12),
                ("TodoWrite", 7),
                ("Glob", 6),
                ("WebFetch", 5),
                ("mcp__github__create_pull_request_review_comment", 4),
                ("WebSearch", 4),
                ("Write", 3),
                ("mcp__linear__list_issues", 3),
                ("Task", 2),
                ("Skill", 2),
                ("NotebookEdit", 1),
            ],
            Self::LongNames => &[
                ("Read", 9),
                (
                    "mcp__acme-observability__query_trace_spans_by_service_and_window",
                    6,
                ),
                ("Bash", 4),
                ("Exampletoolnamewithoutanyseparatorsatall", 2),
            ],
            Self::None => &[],
        }
    }
}

/// Which of the other fields are present.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExitFieldsFixture {
    /// Resumable, with tokens, skills, and subagents.
    #[default]
    Full,
    /// Resumable, but only messages and tools were recorded.
    Minimal,
    /// The session has an ID but nothing to resume.
    NotResumable,
    /// The session never got an ID.
    NoSession,
}

impl ExitFieldsFixture {
    pub const ALL: [Self; 4] = [
        Self::Full,
        Self::Minimal,
        Self::NotResumable,
        Self::NoSession,
    ];

    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Full => Self::Minimal,
            Self::Minimal => Self::NotResumable,
            Self::NotResumable => Self::NoSession,
            Self::NoSession => Self::Full,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "all fields",
            Self::Minimal => "messages and tools only",
            Self::NotResumable => "not resumable",
            Self::NoSession => "no session ID",
        }
    }
}

/// Build the fixture model for one tools/fields combination.
#[must_use]
pub fn exit_card_fixture(tools: ExitToolsFixture, fields: ExitFieldsFixture) -> ExitCardModel {
    let mut stats = SessionStats::new();
    for (name, count) in tools.tools() {
        stats.tool_calls.insert((*name).to_string(), *count);
    }
    let active = matches!(fields, ExitFieldsFixture::Full | ExitFieldsFixture::Minimal);
    if active {
        stats.user_messages = 14;
        stats.assistant_messages = 19;
    }
    let full = fields == ExitFieldsFixture::Full;
    if full {
        stats.skills_used = vec!["test-driven-development".into(), "writing-plans".into()];
        stats.subagents_used = vec!["nori-code-researcher".into(), "nori-web-researcher".into()];
    }
    ExitCardModel {
        session_id: (fields != ExitFieldsFixture::NoSession).then(|| SESSION_ID.to_string()),
        resumable: active,
        stats,
        token_usage: full.then_some(TokenUsage {
            input_tokens: 388_200,
            cached_input_tokens: 301_400,
            output_tokens: 24_100,
            reasoning_output_tokens: 0,
            total_tokens: 412_300,
        }),
    }
}
