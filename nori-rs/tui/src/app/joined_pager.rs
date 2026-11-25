//! Paging back through a joined host's session. Its full-screen view shows
//! only the newest rows, so PageUp opens the transcript a page above them.

use color_eyre::eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;

use super::App;
use crate::tui;
use crate::tui::TuiEvent;

impl App {
    /// Returns whether `key` opened the transcript.
    pub(super) async fn page_up_into_joined_transcript(
        &mut self,
        tui: &mut tui::Tui,
        key: KeyEvent,
    ) -> Result<bool> {
        if key.kind != KeyEventKind::Press
            || key.code != KeyCode::PageUp
            || self.joined.is_none()
            || !self.screen.is_full_screen()
            || self.chat_widget.has_active_popup()
        {
            return Ok(false);
        }
        self.open_transcript_overlay(tui);
        self.handle_backtrack_overlay_event(tui, TuiEvent::Key(key))
            .await
    }
}
