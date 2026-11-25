//! Design specimen for an unbordered exit card.
//!
//! Design-only: nothing in the session calls this view. It exists so the
//! storybook can show the proposed layout against fixture data before the
//! production exit card changes.
//!
//! Layout rules, shared with the intro and `/status` cards:
//! - no border and no block-drawing characters;
//! - a two-cell inset, an 11-cell label column, a two-cell gutter;
//! - each row is a list of items. Rows break only between items, and every
//!   continuation line starts at the value column;
//! - an item wider than the value column breaks after `_`, `-`, `/`, `.` or
//!   `:`, then at a space, and is cut at the column edge only when it has
//!   none of those;
//! - below [`MIN_COLUMN_WIDTH`] cells each label sits on its own line with its
//!   values indented beneath it;
//! - rows with nothing to show are left out;
//! - shading is an optional band blended from the terminal background. A
//!   terminal without a known background gets no band.

use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

use crate::session_stats::SessionStats;
use crate::ui_types::TokenUsage;
use crate::ui_types::format_si_suffix;

const INSET: usize = 2;
const LABEL_WIDTH: usize = 11;
const GUTTER: usize = 2;
const VALUE_OFFSET: usize = INSET + LABEL_WIDTH + GUTTER;
const MIN_VALUE_WIDTH: usize = 12;

/// Narrowest width that keeps labels and values side by side.
pub const MIN_COLUMN_WIDTH: usize = VALUE_OFFSET + MIN_VALUE_WIDTH;

const SEPARATOR: &str = " · ";
const BREAK_AFTER: [char; 5] = ['_', '-', '/', '.', ':'];

/// Everything the exit card can show. Every field is optional in practice:
/// a session that never started has no ID, a quiet one has no tools.
#[derive(Clone, Debug, Default)]
pub struct ExitCardModel {
    pub session_id: Option<String>,
    /// Whether `nori resume` can reopen this session.
    pub resumable: bool,
    pub stats: SessionStats,
    pub token_usage: Option<TokenUsage>,
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

/// Render the proposed exit card at `width` terminal cells. No returned line
/// is wider than `width`, so the terminal never soft-wraps the card.
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
    let mut lines = header_lines(width);
    let rows = rows(model);
    if !rows.is_empty() {
        lines.push(Line::default());
    }
    for row in rows {
        lines.extend(row_lines(&row, width));
    }
    if !paint.color {
        lines = lines.into_iter().map(strip_color).collect();
    }
    match paint.shade_from {
        Some(bg) => shade(lines, width, crate::style::user_message_bg(bg)),
        None => lines,
    }
}

struct Row {
    label: &'static str,
    items: Vec<Vec<Span<'static>>>,
}

fn header_lines(width: usize) -> Vec<Line<'static>> {
    let inset = inset_for(width);
    let marker = "› ";
    let items = vec![
        vec![Span::from("Nori CLI").bold()],
        vec![Span::from("session ended").dim()],
    ];
    let avail = width.saturating_sub(inset + marker.width()).max(1);
    flow(&items, avail, "  ")
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

fn rows(model: &ExitCardModel) -> Vec<Row> {
    let mut rows = Vec::new();
    let stats = &model.stats;

    match (&model.session_id, model.resumable) {
        (Some(id), true) => rows.push(Row {
            label: "Resume",
            items: vec![vec![Span::from(format!("nori resume {id}")).cyan()]],
        }),
        (Some(id), false) => rows.push(Row {
            label: "Session",
            items: vec![vec![Span::from(id.clone())]],
        }),
        (None, _) => {}
    }

    if stats.user_messages + stats.assistant_messages > 0 {
        rows.push(Row {
            label: "Messages",
            items: vec![
                counted(stats.user_messages, "prompt", "prompts"),
                counted(stats.assistant_messages, "reply", "replies"),
            ],
        });
    }

    if let Some(usage) = model.token_usage.as_ref().filter(|usage| !usage.is_zero()) {
        let mut items = vec![
            tokens(usage.total_tokens, "total"),
            tokens(usage.input_tokens, "in"),
        ];
        if usage.cached_input_tokens > 0 {
            items.push(tokens(usage.cached_input_tokens, "cached"));
        }
        items.push(tokens(usage.output_tokens, "out"));
        if usage.reasoning_output_tokens > 0 {
            items.push(tokens(usage.reasoning_output_tokens, "reasoning"));
        }
        rows.push(Row {
            label: "Tokens",
            items,
        });
    }

    if !stats.tool_calls.is_empty() {
        let mut tools: Vec<_> = stats.tool_calls.iter().collect();
        tools.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        rows.push(Row {
            label: "Tools",
            items: tools
                .into_iter()
                .map(|(name, count)| {
                    vec![
                        Span::from(format!("{name} ")),
                        Span::from(count.to_string()).dim(),
                    ]
                })
                .collect(),
        });
    }

    for (label, names) in [
        ("Skills", &stats.skills_used),
        ("Subagents", &stats.subagents_used),
    ] {
        if !names.is_empty() {
            rows.push(Row {
                label,
                items: names
                    .iter()
                    .map(|name| vec![Span::from(name.clone())])
                    .collect(),
            });
        }
    }
    rows
}

fn counted(count: u32, one: &str, many: &str) -> Vec<Span<'static>> {
    let noun = if count == 1 { one } else { many };
    vec![
        Span::from(count.to_string()),
        Span::from(format!(" {noun}")).dim(),
    ]
}

fn tokens(value: i64, label: &str) -> Vec<Span<'static>> {
    vec![
        Span::from(format_si_suffix(value)),
        Span::from(format!(" {label}")).dim(),
    ]
}

fn inset_for(width: usize) -> usize {
    if width >= 12 { INSET } else { 0 }
}

