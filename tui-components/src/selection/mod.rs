//! Selection list components and popup infrastructure.
//!
//! This module provides reusable components for creating interactive selection lists
//! with keyboard navigation, search filtering, and customizable styling.
//!
//! # Components
//!
//! - [`selection_option_row`]: Renders a single selectable row with index and marker
//! - [`standard_popup_hint_line`]: Standard footer hint for popups
//! - [`MAX_POPUP_ROWS`]: Maximum visible rows in popups
//!
//! # Examples
//!
//! ```rust
//! use tui_components::selection::selection_option_row;
//! use ratatui::buffer::Buffer;
//! use ratatui::layout::Rect;
//! use tui_components::render::Renderable;
//!
//! let row = selection_option_row(0, "Option 1".to_string(), true);
//! let area = Rect::new(0, 0, 40, 1);
//! let mut buf = Buffer::empty(area);
//! row.render(area, &mut buf);
//! ```

use crate::render::renderable::Renderable;
use crate::render::renderable::RowRenderable;
use ratatui::style::Style;
use ratatui::style::Styled as _;
use ratatui::style::Stylize as _;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use unicode_width::UnicodeWidthStr;

mod common;
mod list;

pub use common::{GenericDisplayRow, measure_rows_height, render_rows};
pub use list::{SelectionItem, SelectionList, SelectionListConfig, SelectionListEvent};

/// Maximum number of rows any popup should attempt to display.
/// Keep this consistent across all popups for a uniform feel.
pub const MAX_POPUP_ROWS: usize = 8;

/// Standard footer hint text used by popups.
///
/// Returns a line with "Press Enter to confirm or Esc to go back".
///
/// # Examples
///
/// ```rust
/// use tui_components::selection::standard_popup_hint_line;
///
/// let hint = standard_popup_hint_line();
/// ```
pub fn standard_popup_hint_line() -> Line<'static> {
    use crate::key_hint;
    use crossterm::event::KeyCode;

    Line::from(vec![
        "Press ".into(),
        key_hint::plain(KeyCode::Enter).into(),
        " to confirm or ".into(),
        key_hint::plain(KeyCode::Esc).into(),
        " to go back".into(),
    ])
}

/// Renders a single selectable option row with index and selection marker.
///
/// Creates a row with a selection marker (':' for selected, ' ' for unselected),
/// followed by the index number and the label text. The label will wrap if it
/// exceeds the available width.
///
/// # Arguments
///
/// * `index` - Zero-based index of the option (displayed as index+1)
/// * `label` - Text to display for this option
/// * `is_selected` - Whether this option is currently selected
///
/// # Returns
///
/// A boxed `Renderable` that can be rendered to a buffer.
///
/// # Examples
///
/// ```rust
/// use tui_components::selection::selection_option_row;
/// use tui_components::render::Renderable;
/// use ratatui::buffer::Buffer;
/// use ratatui::layout::Rect;
///
/// let row = selection_option_row(0, "First Option".to_string(), true);
/// let area = Rect::new(0, 0, 40, 1);
/// let mut buf = Buffer::empty(area);
/// row.render(area, &mut buf);
/// // Renders: ": 1. First Option" in cyan
/// ```
pub fn selection_option_row(
    index: usize,
    label: String,
    is_selected: bool,
) -> Box<dyn Renderable> {
    let prefix = if is_selected {
        format!(": {}. ", index + 1)
    } else {
        format!("  {}. ", index + 1)
    };
    let style = if is_selected {
        Style::default().cyan()
    } else {
        Style::default()
    };
    let prefix_width = UnicodeWidthStr::width(prefix.as_str()) as u16;
    let mut row = RowRenderable::new();
    row.push(prefix_width, prefix.set_style(style));
    row.push(
        u16::MAX,
        Paragraph::new(label)
            .style(style)
            .wrap(Wrap { trim: false }),
    );
    row.into()
}
