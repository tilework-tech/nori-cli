use tokio::sync::mpsc::UnboundedSender;

use crate::app_event::AppEvent;
use crate::app_event::SessionGeneration;
use crate::session_log;

#[derive(Clone, Debug)]
pub(crate) struct AppEventSender {
    pub app_event_tx: UnboundedSender<AppEvent>,
    /// The session whose widget owns this sender. History it sends is tagged
    /// with the session so the App can file it in that session's transcript,
    /// even while another session is on screen.
    session: Option<SessionGeneration>,
}

impl AppEventSender {
    pub(crate) fn new(app_event_tx: UnboundedSender<AppEvent>) -> Self {
        Self {
            app_event_tx,
            session: None,
        }
    }

    /// A sender whose history events belong to `generation`.
    pub(crate) fn for_session(&self, generation: SessionGeneration) -> Self {
        Self {
            app_event_tx: self.app_event_tx.clone(),
            session: Some(generation),
        }
    }

    /// Send an event to the app event channel. If it fails, we swallow the
    /// error and log it.
    pub(crate) fn send(&self, event: AppEvent) {
        // Record inbound events for high-fidelity session replay.
        // Avoid double-logging Ops; those are logged at the point of submission.
        if !matches!(event, AppEvent::HarnessAction(_)) {
            session_log::log_inbound_app_event(&event);
        }
        let event = match (self.session, event) {
            (
                Some(generation),
                event @ (AppEvent::InsertHistoryCell(_) | AppEvent::ConsolidateAgentMessage { .. }),
            ) => AppEvent::SessionHistory {
                generation,
                event: Box::new(event),
            },
            (_, event) => event,
        };
        if let Err(e) = self.app_event_tx.send(event) {
            tracing::error!("failed to send event: {e}");
        }
    }
}
