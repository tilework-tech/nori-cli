//! What the terminal shows for the active session.
//!
//! The scrollback belongs to one session at a time. Showing a session clears
//! the screen and the terminal scrollback, then reprints that session's
//! finished cells. Resize reflow and session jumps share this path.

use color_eyre::eyre::Result;

use super::App;
use crate::transcript_reflow::render_transcript_tail;
use crate::tui;

impl App {
    /// Clear the terminal, including its scrollback, and reprint the active
    /// session's finished cells at the current width. The in-progress turn
    /// stays in the viewport, because it lives in the active widget and not
    /// in the transcript.
    pub(super) fn reprint_active_transcript(&mut self, tui: &mut tui::Tui) -> Result<()> {
        let width = tui.terminal.size()?.width;
        let lines = render_transcript_tail(&self.transcript_cells, width);
        tui.clear_pending_history_lines();
        tui.terminal
            .clear_scrollback_and_visible_screen_for_reflow()?;
        let mut area = tui.terminal.viewport_area;
        area.y = 0;
        area.width = width;
        tui.terminal.set_viewport_area(area);
        self.deferred_history_lines.clear();
        self.has_emitted_history_lines = !lines.is_empty();
        if !lines.is_empty() {
            tui.insert_history_lines(lines);
        }
        self.transcript_reflow.mark_reflowed(width);
        Ok(())
    }

    /// Start an empty transcript for a session that replaces the active one
    /// and will replay its own history, such as a resumed session. The
    /// previous session's cells leave the transcript and the terminal.
    pub(super) fn begin_replacing_session_transcript(&mut self, tui: &mut tui::Tui) -> Result<()> {
        self.transcript_cells.clear();
        self.backtrack = crate::app_backtrack::BacktrackState::default();
        self.reprint_active_transcript(tui)
    }
}
