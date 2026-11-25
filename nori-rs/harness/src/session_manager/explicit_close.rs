//! Explicit close belongs to one process generation. Only its accepted
//! terminal boundary retires descendants and releases their pending work.

use super::*;
use anyhow::Context;

#[derive(Default)]
pub(super) struct Closing {
    running: bool,
    awaiting: Option<AgentProcessId>,
    children: Vec<AgentProcessId>,
}

impl State {
    fn retire_descendants(&mut self, session: ConversationId, root: Option<AgentProcessId>) {
        let children = self
            .cancel_tree(&session)
            .into_iter()
            .filter_map(|id| self.registry.live_process(&id))
            .filter(|process| Some(*process) != root)
            .collect::<Vec<_>>();
        let closing = self.explicit_closes.entry(session).or_default();
        closing.awaiting = None;
        for child in children {
            if !closing.children.contains(&child) {
                closing.children.push(child);
            }
        }
    }

    pub(super) fn observe_explicit_close(&mut self, process: AgentProcessId, event: &SessionEvent) {
        if matches!(event, SessionEvent::Nori(NoriEvent::SessionEnded(end)) if end.reason == nori_protocol::SessionEndReason::Closed)
            && let Some(session) = self.registry.conversation_of(process)
            && self
                .explicit_closes
                .get(&session)
                .is_some_and(|closing| closing.awaiting == Some(process))
        {
            self.retire_descendants(session, Some(process));
        }
    }
}

impl SessionManager {
    /// Ask the named live adapter to release the session. Refusal keeps its
    /// permissions and children live. A cleanup retry addresses the captured
    /// processes even if a conversation has since been resumed.
    pub async fn close_explicitly(&self, session: ConversationId) -> anyhow::Result<()> {
        let target = {
            let mut state = self.shared.state();
            anyhow::ensure!(
                !state
                    .explicit_closes
                    .get(&session)
                    .is_some_and(|c| c.running),
                "Session close already in progress"
            );
            let record = state
                .registry
                .record(&session)
                .ok_or_else(|| anyhow::anyhow!("Unknown session"))?;
            let target = if state.explicit_closes.contains_key(&session) {
                // A previous close was acknowledged but descendant cleanup did
                // not finish. Retrying it must spare any reopened root.
                None
            } else {
                state
                    .registry
                    .live_process(&session)
                    .and_then(|p| state.handles.get(&p).cloned().map(|handle| (p, handle)))
            };
            if target.is_none() && !state.explicit_closes.contains_key(&session) {
                if record.status.is_closed() {
                    return Ok(());
                }
                state.retire_descendants(session, None);
            }
            let closing = state.explicit_closes.entry(session).or_default();
            closing.running = true;
            closing.awaiting = target.as_ref().map(|(p, _)| *p);
            target
        };
        let manager = self.clone();
        // A terminal can disconnect after sending the request.
        tokio::spawn(async move {
            let result = manager.finish_explicit_close(session, target).await;
            let mut state = manager.shared.state();
            let closing = state
                .explicit_closes
                .get_mut(&session)
                .context("Missing pending session close")?;
            closing.running = false;
            closing.awaiting = None;
            if closing.children.is_empty() {
                state.explicit_closes.remove(&session);
            }
            result
        })
        .await?
    }

    async fn finish_explicit_close(
        &self,
        session: ConversationId,
        target: Option<(AgentProcessId, HarnessHandle)>,
    ) -> anyhow::Result<()> {
        if let Some((process, handle)) = target {
            handle.close_from_host().await?;
            super::delegation_driver::wait_process_end(&self.shared, process).await;
            anyhow::ensure!(
                self.shared
                    .state()
                    .explicit_closes
                    .get(&session)
                    .context("Missing pending session close")?
                    .awaiting
                    .is_none(),
                "Session ended before close was acknowledged"
            );
        }
        let children = self
            .shared
            .state()
            .explicit_closes
            .get(&session)
            .context("Missing pending session close")?
            .children
            .clone();
        let mut first_error = None;
        for process in children {
            let handle = self.shared.state().handles.get(&process).cloned();
            if let Some(handle) = handle {
                let result = tokio::time::timeout(self.shared.config.shutdown_timeout, async {
                    handle.shutdown_with_grace(Duration::ZERO).await?;
                    super::delegation_driver::wait_process_end(&self.shared, process).await;
                    anyhow::Ok(())
                })
                .await
                .map_err(anyhow::Error::from)
                .and_then(|r| r);
                if let Err(error) = result
                    && self.shared.state().handles.contains_key(&process)
                {
                    first_error.get_or_insert(error);
                    continue;
                }
            }
            self.shared
                .state()
                .explicit_closes
                .get_mut(&session)
                .context("Missing pending session close")?
                .children
                .retain(|p| *p != process);
        }
        first_error.map_or(Ok(()), Err)
    }
}
