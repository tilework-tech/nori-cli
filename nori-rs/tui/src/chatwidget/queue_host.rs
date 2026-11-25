//! The queue of a session on the host is resumed and reclaimed with
//! `_nori/session/queue/*`, so whichever terminal holds control acts on the
//! same queue. The host returns a taken prompt's text to exactly one caller;
//! it is restored to the composer only once the host has answered.

use super::*;
use crate::app_event::QueuedPromptAction;
use crate::app_event::ReclaimOutcome;
use crate::host_client::HostCall;

impl ChatWidget {
    /// The host session whose queue this widget controls, if any.
    pub(super) fn host_queue_session(&self) -> Option<String> {
        self.managed_conversation_id
            .filter(|_| !self.cloud_mode)
            .map(|id| id.to_string())
    }

    pub(super) fn resume_queue_on_host(&self, session_id: String) {
        self.app_event_tx.send(AppEvent::HostRequest {
            call: HostCall::ResumeQueue { session_id },
            reply: crate::app_event_sender::HostReply::ReportError(self.app_event_tx.clone()),
        });
    }

    pub(super) fn take_queued_on_host(
        &self,
        session_id: String,
        event_id: String,
        action: QueuedPromptAction,
    ) {
        let tx = self.app_event_tx.clone();
        let generation = self.session_generation;
        let reply = tx.host_receipt(HostCall::TakeQueued {
            session_id,
            entry_id: event_id.clone(),
        });
        tokio::spawn(async move {
            let outcome = match reply.await {
                Ok(reply) => match reply.get("text") {
                    Some(serde_json::Value::String(text)) => {
                        ReclaimOutcome::Reclaimed(text.clone())
                    }
                    Some(serde_json::Value::Null) => ReclaimOutcome::AlreadyStarted,
                    _ => ReclaimOutcome::Failed,
                },
                Err(error) => {
                    tx.send(AppEvent::HarnessActionFailed(format!(
                        "Could not take the queued message back: {error}"
                    )));
                    ReclaimOutcome::Failed
                }
            };
            tx.send(AppEvent::QueuedPromptReclaimed {
                generation,
                event_id,
                action,
                outcome,
            });
        });
    }
}
