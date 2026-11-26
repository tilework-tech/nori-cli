//! Shared popup-related constants for bottom pane widgets.

use crossterm::event::KeyCode;
use ratatui::text::Line;

use crate::key_hint;

/// Maximum number of rows any popup should attempt to display.
/// Keep this consistent across all popups for a uniform feel.
pub(crate) const MAX_POPUP_ROWS: usize = 8;

/// Maximum number of *commands* the slash-command popup shows at once. Counted
/// in commands, not terminal lines, so a two-line agent command still costs one
/// slot -- the list stays predictable regardless of how the rows are laid out.
pub(crate) const MAX_COMMAND_POPUP_ROWS: usize = 10;

/// Standard footer hint text used by non-searchable popups.
/// Includes j/k vim-style navigation hint.
pub(crate) fn standard_popup_hint_line() -> Line<'static> {
    Line::from(vec![
        "↑/k ↓/j to navigate, ".into(),
        key_hint::plain(KeyCode::Enter).into(),
        " to confirm, ".into(),
        key_hint::plain(KeyCode::Esc).into(),
        " to go back".into(),
    ])
}
