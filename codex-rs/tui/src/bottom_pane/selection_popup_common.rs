use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
// Note: Table-based layout previously used Constraint; the manual renderer
// below no longer requires it.
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthChar;

use crate::key_hint::KeyBinding;
use crate::render::line_utils::line_to_static;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_line;

use super::scroll_state::ScrollState;

/// Minimum number of columns the description needs to be rendered side-by-side
/// with the name. Below this threshold, the description is stacked below the
/// name on its own line(s) instead.
const MIN_DESC_COLUMNS: usize = 12;

/// Indent used for description text when stacked below the name.
const STACKED_DESC_INDENT: &str = "    ";

/// A generic representation of a display row for selection popups.
pub(crate) struct GenericDisplayRow {
    pub name: String,
    pub display_shortcut: Option<KeyBinding>,
    pub match_indices: Option<Vec<usize>>, // indices to bold (char positions)
    pub description: Option<String>,       // optional grey text after the name
}

/// Compute a shared description-column start based on the widest visible name
/// plus two spaces of padding. Ensures at least one column is left for the
/// description.
fn compute_desc_col(
    rows_all: &[GenericDisplayRow],
    start_idx: usize,
    visible_items: usize,
    content_width: u16,
) -> usize {
    let visible_range = start_idx..(start_idx + visible_items);
    let max_name_width = rows_all
        .iter()
        .enumerate()
        .filter(|(i, _)| visible_range.contains(i))
        .map(|(_, r)| Line::from(r.name.clone()).width())
        .max()
        .unwrap_or(0);
    let mut desc_col = max_name_width.saturating_add(2);
    if (desc_col as u16) >= content_width {
        desc_col = content_width.saturating_sub(1) as usize;
    }
    desc_col
}

/// Returns true if the description should be stacked below the name rather than
/// placed side-by-side, because there isn't enough horizontal room.
fn should_stack_description(desc_col: usize, total_width: usize) -> bool {
    total_width.saturating_sub(desc_col) < MIN_DESC_COLUMNS
}

/// Build the name-only portion of a row (no description). Used for both
/// side-by-side and stacked layouts.
fn build_name_spans(row: &GenericDisplayRow, name_limit: usize) -> (Vec<Span<'static>>, bool) {
    let mut name_spans: Vec<Span> = Vec::with_capacity(row.name.len());
    let mut used_width = 0usize;
    let mut truncated = false;

    if let Some(idxs) = row.match_indices.as_ref() {
        let mut idx_iter = idxs.iter().peekable();
        for (char_idx, ch) in row.name.chars().enumerate() {
            let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0);
            if used_width + ch_w > name_limit {
                truncated = true;
                break;
            }
            used_width += ch_w;

            if idx_iter.peek().is_some_and(|next| **next == char_idx) {
                idx_iter.next();
                name_spans.push(ch.to_string().bold());
            } else {
                name_spans.push(ch.to_string().into());
            }
        }
    } else {
        for ch in row.name.chars() {
            let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0);
            if used_width + ch_w > name_limit {
                truncated = true;
                break;
            }
            used_width += ch_w;
            name_spans.push(ch.to_string().into());
        }
    }

    if truncated {
        name_spans.push("…".into());
    }

    (name_spans, truncated)
}

/// Build the full display line for a row with the description padded to start
/// at `desc_col`. Applies fuzzy-match bolding when indices are present and
/// dims the description.
fn build_full_line(row: &GenericDisplayRow, desc_col: usize) -> Line<'static> {
    // Enforce single-line name: allow at most desc_col - 2 cells for name,
    // reserving two spaces before the description column.
    let name_limit = desc_col.saturating_sub(2);

    let (name_spans, _truncated) = build_name_spans(row, name_limit);

    let this_name_width = Line::from(name_spans.clone()).width();
    let mut full_spans: Vec<Span> = name_spans;
    if let Some(display_shortcut) = row.display_shortcut {
        full_spans.push(" (".into());
        full_spans.push(display_shortcut.into());
        full_spans.push(")".into());
    }
    if let Some(desc) = row.description.as_ref() {
        let gap = desc_col.saturating_sub(this_name_width);
        if gap > 0 {
            full_spans.push(" ".repeat(gap).into());
        }
        full_spans.push(desc.clone().dim());
    }
    Line::from(full_spans)
}

/// Build a name-only line for stacked layout (no description appended).
fn build_name_line(row: &GenericDisplayRow, width: usize) -> Line<'static> {
    let name_limit = width.saturating_sub(1);
    let (mut name_spans, _truncated) = build_name_spans(row, name_limit);
    if let Some(display_shortcut) = row.display_shortcut {
        name_spans.push(" (".into());
        name_spans.push(display_shortcut.into());
        name_spans.push(")".into());
    }
    Line::from(name_spans)
}