fn row_lines(row: &Row, width: usize) -> Vec<Line<'static>> {
    if width >= MIN_COLUMN_WIDTH {
        let avail = width - VALUE_OFFSET;
        let label = format!(
            "{}{:<LABEL_WIDTH$}{}",
            " ".repeat(INSET),
            row.label,
            " ".repeat(GUTTER)
        );
        let indent = " ".repeat(VALUE_OFFSET);
        flow(&row.items, avail, SEPARATOR)
            .into_iter()
            .enumerate()
            .map(|(index, spans)| {
                let lead = if index == 0 {
                    Span::from(label.clone()).dim()
                } else {
                    Span::from(indent.clone())
                };
                Line::from([vec![lead], spans].concat())
            })
            .collect()
    } else {
        let inset = inset_for(width);
        let value_indent = if width >= 8 { inset + 2 } else { inset };
        let mut lines: Vec<Line<'static>> = flow(
            &[vec![Span::from(row.label).dim()]],
            width - inset,
            SEPARATOR,
        )
        .into_iter()
        .map(|spans| Line::from([vec![Span::from(" ".repeat(inset))], spans].concat()))
        .collect();
        lines.extend(
            flow(&row.items, width - value_indent, SEPARATOR)
                .into_iter()
                .map(|spans| {
                    Line::from([vec![Span::from(" ".repeat(value_indent))], spans].concat())
                }),
        );
        lines
    }
}

/// Pack atomic items into lines no wider than `avail` cells.
fn flow(
    items: &[Vec<Span<'static>>],
    avail: usize,
    separator: &'static str,
) -> Vec<Vec<Span<'static>>> {
    let avail = avail.max(1);
    let sep_width = separator.width();
    let mut lines = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for item in items {
        let item_width: usize = item.iter().map(|span| span.content.width()).sum();
        if !current.is_empty() && used + sep_width + item_width <= avail {
            current.push(Span::from(separator).dim());
            current.extend(item.iter().cloned());
            used += sep_width + item_width;
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if item_width <= avail {
            current = item.clone();
            used = item_width;
        } else {
            let mut pieces = break_item(item, avail);
            let last = pieces.pop().unwrap_or_default();
            lines.extend(pieces);
            used = last.iter().map(|span| span.content.width()).sum();
            current = last;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Split one item wider than `avail` into pieces that each fit, preferring to
/// break after a name separator so the parts stay readable.
fn break_item(item: &[Span<'static>], avail: usize) -> Vec<Vec<Span<'static>>> {
    let cells: Vec<(char, Style)> = item
        .iter()
        .flat_map(|span| span.content.chars().map(move |ch| (ch, span.style)))
        .collect();
    let mut pieces = Vec::new();
    let mut start = 0;
    while start < cells.len() {
        let mut end = start;
        let mut used = 0;
        let mut after_separator = None;
        let mut after_space = None;
        while end < cells.len() {
            let ch_width = cells[end].0.width().unwrap_or(0);
            if used + ch_width > avail && end > start {
                break;
            }
            used += ch_width;
            end += 1;
            match cells[end - 1].0 {
                ' ' => after_space = Some(end),
                ch if BREAK_AFTER.contains(&ch) => after_separator = Some(end),
                _ => {}
            }
        }
        // A name separator beats a space, so a count stays with its name.
        let cut = if end < cells.len() {
            after_separator.or(after_space).unwrap_or(end)
        } else {
            end
        };
        pieces.push(spans_from_cells(&cells[start..cut]));
        start = cut;
        while start < cells.len() && cells[start].0 == ' ' {
            start += 1;
        }
    }
    pieces
}

fn spans_from_cells(cells: &[(char, Style)]) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for &(ch, style) in cells {
        match spans.last_mut() {
            Some(span) if span.style == style => span.content.to_mut().push(ch),
            _ => spans.push(Span::styled(ch.to_string(), style)),
        }
    }
    // A trailing space at a break point carries no meaning on screen.
    if let Some(span) = spans.last_mut() {
        let trimmed = span.content.trim_end().to_string();
        span.content = trimmed.into();
    }
    spans.retain(|span| !span.content.is_empty());
    spans
}

fn strip_color(line: Line<'static>) -> Line<'static> {
    let spans = line
        .spans
        .into_iter()
        .map(|span| {
            let mut style = Style::default();
            style.add_modifier = span.style.add_modifier & (Modifier::BOLD | Modifier::DIM);
            Span::styled(span.content, style)
        })
        .collect::<Vec<_>>();
    Line::from(spans)
}

/// Paint every line as one band hugging the widest line, with a blank band
/// row above and below.
fn shade(lines: Vec<Line<'static>>, width: usize, band: Color) -> Vec<Line<'static>> {
    let content = lines.iter().map(Line::width).max().unwrap_or(0);
    let band_width = (content + INSET).min(width);
    let pad_row = || {
        Line::from(Span::styled(
            " ".repeat(band_width),
            Style::default().bg(band),
        ))
    };
    let mut shaded = vec![pad_row()];
    for line in lines {
        let pad = band_width.saturating_sub(line.width());
        let mut spans: Vec<Span<'static>> = line
            .spans
            .into_iter()
            .map(|span| {
                let style = span.style.bg(band);
                Span::styled(span.content, style)
            })
            .collect();
        if pad > 0 {
            spans.push(Span::styled(" ".repeat(pad), Style::default().bg(band)));
        }
        shaded.push(Line::from(spans));
    }
    shaded.push(pad_row());
    shaded
}

mod fixtures;

pub use fixtures::ExitFieldsFixture;
pub use fixtures::ExitToolsFixture;
pub use fixtures::exit_card_fixture;

#[cfg(test)]
mod tests;
