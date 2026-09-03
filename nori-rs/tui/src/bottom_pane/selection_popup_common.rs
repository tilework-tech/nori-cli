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

/// Indent used for description text when stacked below the name because the
/// terminal is too narrow for a side-by-side column.
const STACKED_DESC_INDENT: &str = "    ";

/// Indent used for the description line of a two-line row.
const TWO_LINE_DESC_INDENT: &str = "  ";

/// A generic representation of a display row for selection popups.
pub(crate) struct GenericDisplayRow {
    pub name: String,
    pub display_shortcut: Option<KeyBinding>,
    pub match_indices: Option<Vec<usize>>, // indices to bold (char positions)
    pub description: Option<String>,       // optional grey text after the name
    pub styled_description: Option<Line<'static>>,
    pub disabled: bool,
    /// Non-selectable section header: rendered bold with no selection cursor.
    pub is_header: bool,
    /// Render the row as two lines -- the name, then the description indented
    /// beneath it -- instead of aligning the description into the shared
    /// column. Two-line rows are excluded from the shared column measurement,
    /// so one long name cannot push every other row's description rightwards.
    pub two_line: bool,
}

/// Compute a shared description-column start based on the widest filtered name
/// plus two spaces of padding. Measuring every row keeps columns stable as the
/// viewport scrolls. Ensures at least one column remains for the description.
/// Two-line rows carry their description on their own line and are skipped, so
/// a single long name cannot widen the column for everything else.
fn compute_desc_col(rows_all: &[GenericDisplayRow], content_width: u16) -> usize {
    let max_name_width = rows_all
        .iter()
        .filter(|row| !row.two_line)
        .map(|row| Line::from(row.name.clone()).width())
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
    if let Some(desc) = row_description_line(row) {
        let gap = desc_col.saturating_sub(this_name_width);
        if gap > 0 {
            full_spans.push(" ".repeat(gap).into());
        }
        full_spans.extend(desc.spans);
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

/// Truncate a styled line to `max_width` cells, appending a single ellipsis
/// when it does not fit. Truncation is span-aware so styling (match bolding,
/// coloured markers) survives.
fn truncate_line(line: &Line<'static>, max_width: usize) -> Line<'static> {
    if line.width() <= max_width {
        return line.clone();
    }
    if max_width == 0 {
        return Line::from(Vec::new());
    }

    // Reserve one cell for the ellipsis.
    let limit = max_width - 1;
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    let mut ellipsis_style = Style::default();

    for span in &line.spans {
        ellipsis_style = span.style;
        let mut taken = String::new();
        for ch in span.content.chars() {
            let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + ch_w > limit {
                break;
            }
            used += ch_w;
            taken.push(ch);
        }
        let complete = taken.chars().count() == span.content.chars().count();
        if !taken.is_empty() {
            spans.push(Span::styled(taken, span.style));
        }
        if !complete {
            break;
        }
    }

    spans.push(Span::styled("\u{2026}", ellipsis_style));
    Line::from(spans)
}

/// Render a row as exactly two lines: the name, then its description indented
/// beneath it. Neither line wraps -- each is truncated to a single ellipsis.
fn build_two_line_row(row: &GenericDisplayRow, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![truncate_line(&build_name_line(row, width), width)];

    if let Some(desc) = row_description_line(row) {
        let indent_width = TWO_LINE_DESC_INDENT.len();
        let mut spans: Vec<Span<'static>> = vec![TWO_LINE_DESC_INDENT.into()];
        spans.extend(truncate_line(&desc, width.saturating_sub(indent_width)).spans);
        lines.push(Line::from(spans));
    }

    lines
}

/// Wrap a single row into output lines, choosing two-line, stacked, or
/// side-by-side layout.
fn wrap_row(row: &GenericDisplayRow, desc_col: usize, width: usize) -> Vec<Line<'static>> {
    if row.two_line {
        return build_two_line_row(row, width);
    }

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

        if let Some(desc_line) = row_description_line(row) {
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

fn row_description_line(row: &GenericDisplayRow) -> Option<Line<'static>> {
    row.styled_description.clone().or_else(|| {
        row.description
            .clone()
            .map(|description| Line::from(description.dim()))
    })
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

    let desc_col = compute_desc_col(rows_all, area.width);

    // The window above is item-based, but rows can be taller than one line, so
    // fitting `visible_items` items does not guarantee the selected one is
    // drawn. Advance the start until the selection fits in the available lines.
    if let Some(sel) = state.selected_idx {
        let available = area.height as usize;
        while start_idx < sel {
            let mut used = 0usize;
            let mut selection_fits = false;
            for (i, row) in rows_all
                .iter()
                .enumerate()
                .skip(start_idx)
                .take(visible_items)
            {
                let row_height = wrap_row(row, desc_col, area.width as usize).len();
                if used + row_height > available {
                    break;
                }
                used += row_height;
                if i == sel {
                    selection_fits = true;
                    break;
                }
            }
            if selection_fits {
                break;
            }
            start_idx += 1;
        }
    }

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

        if row.is_header {
            for line in &mut wrapped {
                line.spans.iter_mut().for_each(|span| {
                    span.style = span.style.bold();
                });
            }
        } else if row.disabled {
            for line in &mut wrapped {
                line.spans.iter_mut().for_each(|span| {
                    span.style = span.style.dim();
                });
            }
        } else if Some(i) == state.selected_idx {
            for line in &mut wrapped {
                line.spans.iter_mut().for_each(|span| {
                    let selected_fg = if span.style.fg == Some(Color::Red) {
                        Color::Red
                    } else {
                        Color::Green
                    };
                    span.style = Style::default().fg(selected_fg).bold();
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

    let desc_col = compute_desc_col(rows_all, width);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_row_preserves_red_symbol_in_styled_description() {
        let rows = vec![GenericDisplayRow {
            name: "/agent".to_string(),
            display_shortcut: None,
            match_indices: None,
            description: Some("recording ● on".to_string()),
            styled_description: Some(Line::from(vec!["recording ".dim(), "●".red(), " on".dim()])),
            disabled: false,
            is_header: false,
            two_line: false,
        }];
        let mut state = ScrollState::new();
        state.clamp_selection(rows.len());
        let area = Rect::new(0, 0, 40, 1);
        let mut buf = Buffer::empty(area);

        render_rows(area, &mut buf, &rows, &state, 1, "no matches");

        let dot_x = (0..area.width)
            .find(|x| buf[(*x, 0)].symbol() == "●")
            .expect("recording dot cell");
        assert_eq!(buf[(dot_x, 0)].fg, Color::Red);
    }

    #[test]
    fn selected_row_uses_green_focus_accent() {
        let rows = vec![GenericDisplayRow {
            name: "Selected action".to_string(),
            display_shortcut: None,
            match_indices: None,
            description: None,
            styled_description: None,
            disabled: false,
            is_header: false,
            two_line: false,
        }];
        let state = ScrollState {
            selected_idx: Some(0),
            scroll_top: 0,
        };
        let area = Rect::new(0, 0, 40, 1);
        let mut buffer = Buffer::empty(area);

        render_rows(area, &mut buffer, &rows, &state, 1, "no matches");

        assert_eq!(buffer[(0, 0)].fg, Color::Green);
    }

    fn marker_x(buf: &Buffer, y: u16) -> u16 {
        (0..buf.area().width)
            .find(|x| buf[(*x, y)].symbol() == "§")
            .expect("description marker")
    }

    fn buffer_text(buf: &Buffer) -> String {
        (0..buf.area().height)
            .map(|y| {
                (0..buf.area().width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn two_line_row(name: &str, description: &str) -> GenericDisplayRow {
        GenericDisplayRow {
            name: name.to_string(),
            display_shortcut: None,
            match_indices: None,
            description: Some(description.to_string()),
            styled_description: None,
            disabled: false,
            is_header: false,
            two_line: true,
        }
    }

    #[test]
    fn two_line_row_renders_name_then_indented_description() {
        let rows = vec![two_line_row("/agent:skill", "does a thing")];
        let area = Rect::new(0, 0, 40, 2);
        let mut buf = Buffer::empty(area);

        render_rows(area, &mut buf, &rows, &ScrollState::new(), 10, "no matches");

        assert_eq!(buffer_text(&buf), "/agent:skill\n  does a thing");
    }

    #[test]
    fn two_line_row_truncates_both_lines_to_one_ellipsis() {
        let rows = vec![two_line_row(
            "/agent:an-exceptionally-long-command-name",
            "a description that also runs well past the available width",
        )];
        let area = Rect::new(0, 0, 20, 2);
        let mut buf = Buffer::empty(area);

        render_rows(area, &mut buf, &rows, &ScrollState::new(), 10, "no matches");

        let text = buffer_text(&buf);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "two-line rows never wrap past two lines");
        for line in &lines {
            assert!(
                line.chars().filter(|c| *c == '\u{2026}').count() == 1,
                "expected exactly one ellipsis in {line:?}"
            );
            assert!(line.chars().count() <= area.width as usize);
        }
        assert!(lines[1].starts_with("  "));
    }

    #[test]
    fn two_line_rows_do_not_widen_the_shared_description_column() {
        let builtin = GenericDisplayRow {
            name: "/init".to_string(),
            display_shortcut: None,
            match_indices: None,
            description: Some("§ init".to_string()),
            styled_description: None,
            disabled: false,
            is_header: false,
            two_line: false,
        };
        let area = Rect::new(0, 0, 60, 3);

        let mut alone = Buffer::empty(area);
        render_rows(
            area,
            &mut alone,
            std::slice::from_ref(&builtin),
            &ScrollState::new(),
            10,
            "no matches",
        );

        let with_agent_row = vec![
            builtin,
            two_line_row("/agent:an-exceptionally-long-command-name", "desc"),
        ];
        let mut mixed = Buffer::empty(area);
        render_rows(
            area,
            &mut mixed,
            &with_agent_row,
            &ScrollState::new(),
            10,
            "no matches",
        );

        assert_eq!(marker_x(&alone, 0), marker_x(&mixed, 0));
    }

    #[test]
    fn selection_stays_visible_when_two_line_rows_exceed_the_viewport() {
        let rows: Vec<GenericDisplayRow> = (0..6)
            .map(|i| two_line_row(&format!("/agent:cmd-{i}"), &format!("description {i}")))
            .collect();
        // Six two-line rows need 12 lines; only 6 are available.
        let area = Rect::new(0, 0, 40, 6);
        let mut buf = Buffer::empty(area);
        let state = ScrollState {
            selected_idx: Some(5),
            scroll_top: 0,
        };

        render_rows(area, &mut buf, &rows, &state, 10, "no matches");

        assert!(
            buffer_text(&buf).contains("/agent:cmd-5"),
            "selected row must be drawn, got:\n{}",
            buffer_text(&buf)
        );
    }

    #[test]
    fn truncate_line_preserves_span_styling() {
        let line = Line::from(vec!["recording ".dim(), "●".red(), " on and on".dim()]);
        let truncated = truncate_line(&line, 12);

        assert_eq!(truncated.width(), 12);
        assert_eq!(
            truncated.spans.last().map(|span| span.content.as_ref()),
            Some("\u{2026}")
        );
        assert!(
            truncated
                .spans
                .iter()
                .any(|span| span.content.as_ref() == "●" && span.style.fg == Some(Color::Red)),
            "styled spans inside the kept prefix survive truncation"
        );
    }

    #[test]
    fn description_columns_stay_stable_while_scrolling() {
        let rows = vec![
            GenericDisplayRow {
                name: "exceptionally-long-command".to_string(),
                display_shortcut: None,
                match_indices: None,
                description: Some("§ first".to_string()),
                styled_description: None,
                disabled: false,
                is_header: false,
                two_line: false,
            },
            GenericDisplayRow {
                name: "a".to_string(),
                display_shortcut: None,
                match_indices: None,
                description: Some("§ second".to_string()),
                styled_description: None,
                disabled: false,
                is_header: false,
                two_line: false,
            },
            GenericDisplayRow {
                name: "b".to_string(),
                display_shortcut: None,
                match_indices: None,
                description: Some("§ third".to_string()),
                styled_description: None,
                disabled: false,
                is_header: false,
                two_line: false,
            },
        ];
        let area = Rect::new(0, 0, 50, 2);
        let mut before = Buffer::empty(area);
        let first_state = ScrollState {
            selected_idx: Some(1),
            scroll_top: 0,
        };
        render_rows(area, &mut before, &rows, &first_state, 2, "no matches");

        let mut after = Buffer::empty(area);
        let second_state = ScrollState {
            selected_idx: Some(2),
            scroll_top: 1,
        };
        render_rows(area, &mut after, &rows, &second_state, 2, "no matches");

        assert_eq!(marker_x(&before, 1), marker_x(&after, 0));
        insta::assert_snapshot!(
            "stable_description_columns_while_scrolling",
            format!(
                "before:\n{}\nafter:\n{}",
                buffer_text(&before),
                buffer_text(&after)
            )
        );
    }
}
