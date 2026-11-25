//! Closing one session while others keep running.

use super::ChatWidget;
use crate::app_event::AppEvent;

impl ChatWidget {
    /// The host decides whether to ask the adapter or close its private process.
    pub(super) fn start_host_close(&mut self) {
        let Some(session) = self.managed_conversation_id else {
            self.add_error_message("No live host session to close.".into());
            return;
        };
        self.session_close_in_flight = true;
        let tx = self.app_event_tx.clone();
        let generation = self.session_generation;
        let reply = tx.host_receipt(crate::host_client::HostCall::Close {
            session_id: session.to_string(),
        });
        tokio::spawn(async move {
            if let Err(error) = reply.await {
                tx.send(AppEvent::SessionCloseFailed {
                    generation,
                    message: error.to_string(),
                });
            }
        });
    }

    /// Release this cloud session through the agent's `session/close`.
    pub(crate) fn start_cloud_close(&mut self) {
        let Some(handle) = self.harness_handle.clone() else {
            self.add_error_message("No live agent connection to close.".to_string());
            return;
        };
        self.session_close_in_flight = true;
        let tx = self.app_event_tx.clone();
        let generation = self.session_generation;
        tokio::spawn(async move {
            // Success ends the session; the app confirms the release on
            // the screen that replaces it.
            if let Err(e) = handle.close_session().await {
                tx.send(AppEvent::SessionCloseFailed {
                    generation,
                    message: format!("{e}"),
                });
            }
        });
    }

    /// Mark this session as closed by the user, so its agent shutting down
    /// ends only this session.
    pub(crate) fn mark_closed_by_user(&mut self) {
        self.closed_by_user = true;
    }

    pub(crate) fn is_closed_by_user(&self) -> bool {
        self.closed_by_user
    }

    /// Whether the agent can release this session with `session/close`.
    pub(crate) fn supports_session_close(&self) -> bool {
        self.session_agent_capabilities.session_close
    }
}
