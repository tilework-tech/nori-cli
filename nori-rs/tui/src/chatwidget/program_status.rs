//! Reports the foreground session's state to the terminal as OSC 7501.
//!
//! The status is derived on each refresh: a pending approval blocks, a
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
        let status = if let Some(title) = self.bottom_pane.pending_approval_title() {
            ProgramStatus::Blocked {
                message: title.to_string(),
            }
        } else if self.bottom_pane.is_task_running() {
            ProgramStatus::Working
        } else {
            self.turn_end_status.clone()
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