/// Wrap a single row into output lines, choosing stacked or side-by-side layout.
fn wrap_row(row: &GenericDisplayRow, desc_col: usize, width: usize) -> Vec<Line<'static>> {
    let stacked = row.description.is_some() && should_stack_description(desc_col, width);

    if stacked {
        let name_line = build_name_line(row, width);
        let name_opts = RtOptions::new(width)
            .initial_indent(Line::from(""))
            .subsequent_indent("  ".into());
        let mut lines: Vec<Line<'static>> = word_wrap_line(&name_line, name_opts)
            .iter()
            .map(line_to_static)
            .collect();

        if let Some(desc) = row.description.as_ref() {
            let desc_line: Line<'static> = Line::from(desc.clone().dim());
            let desc_opts = RtOptions::new(width)
                .initial_indent(STACKED_DESC_INDENT.dim().into())
                .subsequent_indent(STACKED_DESC_INDENT.dim().into());
            lines.extend(
                word_wrap_line(&desc_line, desc_opts)
                    .iter()
                    .map(line_to_static),
            );
        }
        lines
    } else {
        let full_line = build_full_line(row, desc_col);
        let options = RtOptions::new(width)
            .initial_indent(Line::from(""))
            .subsequent_indent(Line::from(" ".repeat(desc_col)));
        word_wrap_line(&full_line, options)
            .iter()
            .map(line_to_static)
            .collect()
    }
}

/// Render a list of rows using the provided ScrollState, with shared styling
/// and behavior for selection popups.
pub(crate) fn render_rows(
    area: Rect,
    buf: &mut Buffer,
    rows_all: &[GenericDisplayRow],
    state: &ScrollState,
    max_results: usize,
    empty_message: &str,
) {
    if rows_all.is_empty() {
        if area.height > 0 {
            Line::from(empty_message.dim().italic()).render(area, buf);
        }
        return;
    }

    // Determine which logical rows (items) are visible given the selection and
    // the max_results clamp. Scrolling is still item-based for simplicity.
    let visible_items = max_results
        .min(rows_all.len())
        .min(area.height.max(1) as usize);

    let mut start_idx = state.scroll_top.min(rows_all.len().saturating_sub(1));
    if let Some(sel) = state.selected_idx {
        if sel < start_idx {
            start_idx = sel;
        } else if visible_items > 0 {
            let bottom = start_idx + visible_items - 1;
            if sel > bottom {
                start_idx = sel + 1 - visible_items;
            }
        }
    }

    let desc_col = compute_desc_col(rows_all, start_idx, visible_items, area.width);

    // Render items, wrapping descriptions and aligning wrapped lines under the
    // shared description column. Stop when we run out of vertical space.
    let mut cur_y = area.y;
    for (i, row) in rows_all
        .iter()
        .enumerate()
        .skip(start_idx)
        .take(visible_items)
    {
        if cur_y >= area.y + area.height {
            break;
        }

        let mut wrapped = wrap_row(row, desc_col, area.width as usize);

        if Some(i) == state.selected_idx {
            for line in &mut wrapped {
                line.spans.iter_mut().for_each(|span| {
                    span.style = Style::default().fg(Color::Cyan).bold();
                });
            }
        }

        for line in wrapped {
            if cur_y >= area.y + area.height {
                break;
            }
            line.render(
                Rect {
                    x: area.x,
                    y: cur_y,
                    width: area.width,
                    height: 1,
                },
                buf,
            );
            cur_y = cur_y.saturating_add(1);
        }
    }
}

/// Compute the number of terminal rows required to render up to `max_results`
/// items from `rows_all` given the current scroll/selection state and the
/// available `width`. Accounts for description wrapping and alignment so the
/// caller can allocate sufficient vertical space.
pub(crate) fn measure_rows_height(
    rows_all: &[GenericDisplayRow],
    state: &ScrollState,
    max_results: usize,
    width: u16,
) -> u16 {
    if rows_all.is_empty() {
        return 1; // placeholder "no matches" line
    }

    let visible_items = max_results.min(rows_all.len());
    let mut start_idx = state.scroll_top.min(rows_all.len().saturating_sub(1));
    if let Some(sel) = state.selected_idx {
        if sel < start_idx {
            start_idx = sel;
        } else if visible_items > 0 {
            let bottom = start_idx + visible_items - 1;
            if sel > bottom {
                start_idx = sel + 1 - visible_items;
            }
        }
    }

    let desc_col = compute_desc_col(rows_all, start_idx, visible_items, width);

    let mut total: u16 = 0;
    for row in rows_all
        .iter()
        .enumerate()
        .skip(start_idx)
        .take(visible_items)
        .map(|(_, r)| r)
    {
        total = total.saturating_add(wrap_row(row, desc_col, width as usize).len() as u16);
    }
    total.max(1)
}
