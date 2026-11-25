//! Design specimen for an unbordered exit card.
//!
//! Design-only: nothing in the session calls this view. It exists so the
//! storybook can show the proposed layout against fixture data before the
//! production exit card changes.
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
//! - below [`MIN_COLUMN_WIDTH`] cells each label sits on its own line with its
//!   values indented beneath it;
//! - rows with nothing to show are left out;
//! - the last line is the bare resume command, unindented and unwrapped, so
//!   a triple-click selects exactly the command. Every other line fits the
//!   terminal width;
//! - shading is an optional band blended from the terminal background. A
//!   terminal without a known background gets no band. The resume command
//!   sits outside the band.

use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

use crate::nori::token_count::format_with_thousands_separator;
use crate::session_stats::SessionStats;

mod fixtures;
mod layout;

use layout::Body;
use layout::Row;
use layout::Spans;

pub use fixtures::ExitFieldsFixture;
pub use fixtures::ExitToolsFixture;
pub use fixtures::exit_card_fixture;
pub use layout::MIN_COLUMN_WIDTH;

/// Context window usage from the agent's last usage update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitContext {
    pub used: i64,
    pub size: i64,
}

/// Session token totals summed from the agent's per-turn usage reports.
/// Optional fields are the ones agents may leave out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExitTokens {
    pub total: i64,
    pub input: i64,
    pub output: i64,
    pub cached_read: Option<i64>,
    pub cached_write: Option<i64>,
    pub reasoning: Option<i64>,
}

/// Cumulative session cost, when the agent reports one.
#[derive(Clone, Debug, PartialEq)]
pub struct ExitCost {
    pub amount: f64,
    /// ISO 4217 code, such as `USD`.
    pub currency: String,
}

/// Everything the exit card can show. Every field is optional in practice:
/// a session that never started has no ID, a quiet one has no tools.
#[derive(Clone, Debug, Default)]
pub struct ExitCardModel {
    pub session_id: Option<String>,
    /// Session title from the agent's session info updates.
    pub title: Option<String>,
    /// Display name of the agent the session ran with.
    pub agent: Option<String>,
    /// Whether `nori resume` can reopen this session.
    pub resumable: bool,
    pub stats: SessionStats,
    pub context: Option<ExitContext>,
    pub tokens: Option<ExitTokens>,
    pub cost: Option<ExitCost>,
}

/// How the card is painted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExitCardPaint {
    /// Terminal background to blend the band from. `None` means no band.
    pub shade_from: Option<(u8, u8, u8)>,
    /// Keep foreground colors. Off for `NO_COLOR` terminals; bold and dim stay.
    pub color: bool,
}

impl ExitCardPaint {
    #[must_use]
    pub fn plain() -> Self {
        Self {
            shade_from: None,
            color: true,
        }
    }
}

/// The shell command that reopens a session.
#[must_use]
pub fn resume_command(session_id: &str) -> String {
    format!("nori resume {session_id}")
}

/// Render the proposed exit card at `width` terminal cells.
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

    let mut resume = match (&model.session_id, model.resumable) {
        (Some(id), true) => vec![
            Line::default(),
            Line::from(Span::from(resume_command(id)).green()),
        ],
        (Some(_), false) | (None, _) => Vec::new(),
    };
    if !paint.color {
        card = card.into_iter().map(layout::strip_color).collect();
        resume = resume.into_iter().map(layout::strip_color).collect();
    }
    if let Some(bg) = paint.shade_from {
        card = layout::shade(card, width, crate::style::user_message_bg(bg));
    }
    card.extend(resume);
    card
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
        let fields = [("Title", &model.title), ("Agent", &model.agent)]
            .into_iter()
            .filter_map(|(label, value)| {
                value
                    .as_ref()
                    .map(|value| (label, vec![Span::from(value.clone())]))
            })
            .collect();
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

    if let Some(tokens) = model.tokens.filter(|tokens| tokens.total > 0) {
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
            "USD" => format!("${:.2}", cost.amount),
            currency => format!("{:.2} {currency}", cost.amount),
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
