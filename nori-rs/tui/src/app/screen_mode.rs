//! `/screen`: the inline view or the full-screen view of the same app.
//!
//! Inline, finished history goes into the terminal's own scrollback above a
//! bottom viewport. Full-screen, the app draws on the alternate screen: the
//! active session's transcript fills the space above the chat widget, and
//! history the inline view would print is held instead. Returning inline
//! prints the held lines once, in order, so the scrollback reads as if the
//! app had never left. Overlays (the transcript pager, the diff pager) hold
//! history the same way while they cover the screen.

use color_eyre::eyre::Result;
use ratatui::text::Line;

use super::App;
use super::full_screen_frame::TranscriptRows;
use super::full_screen_frame::render_full_screen;
use crate::tui;

#[derive(Default)]
pub(crate) struct ScreenMode {
    full_screen: bool,
    held: Vec<Line<'static>>,
    transcript_rows: TranscriptRows,
}

impl ScreenMode {
    pub(crate) fn is_full_screen(&self) -> bool {
        self.full_screen
    }

    pub(crate) fn set_full_screen(&mut self, full_screen: bool) {
        self.full_screen = full_screen;
    }

    /// History lines for the inline scrollback. Returns the lines to print
    /// now; while the full-screen view or an overlay covers the screen they
    /// are held and nothing is returned.
    pub(crate) fn print_or_hold(
        &mut self,
        lines: Vec<Line<'static>>,
        overlay_open: bool,
    ) -> Vec<Line<'static>> {
        if self.full_screen || overlay_open || !self.held.is_empty() {
            self.held.extend(lines);
            return self.release(overlay_open);
        }
        lines
    }

    /// The held lines, once nothing covers the screen any more. Returns
    /// nothing while the screen is still covered.
    pub(crate) fn release(&mut self, overlay_open: bool) -> Vec<Line<'static>> {
        if self.full_screen || overlay_open {
            return Vec::new();
        }
        std::mem::take(&mut self.held)
    }

    /// Drop the held lines because the whole transcript is about to be
    /// reprinted.
    pub(crate) fn discard_held(&mut self) {
        self.held.clear();
    }
}

impl App {
    /// `/screen`: switch between the inline and the full-screen view.
    pub(super) fn toggle_screen_mode(&mut self, tui: &mut tui::Tui) -> Result<()> {
        if self.screen.is_full_screen() {
            self.leave_full_screen(tui)?;
        } else {
            tui.enter_alt_screen()?;
            self.screen.set_full_screen(true);
        }
        tui.frame_requester().schedule_frame();
        Ok(())
    }

    /// Return to the inline view, then print the history held meanwhile.
    pub(super) fn leave_full_screen(&mut self, tui: &mut tui::Tui) -> Result<()> {
        if !self.screen.is_full_screen() {
            return Ok(());
        }
        self.screen.set_full_screen(false);
        tui.leave_alt_screen()?;
        // The normal screen still shows the inline viewport from before;
        // redraw all of it.
        tui.terminal.clear()?;
        self.release_held_history(tui);
        Ok(())
    }

    /// On exit, close any overlay and leave the alternate screen however it
    /// is held, so the held history and the exit message reach the inline
    /// scrollback.
    pub(super) fn return_inline_for_exit(&mut self, tui: &mut tui::Tui) -> Result<()> {
        if self.overlay.is_some() {
            self.close_transcript_overlay(tui);
        }
        self.screen.set_full_screen(false);
        if tui.leave_alt_screen_entirely() {
            tui.terminal.clear()?;
            // No later draw runs the reprint deferred while full-screen.
            if self.transcript_reflow.has_pending_reflow() || self.session_reprint_pending {
                self.session_reprint_pending = false;
                self.screen.discard_held();
                return self.reprint_active_transcript(tui);
            }
        }
        self.release_held_history(tui);
        Ok(())
    }

    /// Print history held while the screen was covered, unless the whole
    /// transcript is about to be reprinted anyway.
    pub(crate) fn release_held_history(&mut self, tui: &mut tui::Tui) {
        if self.transcript_reflow.has_pending_reflow() || self.session_reprint_pending {
            self.screen.discard_held();
            tui.frame_requester().schedule_frame();
            return;
        }
        let lines = self.screen.release(self.overlay.is_some());
        if !lines.is_empty() {
            tui.insert_history_lines(lines);
        }
    }

    /// Draw one full-screen frame of the session on screen.
    pub(super) fn draw_full_screen(&mut self, tui: &mut tui::Tui) -> Result<()> {
        let width = tui.terminal.size()?.width;
        self.screen
            .transcript_rows
            .update(&self.transcript_cells, width);
        let rows = &self.screen.transcript_rows;
        let chat_widget = &self.chat_widget;
        tui.draw(u16::MAX, |frame| {
            if let Some(position) =
                render_full_screen(frame.area(), frame.buffer, rows, chat_widget)
            {
                frame.set_cursor_position(position);
            }
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ScreenMode;
    use pretty_assertions::assert_eq;
    use ratatui::text::Line;

    fn lines(texts: &[&'static str]) -> Vec<Line<'static>> {
        texts.iter().map(|text| Line::from(*text)).collect()
    }

    #[test]
    fn inline_history_prints_at_once() {
        let mut screen = ScreenMode::default();
        assert_eq!(
            screen.print_or_hold(lines(&["one", "two"]), false),
            lines(&["one", "two"])
        );
        assert_eq!(screen.release(false), lines(&[]));
    }

    #[test]
    fn full_screen_history_prints_once_in_order_after_returning_inline() {
        let mut screen = ScreenMode::default();
        screen.set_full_screen(true);
        assert_eq!(screen.print_or_hold(lines(&["one"]), false), lines(&[]));
        assert_eq!(
            screen.print_or_hold(lines(&["two", "three"]), false),
            lines(&[])
        );
        assert_eq!(
            screen.release(false),
            lines(&[]),
            "nothing prints while still full-screen"
        );

        screen.set_full_screen(false);
        assert_eq!(screen.release(false), lines(&["one", "two", "three"]));
        assert_eq!(screen.release(false), lines(&[]), "printed exactly once");
    }

    #[test]
    fn closing_an_overlay_over_the_full_screen_view_keeps_history_held() {
        let mut screen = ScreenMode::default();
        assert_eq!(screen.print_or_hold(lines(&["before"]), true), lines(&[]));
        screen.set_full_screen(true);
        assert_eq!(screen.print_or_hold(lines(&["during"]), true), lines(&[]));
        assert_eq!(screen.release(false), lines(&[]), "the overlay closed");
        assert_eq!(screen.print_or_hold(lines(&["after"]), false), lines(&[]));

        screen.set_full_screen(false);
        assert_eq!(screen.release(false), lines(&["before", "during", "after"]));
    }

    #[test]
    fn an_overlay_over_the_inline_view_holds_history_until_it_closes() {
        let mut screen = ScreenMode::default();
        assert_eq!(screen.print_or_hold(lines(&["held"]), true), lines(&[]));
        assert_eq!(
            screen.release(true),
            lines(&[]),
            "the overlay is still open"
        );
        assert_eq!(screen.release(false), lines(&["held"]));
    }

    #[test]
    fn a_reprint_discards_held_history() {
        let mut screen = ScreenMode::default();
        screen.set_full_screen(true);
        screen.print_or_hold(lines(&["stale"]), false);
        screen.discard_held();
        screen.set_full_screen(false);
        assert_eq!(screen.release(false), lines(&[]));
    }
}
