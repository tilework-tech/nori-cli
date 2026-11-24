//! Reports the foreground session's state to the terminal as OSC 7501.
//!
//! The status is derived before each frame: a pending approval blocks, a
//! running turn works, and otherwise the way the last turn ended decides.
//! Parked sessions never write, the same as the terminal title.

use super::ChatWidget;
use crate::program_status::ProgramStatus;

impl ChatWidget {
    /// Write the current status if it changed since the last write.
    pub(crate) fn refresh_program_status(&mut self) {
        if !self.foreground {
            return;
        }
        let status = match self.bottom_pane.pending_approval_title() {
            Some(title) => {
                // Compare before copying the title, since this runs every frame.
                if matches!(
                    &self.last_program_status,
                    Some(ProgramStatus::Blocked { message }) if message == title
                ) {
                    return;
                }
                ProgramStatus::Blocked {
                    message: title.to_string(),
                }
            }
            None if self.bottom_pane.is_task_running() => ProgramStatus::Working,
            None => self.turn_end_status.clone(),
        };
        if self.last_program_status.as_ref() == Some(&status) {
            return;
        }
        if let Err(err) = crate::program_status::write_program_status(&status) {
            tracing::debug!(error = %err, "failed to write program status");
        }
        self.last_program_status = Some(status);
    }
}
