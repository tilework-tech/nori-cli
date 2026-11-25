//! Layout primitives for the exit card: row bodies, item flow, name
//! breaking, color stripping, and the shading band.

use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

pub(super) const INSET: usize = 2;
pub(super) const LABEL_WIDTH: usize = 11;
pub(super) const GUTTER: usize = 2;
pub(super) const VALUE_OFFSET: usize = INSET + LABEL_WIDTH + GUTTER;
const MIN_VALUE_WIDTH: usize = 12;

/// Narrowest width that keeps labels and values side by side.
pub const MIN_COLUMN_WIDTH: usize = VALUE_OFFSET + MIN_VALUE_WIDTH;

/// Widest terminals switch short lists to one line and tools to columns.
pub(super) const WIDE_WIDTH: usize = 100;

const SEPARATOR: &str = " · ";
const COUNT_GAP: usize = 2;
const COLUMN_GAP: usize = 3;
const MAX_TOOL_COLUMNS: usize = 3;
/// A name wider than this keeps the list in one column, so one long MCP name
/// cannot stretch a column of short ones.
const MAX_COLUMN_NAME_WIDTH: usize = 24;
const BREAK_AFTER: [char; 5] = ['_', '-', '/', '.', ':'];

pub(super) type Spans = Vec<Span<'static>>;

/// What sits to the right of a label.
pub(super) enum Body {
    /// Items flowing left to right, separated by a dot.
    Inline(Vec<Spans>),
    /// One item per line, or one line when `one_line` and they all fit.
    List { items: Vec<Spans>, one_line: bool },
    /// Right-justified counts, then left-justified names. Fills up to three
    /// columns, top to bottom, when `columns`, every name is short, and they
    /// fit.
    Counts {
        rows: Vec<(Spans, Spans)>,
        columns: bool,
    },
    /// A main value with labelled detail rows beneath it.
    Detail {
        main: Spans,
        fields: Vec<(&'static str, Spans)>,
    },
}

pub(super) struct Row {
    pub label: &'static str,
    pub body: Body,
}

pub(super) fn inset_for(width: usize) -> usize {
    if width >= 12 { INSET } else { 0 }
}

/// Render one labelled row. Every returned line fits in `width`.
pub(super) fn row_lines(row: &Row, width: usize) -> Vec<Line<'static>> {
    if width >= MIN_COLUMN_WIDTH {
        let label = format!(
            "{}{:<LABEL_WIDTH$}{}",
            " ".repeat(INSET),
            row.label,
            " ".repeat(GUTTER)
        );
        return body_lines(&row.body, width - VALUE_OFFSET)
            .into_iter()
            .enumerate()
            .map(|(index, spans)| {
                let lead = if index == 0 {
                    Span::styled(label.clone(), Style::new().add_modifier(Modifier::DIM))
                } else {
                    Span::from(" ".repeat(VALUE_OFFSET))
                };
                Line::from([vec![lead], spans].concat())
            })
            .collect();
    }
    let inset = inset_for(width);
    let value_indent = if width >= 8 { inset + 2 } else { inset };
    let label = vec![Span::styled(
        row.label,
        Style::new().add_modifier(Modifier::DIM),
    )];
    let mut lines = indented(flow(&[label], width - inset), inset);
    lines.extend(indented(
        body_lines(&row.body, width - value_indent),
        value_indent,
    ));
    lines
}

fn indented(rows: Vec<Spans>, indent: usize) -> Vec<Line<'static>> {
    rows.into_iter()
        .map(|spans| Line::from([vec![Span::from(" ".repeat(indent))], spans].concat()))
        .collect()
}

fn body_lines(body: &Body, avail: usize) -> Vec<Spans> {
    let avail = avail.max(1);
    match body {
        Body::Inline(items) => flow(items, avail),
        Body::List { items, one_line } => {
            let joined = items.iter().map(|item| width_of(item)).sum::<usize>()
                + SEPARATOR.width() * items.len().saturating_sub(1);
            if *one_line && joined <= avail {
                flow(items, avail)
            } else {
                items.iter().flat_map(|item| fit(item, avail)).collect()
            }
        }
        Body::Counts { rows, columns } => count_lines(rows, avail, *columns),
        Body::Detail { main, fields } => {
            let mut lines = fit(main, avail);
            let label_width = fields
                .iter()
                .map(|(label, _)| label.width())
                .max()
                .unwrap_or(0);
            let value_avail = avail.saturating_sub(label_width + COUNT_GAP);
            for (label, value) in fields {
                let label = Span::styled(
                    format!("{label:<label_width$}"),
                    Style::new().add_modifier(Modifier::DIM),
                );
                if value_avail < MIN_VALUE_WIDTH / 2 {
                    lines.extend(fit(&vec![label], avail));
                    lines.extend(fit(value, avail));
                    continue;
                }
                for (index, piece) in fit(value, value_avail).into_iter().enumerate() {
                    let lead = if index == 0 {
                        vec![label.clone(), Span::from(" ".repeat(COUNT_GAP))]
                    } else {
                        vec![Span::from(" ".repeat(label_width + COUNT_GAP))]
                    };
                    lines.push([lead, piece].concat());
                }
            }
            lines
        }
    }
}

