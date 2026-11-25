//! The queue of a session on the host is resumed and reclaimed with
//! `_nori/session/queue/*`, so whichever terminal holds control acts on the
//! same queue. The host returns a taken prompt's text to exactly one caller;
//! it is restored to the composer only once the host has answered. Each
//! take carries a key, and a take whose reply was lost is asked again with
//! the same key until the host answers, so the text is never dropped.

use super::*;
use crate::app_event::QueuedPromptAction;
use crate::app_event::ReclaimOutcome;
use crate::host_client::HostCall;

/// A take whose reply was lost is asked again for about a minute, which
/// covers a terminal reconnecting to its host.
const TAKE_RETRIES: i32 = 120;
const TAKE_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

impl ChatWidget {
    /// The host session whose queue this widget controls, if any. A joined
    /// host without `_nori/session/queue/*` has none.
    pub(super) fn host_queue_session(&self) -> Option<String> {
        let served = self
            .host_connection
            .as_ref()
            .is_none_or(|connection| connection.features.queue);
        self.managed_conversation_id
            .filter(|_| !self.cloud_mode && served)
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
        let request_key = uuid::Uuid::new_v4().to_string();
        let call = {
            let entry_id = event_id.clone();
            move || HostCall::TakeQueued {
                session_id: session_id.clone(),
                entry_id: entry_id.clone(),
                request_key: request_key.clone(),
            }
        };
        let first = tx.host_receipt(call());
        tokio::spawn(async move {
            let mut reply = first.await;
            for _ in 0..TAKE_RETRIES {
                let lost = reply.as_ref().is_err_and(|error| {
                    error.downcast_ref::<crate::app::ConnectionLost>().is_some()
                });
                if !lost {
                    break;
                }
                tokio::time::sleep(TAKE_RETRY_DELAY).await;
                reply = tx.host_receipt(call()).await;
            }
            let outcome = match reply {
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
