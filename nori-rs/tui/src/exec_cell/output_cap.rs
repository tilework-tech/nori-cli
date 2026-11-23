//! Head-and-tail cap for tool text shown inline (`tool_output_max_lines`).
//!
//! A limit of N shows at most N rows: N - 1 content lines plus one marker
//! row for the hidden middle. The head gets half of the content rows and the
//! tail gets the rest, so an odd row goes to the tail where errors and
//! summaries usually land. A limit of 1 shows only the marker. Output that
//! already fits is untouched, which means the marker always hides at least
//! two lines. `None`, 0, and negative limits show everything.

use ratatui::style::Stylize;
use ratatui::text::Line;

pub(crate) enum CappedLines<'a, T> {
    All(&'a [T]),
    Split {
        head: &'a [T],
        hidden: i32,
        tail: &'a [T],
    },
}

pub(crate) fn cap_lines<T>(lines: &[T], max_lines: Option<i32>) -> CappedLines<'_, T> {
    let Some(limit) = max_lines.filter(|limit| *limit > 0) else {
        return CappedLines::All(lines);
    };
    let total = i32::try_from(lines.len()).unwrap_or(i32::MAX);
    if total <= limit {
        return CappedLines::All(lines);
    }
    let content = limit - 1;
    let head = content / 2;
    let tail = content - head;
    let head_len = usize::try_from(head).unwrap_or_default();
    let tail_start = lines.len() - usize::try_from(tail).unwrap_or_default();
    CappedLines::Split {
        head: &lines[..head_len],
        hidden: total - content,
        tail: &lines[tail_start..],
    }
}

/// The row that stands in for the hidden middle of capped tool text.
pub(crate) fn hidden_lines_marker(hidden: i32) -> Line<'static> {
    let noun = if hidden == 1 { "line" } else { "lines" };
    Line::from(format!("┄ {hidden} {noun} hidden").dim().italic())
}
