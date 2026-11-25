//! Synthetic exit-card fixtures. Every value here is invented.

use super::ExitCardModel;
use super::ExitContext;
use super::ExitCost;
use super::ExitTokens;
use crate::session_stats::SessionStats;

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
    /// A Claude session: every field, including cost.
    #[default]
    Full,
    /// A Codex session: reasoning tokens and no cost.
    Codex,
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
            Self::Full => Self::Codex,
            Self::Codex => Self::Minimal,
            Self::Minimal => Self::NotResumable,
            Self::NotResumable => Self::NoSession,
            Self::NoSession => Self::Full,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Claude, all fields",
            Self::Codex => "Codex, no cost",
            Self::Minimal => "messages and tools only",
            Self::NotResumable => "not resumable",
            Self::NoSession => "no session ID",
        }
    }
}

/// Build the fixture model for one tools/fields combination.
#[must_use]
pub fn exit_card_fixture(tools: ExitToolsFixture, fields: ExitFieldsFixture) -> ExitCardModel {
    use ExitFieldsFixture::*;

    let mut stats = SessionStats::new();
    for (name, count) in tools.tools() {
        stats.tool_calls.insert((*name).to_string(), *count);
    }
    let active = matches!(fields, Full | Codex | Minimal);
    if active {
        stats.user_messages = 14;
        stats.assistant_messages = 19;
    }
    let rich = matches!(fields, Full | Codex);
    if rich {
        stats.skills_used = vec!["test-driven-development".into(), "writing-plans".into()];
        stats.subagents_used = vec!["nori-code-researcher".into(), "nori-web-researcher".into()];
    }
    let (agent, tokens, cost) = match fields {
        Full => (
            Some("Claude Code"),
            Some(ExitTokens {
                total: 2_204_330,
                input: 18_420,
                output: 41_960,
                cached_read: Some(2_071_850),
                cached_write: Some(72_100),
                reasoning: None,
            }),
            Some(ExitCost {
                amount: 3.42,
                currency: "USD".into(),
            }),
        ),
        Codex => (
            Some("Codex"),
            Some(ExitTokens {
                total: 1_386_204,
                input: 1_302_118,
                output: 37_516,
                cached_read: Some(1_164_288),
                cached_write: None,
                reasoning: Some(46_570),
            }),
            None,
        ),
        Minimal => (Some("Claude Code"), None, None),
        NotResumable | NoSession => (None, None, None),
    };
    ExitCardModel {
        session_id: (fields != NoSession).then(|| SESSION_ID.to_string()),
        title: rich.then(|| "Redesign the exit card".to_string()),
        agent: agent.map(str::to_string),
        resumable: active,
        stats,
        context: rich.then_some(ExitContext {
            used: 164_208,
            size: if fields == Codex { 272_000 } else { 1_000_000 },
        }),
        tokens,
        cost,
    }
}
