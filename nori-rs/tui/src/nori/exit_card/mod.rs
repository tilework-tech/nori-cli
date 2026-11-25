//! The exit card printed into scrollback when a session ends.
//!
//! Layout rules, shared with the intro and `/status` cards:
//! - no border and no block-drawing characters;
//! - a two-cell inset, an 11-cell label column, a two-cell gutter, and every
//!   continuation line starting at the value column;
//! - two sections: what happened (session, messages, tools, skills,
//!   subagents), then what it used (context, tokens, cost);
//! - counts are right-justified in their own column with names
//!   left-justified after them. Tools fill up to three columns on wide
//!   terminals;
//! - skills and subagents take one line each, unless a wide terminal fits a
//!   few on one line;
//! - a name wider than its column breaks after `_`, `-`, `/`, `.` or `:`,
//!   then at a space, and is cut at the column edge only when it has none;
//! - below `layout::MIN_COLUMN_WIDTH` cells each label sits on its own line with its
//!   values indented beneath it;
//! - rows with nothing to show are left out;
//! - every line fits the terminal width, so the terminal never soft-wraps
//!   the card;
//! - green marks the title, the agent name takes its agent's color, and
//!   `NO_COLOR` terminals keep only bold and dim;
//! - shading is an optional band blended from the terminal background. A
//!   terminal without a known background gets no band.
//!
//! The resume command is not part of the card. The CLI prints it bare as the
//! last line of output once the TUI has exited, so the terminal wraps it as
//! one line and a triple-click selects exactly the command.

use nori_harness::AgentKind;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

use crate::history_cell::HistoryCell;
use crate::nori::token_count::format_with_thousands_separator;
use crate::presentation::session_runtime::SessionCost;
use crate::session_stats::SessionStats;

#[cfg(any(test, feature = "storybook"))]
mod fixtures;
mod layout;

use layout::Body;
use layout::Row;
use layout::Spans;

#[cfg(any(test, feature = "storybook"))]
pub use fixtures::ExitFieldsFixture;
#[cfg(any(test, feature = "storybook"))]
pub use fixtures::ExitToolsFixture;
#[cfg(feature = "storybook")]
pub use fixtures::FIXTURE_SESSION_ID;
#[cfg(any(test, feature = "storybook"))]
pub use fixtures::exit_card_fixture;

/// Context window usage from the agent's last usage update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitContext {
    pub used: i64,
    pub size: i64,
}

/// The agent a session ran with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExitAgent {
    pub name: String,
    /// `None` for custom agents, which keep the terminal foreground.
    pub kind: Option<AgentKind>,
}

/// Everything the exit card can show. Every field is optional in practice:
/// a session that never started has no ID, a quiet one has no tools.
#[derive(Clone, Debug, Default)]
pub struct ExitCardModel {
    pub session_id: Option<String>,
    /// Session title from the agent's session info updates.
    pub title: Option<String>,
    pub agent: Option<ExitAgent>,
    /// Messages, tools, skills, subagents, and summed token totals.
    pub stats: SessionStats,
    pub context: Option<ExitContext>,
    pub cost: Option<SessionCost>,
}

/// How the card is painted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExitCardPaint {
    /// Terminal background to blend the band from. `None` means no band.
    pub shade_from: Option<(u8, u8, u8)>,
    /// Keep foreground colors. Off for `NO_COLOR` terminals; bold and dim stay.
    pub color: bool,
}

#[cfg(test)]
impl ExitCardPaint {
    #[must_use]
    pub fn plain() -> Self {
        Self {
            shade_from: None,
            color: true,
        }
    }
}

/// The exit card as a history cell, painted for the current terminal.
#[derive(Debug)]
pub(crate) struct ExitCardCell {
    model: ExitCardModel,
    paint: ExitCardPaint,
}

impl ExitCardCell {
    pub(crate) fn new(model: ExitCardModel) -> Self {
        let color = supports_color::on_cached(supports_color::Stream::Stdout).is_some();
        Self {
            model,
            paint: ExitCardPaint {
                shade_from: None,
                color,
            },
        }
    }
}

impl HistoryCell for ExitCardCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        exit_card_lines(&self.model, width, self.paint)
    }
}

/// The identity color of a built-in agent's name. Agents without a brand
/// color keep the terminal foreground.
#[allow(clippy::disallowed_methods)]
pub(crate) fn agent_style(kind: Option<AgentKind>) -> Style {
    match kind {
        Some(AgentKind::ClaudeCode) => Style::new().fg(Color::Rgb(255, 158, 100)),
        Some(AgentKind::Codex) => Style::new().fg(Color::Gray),
        Some(AgentKind::GrokBuild) => Style::new().fg(Color::Blue),
        Some(AgentKind::Gemini) => Style::new().fg(Color::Yellow),
        Some(AgentKind::OpenCode | AgentKind::Cursor | AgentKind::Hermes | AgentKind::Pi)
        | None => Style::new(),
    }
}

