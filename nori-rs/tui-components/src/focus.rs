//! The focused-row style shared by every list renderer in this crate.
//!
//! A focused row carries three marks: green rails at both edges, a green
//! `›` pointer in its own column, and a shaded background when the theme
//! knows the terminal background. Without shading, the pointer and rails
//! turn bold so they stay visible against the neighbouring rows. Row
//! numbers and multiple-choice dots sit in their own columns after the
//! pointer and never replace it.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;

use crate::Theme;

pub(crate) const POINTER: &str = "›";
const LEFT_RAIL: &str = "▏";
const RIGHT_RAIL: &str = "▕";

/// Cells taken by the pointer and the space after it.
pub(crate) const POINTER_WIDTH: u16 = 2;

/// Style for the pointer and rails, patched over the row's own style.
pub(crate) fn mark_style(row_style: Style, theme: Theme) -> Style {
    let style = row_style.patch(theme.pointer);
    if theme.selected.bg.is_some() {
        style.remove_modifier(Modifier::BOLD)
    } else {
        style.add_modifier(Modifier::BOLD)
    }
}

/// Draws the rails on every line of a focused row.
pub(crate) fn render_rails(area: Rect, buf: &mut Buffer, style: Style) {
    for y in area.y..area.bottom() {
        buf.set_string(area.x, y, LEFT_RAIL, style);
        if area.width > 1 {
            buf.set_string(area.right().saturating_sub(1), y, RIGHT_RAIL, style);
        }
    }
}

#[cfg(test)]
mod tests;
