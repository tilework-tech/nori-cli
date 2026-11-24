//! Closing one session while others keep running.

use super::ChatWidget;
use crate::app_event::AppEvent;

/// One refused `/close` is reported by the adapter's error response and by
/// the close command's error, in either order. Whichever arrives second is
/// dropped when it describes the same refusal.
#[derive(Debug)]
pub(super) enum CloseRefusal {
    /// The close is in flight. Adapter errors seen now wait for the command's
    /// verdict, which prints any it does not already describe.
    Pending { adapter: Vec<AdapterError> },
    /// The command's error printed first; the adapter's matching error is
    /// still to come.
    Reported { command: String },
}

#[derive(Debug)]
pub(super) struct AdapterError {
    raw: String,
    display: String,
}

impl ChatWidget {
    /// The host decides whether to ask the adapter or close its private process.
    pub(super) fn start_host_close(&mut self) {
        let Some(session) = self.managed_conversation_id else {
            self.add_error_message("No live host session to close.".into());
            return;
        };
        self.session_close_in_flight = true;
        self.close_refusal = Some(CloseRefusal::Pending {
            adapter: Vec::new(),
        });
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
        self.close_refusal = Some(CloseRefusal::Pending {
            adapter: Vec::new(),
        });
        let tx = self.app_event_tx.clone();
        let generation = self.session_generation;
        tokio::spawn(async move {
            // Success ends the session; the app confirms the release on
            // the screen that replaces it.
            if let Err(e) = handle.close_session().await {
                tx.send(AppEvent::SessionCloseFailed {
                    generation,
                    message: format!("{e:#}"),
                });
            }
        });
    }

    /// A `/close` failed: surface the (already enhanced) error and unblock the
    /// session-switching commands that were held while the close was in flight.
    pub(crate) fn on_session_close_failed(&mut self, message: String) {
        self.session_close_in_flight = false;
        self.close_refusal = match self.close_refusal.take() {
            Some(CloseRefusal::Pending { adapter }) => {
                let covers =
                    |error: &AdapterError| !error.raw.is_empty() && message.contains(&error.raw);
                let described = adapter.iter().any(covers);
                for error in adapter {
                    if !covers(&error) {
                        self.on_error(error.display);
                    }
                }
                (!described).then(|| CloseRefusal::Reported {
                    command: message.clone(),
                })
            }
            Some(CloseRefusal::Reported { .. }) | None => None,
        };
        self.add_error_message(format!("Failed to close the session: {message}"));
    }

    /// Whether an adapter error that no prompt owns is held or dropped
    /// because the latest `/close` command reports it. A command error
    /// covers only the next such error.
    pub(super) fn absorb_close_refusal(&mut self, raw: &str, display: &str) -> bool {
        match self.close_refusal.take() {
            Some(CloseRefusal::Pending { mut adapter }) => {
                adapter.push(AdapterError {
                    raw: raw.to_string(),
                    display: display.to_string(),
                });
                self.close_refusal = Some(CloseRefusal::Pending { adapter });
                true
            }
            Some(CloseRefusal::Reported { command }) => !raw.is_empty() && command.contains(raw),
            None => false,
        }
    }

    /// The adapter released the session, so errors held for a refusal
    /// belong to something else.
    pub(super) fn release_held_close_errors(&mut self) {
        if let Some(CloseRefusal::Pending { adapter }) = self.close_refusal.take() {
            for error in adapter {
                self.on_error(error.display);
            }
        }
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