/// Render the exit card at `width` terminal cells. No line is wider than
/// `width`.
#[must_use]
pub fn exit_card_lines(
    model: &ExitCardModel,
    width: u16,
    paint: ExitCardPaint,
) -> Vec<Line<'static>> {
    let width = usize::from(width);
    if width == 0 {
        return Vec::new();
    }
    let mut card = header_lines(width);
    for section in [activity_rows(model, width), usage_rows(model)] {
        if section.is_empty() {
            continue;
        }
        card.push(Line::default());
        for row in &section {
            card.extend(layout::row_lines(row, width));
        }
    }
    if !paint.color {
        card = card.into_iter().map(layout::strip_color).collect();
    }
    match paint.shade_from {
        Some(bg) => layout::shade(card, width, crate::style::user_message_bg(bg)),
        None => card,
    }
}

fn header_lines(width: usize) -> Vec<Line<'static>> {
    let inset = layout::inset_for(width);
    let marker = "› ";
    let items = vec![
        vec![Span::from("Nori CLI").bold()],
        vec![Span::from("session ended").dim()],
    ];
    let avail = width.saturating_sub(inset + marker.width()).max(1);
    layout::flow_with(&items, avail, "  ")
        .into_iter()
        .enumerate()
        .map(|(index, spans)| {
            let lead = if index == 0 && width > inset + marker.width() {
                vec![Span::from(" ".repeat(inset)), Span::from(marker).green()]
            } else {
                vec![Span::from(
                    " ".repeat((inset + marker.width()).min(width - 1)),
                )]
            };
            Line::from([lead, spans].concat())
        })
        .collect()
}

fn activity_rows(model: &ExitCardModel, width: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    let stats = &model.stats;
    let wide = width >= layout::WIDE_WIDTH;

    if let Some(id) = &model.session_id {
        let mut fields = Vec::new();
        if let Some(title) = &model.title {
            fields.push(("Title", vec![Span::from(title.clone()).green()]));
        }
        if let Some(agent) = &model.agent {
            fields.push((
                "Agent",
                vec![Span::styled(agent.name.clone(), agent_style(agent.kind))],
            ));
        }
        rows.push(Row {
            label: "Session",
            body: Body::Detail {
                main: vec![Span::from(id.clone())],
                fields,
            },
        });
    }

    if stats.user_messages + stats.assistant_messages > 0 {
        rows.push(Row {
            label: "Messages",
            body: Body::Inline(vec![
                counted(stats.user_messages, "prompt", "prompts"),
                counted(stats.assistant_messages, "reply", "replies"),
            ]),
        });
    }

    if !stats.tool_calls.is_empty() {
        let mut tools: Vec<_> = stats.tool_calls.iter().collect();
        tools.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        rows.push(Row {
            label: "Tools",
            body: Body::Counts {
                rows: tools
                    .into_iter()
                    .map(|(name, count)| {
                        (
                            vec![Span::from(count.to_string()).dim()],
                            vec![Span::from(name.clone())],
                        )
                    })
                    .collect(),
                columns: wide,
            },
        });
    }

    for (label, names) in [
        ("Skills", &stats.skills_used),
        ("Subagents", &stats.subagents_used),
    ] {
        if !names.is_empty() {
            rows.push(Row {
                label,
                body: Body::List {
                    items: names
                        .iter()
                        .map(|name| vec![Span::from(name.clone())])
                        .collect(),
                    one_line: wide && names.len() <= 3,
                },
            });
        }
    }
    rows
}

fn usage_rows(model: &ExitCardModel) -> Vec<Row> {
    let mut rows = Vec::new();

    if let Some(context) = model.context.filter(|context| context.size > 0) {
        let percent = context.used * 100 / context.size;
        rows.push(Row {
            label: "Context",
            body: Body::Inline(vec![
                vec![
                    Span::from(format_with_thousands_separator(context.used)),
                    Span::from(" of ").dim(),
                    Span::from(format_with_thousands_separator(context.size)),
                ],
                vec![Span::from(format!("{percent}%")), Span::from(" full").dim()],
            ]),
        });
    }

    if let Some(tokens) = model.stats.tokens.filter(|tokens| tokens.total > 0) {
        let mut lines: Vec<(Spans, Spans)> = vec![(
            vec![Span::from(format_with_thousands_separator(tokens.total)).bold()],
            vec![Span::from("total").bold()],
        )];
        let parts = [
            ("input", Some(tokens.input)),
            ("cached read", tokens.cached_read),
            ("cached write", tokens.cached_write),
            ("output", Some(tokens.output)),
            ("reasoning", tokens.reasoning),
        ];
        for (label, value) in parts {
            if let Some(value) = value.filter(|value| *value > 0) {
                lines.push((
                    vec![Span::from(format_with_thousands_separator(value))],
                    vec![Span::from(label).dim()],
                ));
            }
        }
        rows.push(Row {
            label: "Tokens",
            body: Body::Counts {
                rows: lines,
                columns: false,
            },
        });
    }

    if let Some(cost) = &model.cost {
        let amount = match cost.currency.as_str() {
            "USD" => format!("${}", cost.amount()),
            currency => format!("{} {currency}", cost.amount()),
        };
        rows.push(Row {
            label: "Cost",
            body: Body::Inline(vec![
                vec![Span::from(amount)],
                vec![Span::from("reported by the agent").dim()],
            ]),
        });
    }
    rows
}

fn counted(count: u32, one: &str, many: &str) -> Spans {
    let noun = if count == 1 { one } else { many };
    vec![
        Span::from(count.to_string()),
        Span::from(format!(" {noun}")).dim(),
    ]
}

#[cfg(test)]
mod tests;
