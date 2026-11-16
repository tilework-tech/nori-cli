use std::time::Instant;

use color_eyre::eyre::Result;

use super::App;
use crate::transcript_reflow::REFLOW_DEBOUNCE;
use crate::transcript_reflow::WidthChange;
use crate::transcript_reflow::has_unconsolidated_agent_message;
use crate::tui;

impl App {
    pub(super) fn handle_resize_reflow_draw(&mut self, tui: &mut tui::Tui) -> Result<()> {
        let width = tui.terminal.size()?.width;
        if !self.config.resize_reflow {
            self.transcript_reflow.cancel();
            return Ok(());
        }

        if self.transcript_reflow.note_width(width, Instant::now()) == WidthChange::Scheduled {
            if self.chat_widget.has_active_agent_stream()
                || has_unconsolidated_agent_message(&self.transcript_cells)
            {
                self.transcript_reflow.mark_resize_requested_during_stream();
            }
            tui.frame_requester().schedule_frame_in(REFLOW_DEBOUNCE);
        }
        self.maybe_run_resize_reflow(tui, width)
    }

    fn maybe_run_resize_reflow(&mut self, tui: &mut tui::Tui, width: u16) -> Result<()> {
        let now = Instant::now();
        let Some(deadline) = self.transcript_reflow.pending_until() else {
            return Ok(());
        };
        if !self.transcript_reflow.pending_is_due(now) {
            tui.frame_requester().schedule_frame_in(deadline - now);
            return Ok(());
        }
        if self.overlay.is_some()
            || self.chat_widget.has_active_overlay_or_popup()
            || tui.is_alt_screen_active()
        {
            return Ok(());
        }
        if self.chat_widget.has_active_agent_stream()
            || has_unconsolidated_agent_message(&self.transcript_cells)
        {
            self.transcript_reflow.mark_resize_requested_during_stream();
            return Ok(());
        }
        if self.transcript_cells.is_empty() {
            self.transcript_reflow.mark_reflowed(width);
            return Ok(());
        }

        self.reprint_active_transcript(tui)?;
        Ok(())
    }
}
