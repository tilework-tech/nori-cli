//! The terminal's program status follows the foreground session: whichever
//! widget is `App::chat_widget` after each event. Parked sessions keep their
//! own status current but never write it.

use super::App;

impl App {
    pub(super) fn report_program_status(&mut self) {
        let Some(reporter) = self.program_status.as_mut() else {
            return;
        };
        if let Err(error) = reporter.report_to_terminal(&self.chat_widget.program_status.status()) {
            tracing::debug!(%error, "could not report program status");
        }
    }

    /// Leave the terminal without a status when the TUI exits or detaches.
    pub(super) fn clear_program_status(&mut self) {
        let Some(reporter) = self.program_status.as_mut() else {
            return;
        };
        if let Err(error) = reporter.clear_terminal() {
            tracing::debug!(%error, "could not clear program status");
        }
    }
}
