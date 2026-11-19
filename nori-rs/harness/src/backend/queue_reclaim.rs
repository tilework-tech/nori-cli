//! Taking a queued prompt back before it is sent.
//!
//! A reclaim returns the one-shot hook context and compact summary the
//! prompt consumed when it was queued, so the next prompt still carries them.
//!
//! The request travels through the serialized session runtime so it is
//! ordered against the reducer events that drain the queue: either the
//! prompt is still queued and is removed, or it has already started.

use super::AcpBackend;
use super::PromptWithdrawn;
use super::session_runtime_driver::ReducerActions;
use super::session_runtime_driver::SessionRuntimeInput;

impl AcpBackend {
    pub(crate) async fn reclaim_queued_prompt(
        &self,
        event_id: String,
    ) -> anyhow::Result<Option<String>> {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        self.session_event_tx
            .send(SessionRuntimeInput::ReclaimQueuedPrompt {
                event_id,
                response_tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("ACP session runtime closed before queue reclaim"))?;
        response_rx
            .await
            .map_err(|_| anyhow::anyhow!("ACP session runtime closed during queue reclaim"))
    }

    /// Runs on the serialized session runtime task.
    pub(super) async fn apply_queue_reclaim(&self, event_id: &str) -> Option<String> {
        let reclaimed = self.session_driver.lock().await.reclaim_queued(event_id)?;
        if let Some(restore) = reclaimed.prompt.reclaim.as_deref().cloned() {
            if let Some(context) = restore.hook_context {
                let mut pending = self.pending_hook_context.lock().await;
                *pending = Some(match pending.take() {
                    Some(newer) => format!("{context}\n{newer}"),
                    None => context,
                });
            }
            if let Some(summary) = restore.compact_summary {
                self.pending_compact_summary
                    .lock()
                    .await
                    .get_or_insert(summary);
            }
        }
        if let Some(response_tx) = self
            .pending_prompt_submissions
            .lock()
            .await
            .remove(&reclaimed.prompt.event_id)
        {
            let _ = response_tx.send(Err(PromptWithdrawn.into()));
        }
        self.dispatch_reducer_actions(ReducerActions {
            events: vec![reclaimed.queue_changed],
            side_effects: Vec::new(),
            completed_turn: None,
        })
        .await;
        Some(reclaimed.text)
    }
}
