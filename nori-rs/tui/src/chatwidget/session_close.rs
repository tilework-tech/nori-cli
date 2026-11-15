//! Closing one session while others keep running.

use super::ChatWidget;
use crate::app_event::AppEvent;
use crate::history_cell;

impl ChatWidget {
    /// Release this cloud session through the agent's `session/close`.
    pub(crate) fn start_cloud_close(&mut self) {
        let Some(handle) = self.harness_handle.clone() else {
            self.add_error_message("No live agent connection to close.".to_string());
            return;
        };
        self.session_close_in_flight = true;
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            match handle.close_session().await {
                Ok(()) => {
                    tx.send(AppEvent::InsertHistoryCell(Box::new(
                        history_cell::new_info_event(
                            "Session closed (released).".to_string(),
                            None,
                        ),
                    )));
                }
                Err(e) => {
                    tx.send(AppEvent::SessionCloseFailed {
                        message: format!("{e}"),
                    });
                }
            }
        });
    }

    /// Mark this session as closed by the user, so its agent shutting down
    /// ends only this session.
    pub(crate) fn mark_closed_by_user(&mut self) {
        self.closed_by_user = true;
    }
}
