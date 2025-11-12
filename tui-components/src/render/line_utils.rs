//! Utilities for working with Ratatui text lines.
//!
//! This module provides helper functions for manipulating and analyzing
//! Ratatui's [`Line`] and [`Span`] types, commonly needed when building
//! text-based TUI components.

use ratatui::text::Line;
use ratatui::text::Span;

/// Clones a borrowed Ratatui [`Line`] into an owned `'static` line.
///
/// This is useful when you need to store lines beyond the lifetime of
/// the source data, such as in caches or persistent state.
///
/// # Examples
///
/// ```rust
/// use codex_tui_components::render::line_utils::line_to_static;
/// use ratatui::text::Line;
///
/// let borrowed_line = Line::from("Hello");
/// let owned_line = line_to_static(&borrowed_line);
/// // owned_line can now outlive borrowed_line
/// ```
pub fn line_to_static(line: &Line<'_>) -> Line<'static> {
    Line {
        style: line.style,
        alignment: line.alignment,
        spans: line
            .spans
            .iter()
            .map(|s| Span {
                style: s.style,
                content: std::borrow::Cow::Owned(s.content.to_string()),
            })
            .collect(),
    }
}

/// Appends owned copies of borrowed lines to the output vector.
///
/// This is a convenience function for batch-converting borrowed lines
/// to owned lines and appending them to a collection.
///
/// # Examples
///
/// ```rust
/// use codex_tui_components::render::line_utils::push_owned_lines;
/// use ratatui::text::Line;
///
/// let source = vec![Line::from("Line 1"), Line::from("Line 2")];
/// let mut output = Vec::new();
/// push_owned_lines(&source, &mut output);
/// assert_eq!(output.len(), 2);
/// ```
pub fn push_owned_lines<'a>(src: &[Line<'a>], out: &mut Vec<Line<'static>>) {
    for l in src {
        out.push(line_to_static(l));
    }
}

/// Checks if a line is blank (empty or contains only spaces).
///
/// A line is considered blank if:
/// - It has no spans, or
/// - All spans are empty or contain only space characters (no tabs or newlines)
///
/// This is useful for trimming whitespace or detecting empty content.
///
/// # Examples
///
/// ```rust
/// use codex_tui_components::render::line_utils::is_blank_line_spaces_only;
/// use ratatui::text::Line;
///
/// assert!(is_blank_line_spaces_only(&Line::from("")));
/// assert!(is_blank_line_spaces_only(&Line::from("   ")));
/// assert!(!is_blank_line_spaces_only(&Line::from("text")));
/// assert!(!is_blank_line_spaces_only(&Line::from("\t"))); // tabs don't count
/// ```
pub fn is_blank_line_spaces_only(line: &Line<'_>) -> bool {
    if line.spans.is_empty() {
        return true;
    }
    line.spans
        .iter()
        .all(|s| s.content.is_empty() || s.content.chars().all(|c| c == ' '))
}

/// Prefixes each line with different spans for the first and subsequent lines.
///
/// This is commonly used for creating indented or bulleted lists where the
/// first line has a different prefix (e.g., "• ") than subsequent lines (e.g., "  ").
///
/// # Arguments
///
/// * `lines` - The lines to prefix
/// * `initial_prefix` - The prefix for the first line
/// * `subsequent_prefix` - The prefix for all other lines
///
/// # Returns
///
/// A new vector of lines with prefixes applied
///
/// # Examples
///
/// ```rust
/// use codex_tui_components::render::line_utils::prefix_lines;
/// use ratatui::text::{Line, Span};
///
/// let lines = vec![
///     Line::from("First line"),
///     Line::from("Second line"),
/// ];
///
/// let prefixed = prefix_lines(
///     lines,
///     Span::raw("• "),
///     Span::raw("  "),
/// );
///
/// // First line starts with "• ", subsequent lines with "  "
/// ```
pub fn prefix_lines(
    lines: Vec<Line<'static>>,
    initial_prefix: Span<'static>,
    subsequent_prefix: Span<'static>,
) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            let mut spans = Vec::with_capacity(l.spans.len() + 1);
            spans.push(if i == 0 {
                initial_prefix.clone()
            } else {
                subsequent_prefix.clone()
            });
            spans.extend(l.spans);
            Line::from(spans).style(l.style)
        })
        .collect()
}
