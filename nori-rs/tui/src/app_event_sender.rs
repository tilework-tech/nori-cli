use tokio::sync::mpsc::UnboundedSender;

use crate::app_event::AppEvent;
use crate::app_event::SessionGeneration;
use crate::session_log;

pub(crate) enum HostReply {
    ReportError(AppEventSender),
    Receipt(tokio::sync::oneshot::Sender<anyhow::Result<serde_json::Value>>),
    /// Turns the reply into the event it answers, sent where it lands, so
    /// the caller needs no task of its own to wait for it.
    Then(AppEventSender, HostReplyLanded),
}

pub(crate) type HostReplyLanded =
    Box<dyn FnOnce(anyhow::Result<serde_json::Value>) -> Option<AppEvent> + Send>;

impl std::fmt::Debug for HostReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReportError(_) => f.write_str("HostReply::ReportError"),
            Self::Receipt(_) => f.write_str("HostReply::Receipt"),
            Self::Then(..) => f.write_str("HostReply::Then"),
        }
    }
}

impl HostReply {
    pub(crate) fn deliver(self, result: anyhow::Result<serde_json::Value>) {
        match self {
            Self::ReportError(tx) => {
                if let Err(error) = result {
                    tx.send(AppEvent::HarnessActionFailed(error.to_string()));
                }
            }
            Self::Receipt(sender) => {
                let _ = sender.send(result);
            }
            Self::Then(tx, landed) => {
                if let Some(event) = landed(result) {
                    tx.send(event);
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AppEventSender {
    pub app_event_tx: UnboundedSender<AppEvent>,
    /// The session whose widget owns this sender. History and agent start-up
    /// news it sends are tagged with the session, so the App can file them
    /// with that session even while another session is on screen.
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

    /// Enqueue now to preserve the composer's submission order. The caller can
    /// wait for admission while this widget continues receiving terminal input.
    pub(crate) fn host_receipt(
        &self,
        call: crate::host_client::HostCall,
    ) -> impl Future<Output = anyhow::Result<serde_json::Value>> + Send + 'static {
        let (reply, receipt) = tokio::sync::oneshot::channel();
        self.send(AppEvent::HostRequest {
            call,
            reply: HostReply::Receipt(reply),
        });
        async move {
            receipt
                .await
                .map_err(|_| anyhow::anyhow!("Host stopped before replying."))?
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
                event @ (AppEvent::InsertHistoryCell(_)
                | AppEvent::ConsolidateAgentMessage { .. }
                | AppEvent::AgentConnecting { .. }
                | AppEvent::AgentSpawnFailed { .. }
                | AppEvent::HarnessActionFailed(_)
                | AppEvent::ScriptExecutionComplete { .. }),
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