fn count_lines(rows: &[(Spans, Spans)], avail: usize, columns: bool) -> Vec<Spans> {
    let count_width = rows
        .iter()
        .map(|(count, _)| width_of(count))
        .max()
        .unwrap_or(0);
    let pad_count = |count: &Spans| {
        let mut spans = vec![Span::from(" ".repeat(count_width - width_of(count)))];
        spans.extend(count.iter().cloned());
        spans
    };

    let short_names = rows
        .iter()
        .all(|(_, name)| width_of(name) <= MAX_COLUMN_NAME_WIDTH);
    let column_count = if columns && short_names {
        (2..=MAX_TOOL_COLUMNS)
            .rev()
            .find(|&n| {
                let height = rows.len().div_ceil(n);
                height >= 2 && table_width(rows, n, count_width) <= avail
            })
            .unwrap_or(1)
    } else {
        1
    };

    if column_count > 1 {
        let height = rows.len().div_ceil(column_count);
        let name_widths: Vec<usize> = rows
            .chunks(height)
            .map(|chunk| {
                chunk
                    .iter()
                    .map(|(_, name)| width_of(name))
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        return (0..height)
            .map(|line| {
                let mut spans = Vec::new();
                for (column, name_width) in name_widths.iter().enumerate() {
                    let Some((count, name)) = rows.get(column * height + line) else {
                        continue;
                    };
                    if column > 0 {
                        spans.push(Span::from(" ".repeat(COLUMN_GAP)));
                    }
                    spans.extend(pad_count(count));
                    spans.push(Span::from(" ".repeat(COUNT_GAP)));
                    spans.extend(name.iter().cloned());
                    let is_last = column + 1 == name_widths.len()
                        || rows.get((column + 1) * height + line).is_none();
                    if !is_last {
                        spans.push(Span::from(" ".repeat(name_width - width_of(name))));
                    }
                }
                spans
            })
            .collect();
    }

    let name_offset = count_width + COUNT_GAP;
    if avail <= name_offset {
        // Too narrow for two columns: each count, then its name, stacked.
        return rows
            .iter()
            .flat_map(|(count, name)| [fit(count, avail), fit(name, avail)].concat())
            .collect();
    }
    rows.iter()
        .flat_map(|(count, name)| {
            fit(name, avail - name_offset)
                .into_iter()
                .enumerate()
                .map(|(index, piece)| {
                    let lead = if index == 0 {
                        [pad_count(count), vec![Span::from(" ".repeat(COUNT_GAP))]].concat()
                    } else {
                        vec![Span::from(" ".repeat(name_offset))]
                    };
                    [lead, piece].concat()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn table_width(rows: &[(Spans, Spans)], columns: usize, count_width: usize) -> usize {
    let height = rows.len().div_ceil(columns);
    let names: usize = rows
        .chunks(height)
        .map(|chunk| {
            chunk
                .iter()
                .map(|(_, name)| width_of(name))
                .max()
                .unwrap_or(0)
        })
        .sum();
    let used = rows.chunks(height).count();
    used * (count_width + COUNT_GAP) + names + COLUMN_GAP * used.saturating_sub(1)
}

pub(super) fn width_of(spans: &[Span<'static>]) -> usize {
    spans.iter().map(|span| span.content.width()).sum()
}

/// One item as one or more lines no wider than `avail`.
fn fit(item: &Spans, avail: usize) -> Vec<Spans> {
    if width_of(item) <= avail {
        vec![item.clone()]
    } else {
        break_item(item, avail)
    }
}

/// Pack atomic items into lines no wider than `avail` cells.
pub(super) fn flow(items: &[Spans], avail: usize) -> Vec<Spans> {
    flow_with(items, avail, SEPARATOR)
}

pub(super) fn flow_with(items: &[Spans], avail: usize, separator: &'static str) -> Vec<Spans> {
    let avail = avail.max(1);
    let sep_width = separator.width();
    let mut lines = Vec::new();
    let mut current: Spans = Vec::new();
    let mut used = 0;
    for item in items {
        let item_width = width_of(item);
        if !current.is_empty() && used + sep_width + item_width <= avail {
            current.push(Span::styled(
                separator,
                Style::new().add_modifier(Modifier::DIM),
            ));
            current.extend(item.iter().cloned());
            used += sep_width + item_width;
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        let mut pieces = fit(item, avail);
        current = pieces.pop().unwrap_or_default();
        used = width_of(&current);
        lines.extend(pieces);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Split one item wider than `avail` into pieces that each fit. Prefer to
/// break after a name separator, then at a space, then at the column edge.
fn break_item(item: &[Span<'static>], avail: usize) -> Vec<Spans> {
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

fn spans_from_cells(cells: &[(char, Style)]) -> Spans {
    let mut spans: Spans = Vec::new();
    for &(ch, style) in cells {
        match spans.last_mut() {
            Some(span) if span.style == style => span.content.to_mut().push(ch),
            _ => spans.push(Span::styled(ch.to_string(), style)),
        }
    }
    if let Some(span) = spans.last_mut() {
        let trimmed = span.content.trim_end().to_string();
        span.content = trimmed.into();
    }
    spans.retain(|span| !span.content.is_empty());
    spans
}

/// Drop every color and keep bold and dim, for `NO_COLOR` terminals.
pub(super) fn strip_color(line: Line<'static>) -> Line<'static> {
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
pub(super) fn shade(lines: Vec<Line<'static>>, width: usize, band: Color) -> Vec<Line<'static>> {
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
        let mut spans: Spans = line
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
