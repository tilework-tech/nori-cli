//! Track observed work independently of the local ACP request slot.

use nori_protocol::ExternalTurn;
use nori_protocol::ExternalTurnState;
use nori_protocol::acp::v1 as acp;

/// Synchronously admit session-only cancels to the same queue as observed ACP
/// activity. A pending config RPC on the command loop must not retarget a cancel.
#[derive(Clone, Default)]
pub(crate) struct CancelAdmission(
    std::sync::Arc<
        std::sync::OnceLock<
            tokio::sync::mpsc::WeakSender<super::session_runtime_driver::SessionRuntimeInput>,
        >,
    >,
);

impl CancelAdmission {
    pub(super) fn activate(
        &self,
        tx: &tokio::sync::mpsc::Sender<super::session_runtime_driver::SessionRuntimeInput>,
    ) {
        let _ = self.0.set(tx.downgrade());
    }

    pub(crate) fn submit(
        &self,
    ) -> anyhow::Result<tokio::sync::oneshot::Receiver<anyhow::Result<()>>> {
        let runtime = self
            .0
            .get()
            .and_then(tokio::sync::mpsc::WeakSender::upgrade)
            .ok_or_else(|| anyhow::anyhow!("No active turn to cancel"))?;
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        runtime
            .try_send(super::session_runtime_driver::SessionRuntimeInput::Cancel {
                target: CancelTarget::Current,
                response_tx,
            })
            .map_err(|error| anyhow::anyhow!("Could not enqueue cancellation: {error}"))?;
        Ok(response_rx)
    }
}

pub(crate) enum CancelTarget {
    /// Historical local action; an idle request is harmless.
    Owned,
    External(String),
    /// Standard ACP notifications identify the session only.
    Current,
}

#[derive(Default)]
pub(super) struct ExternalActivity {
    active: Option<Observation>,
}

struct Observation {
    session: acp::SessionId,
    /// Optional identity from a broker that publishes correlated turn boundaries.
    provider_id: Option<String>,
    turn: ExternalTurn,
}

impl ExternalActivity {
    pub(super) fn current(&self, session: &acp::SessionId) -> Option<&ExternalTurn> {
        self.active
            .as_ref()
            .filter(|turn| &turn.session == session)
            .map(|turn| &turn.turn)
    }

    pub(super) fn set_state(&mut self, id: &str, state: ExternalTurnState) -> Option<ExternalTurn> {
        let observation = self.active.as_mut().filter(|active| active.turn.id == id)?;
        observation.turn.state = state;
        Some(observation.turn.clone())
    }

    pub(super) fn finish(&mut self, state: ExternalTurnState) -> Option<ExternalTurn> {
        let mut turn = self.active.take()?.turn;
        turn.state = state;
        Some(turn)
    }

    pub(super) fn observe(
        &mut self,
        session: &acp::SessionId,
        update: &acp::SessionUpdate,
    ) -> Vec<ExternalTurn> {
        use acp::SessionUpdate::*;
        let (provider_id, terminal) = match update {
            SessionInfoUpdate(info) => {
                let Some(meta) = &info.meta else {
                    return vec![];
                };
                let id = meta
                    .get("nori/turnId")
                    .and_then(|v| v.as_str())
                    .filter(|id| !id.is_empty());
                let terminal = match meta.get("nori/turnState").and_then(|v| v.as_str()) {
                    Some("active") if id.is_some() => None,
                    Some("idle") if id.is_some() => {
                        if self
                            .active
                            .as_ref()
                            .and_then(|turn| turn.provider_id.as_deref())
                            != id
                        {
                            return vec![];
                        }
                        Some(
                            if meta.get("nori/stopReason").and_then(|v| v.as_str())
                                == Some("cancelled")
                            {
                                ExternalTurnState::Cancelled
                            } else {
                                ExternalTurnState::Idle
                            },
                        )
                    }
                    _ => match meta
                        .get("nori")
                        .and_then(|v| v.get("status"))
                        .and_then(|v| v.as_str())
                    {
                        Some("working") => None,
                        Some("idle") => Some(ExternalTurnState::Idle),
                        _ => return vec![],
                    },
                };
                (id, terminal)
            }
            UserMessageChunk(chunk) => (
                chunk
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.get("nori/turnId"))
                    .and_then(|v| v.as_str())
                    .filter(|id| !id.is_empty()),
                None,
            ),
            AgentMessageChunk(_) | AgentThoughtChunk(_) | Plan(_) | ToolCall(_)
            | ToolCallUpdate(_) => (None, None),
            _ => return vec![],
        };

        if let Some(state) = terminal {
            return if self.current(session).is_some() {
                self.finish(state).into_iter().collect()
            } else {
                vec![]
            };
        }

        if let Some(active) = &mut self.active
            && &active.session == session
            && (provider_id.is_none()
                || active.provider_id.is_none()
                || active.provider_id.as_deref() == provider_id)
        {
            if active.provider_id.is_none() {
                active.provider_id = provider_id.map(str::to_owned);
            }
            return vec![];
        }

        let mut events = Vec::new();
        events.extend(self.finish(ExternalTurnState::Superseded));
        let turn = ExternalTurn {
            id: uuid::Uuid::new_v4().to_string(),
            state: ExternalTurnState::Working,
        };
        self.active = Some(Observation {
            session: session.clone(),
            provider_id: provider_id.map(str::to_owned),
            turn: turn.clone(),
        });
        events.push(turn);
        events
    }
}

impl super::AcpBackend {
    pub(super) async fn publish_external_turn(&self, change: ExternalTurn) {
        let event = nori_protocol::SessionEvent::Nori(
            nori_protocol::NoriEvent::ExternalTurnChanged(change),
        );
        let _ = self
            .backend_event_tx
            .send(super::BackendEvent::Public(event))
            .await;
    }

    /// Runs on the same serial input queue as prompt admission and live ACP updates.
    pub(super) async fn cancel_turn(&self, target: CancelTarget) -> anyhow::Result<()> {
        let session = self.session_id.read().await;
        let mut driver = self.session_driver.lock().await;
        let external = match (&target, driver.public_phase()) {
            (
                CancelTarget::Owned | CancelTarget::Current,
                nori_protocol::SessionPhase::Prompting { .. }
                | nori_protocol::SessionPhase::Cancelling { .. },
            ) => None,
            (CancelTarget::Owned, _) => return Ok(()),
            (
                CancelTarget::External(_) | CancelTarget::Current,
                nori_protocol::SessionPhase::Idle,
            ) => {
                let Some(observed) = driver.external.current(&session) else {
                    anyhow::bail!("No active turn to cancel");
                };
                if let CancelTarget::External(expected) = &target
                    && expected != &observed.id
                {
                    anyhow::bail!("The observed turn is no longer active");
                }
                if observed.state != ExternalTurnState::Working {
                    return Ok(());
                }
                Some(observed.id.clone())
            }
            _ => anyhow::bail!("The requested turn is no longer active"),
        };

        let Some(id) = external else {
            drop(driver);
            drop(session);
            self.apply_session_event(super::session_reducer::InboundEvent::CancelSubmit)
                .await;
            return Ok(());
        };

        self.connection.cancel(&session).await?;
        let change = driver
            .external
            .set_state(&id, ExternalTurnState::CancelRequested);
        drop(driver);
        drop(session);
        if let Some(change) = change {
            self.publish_external_turn(change).await;
        }

        let runtime = self.session_event_tx.downgrade();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(
                super::session_runtime_driver::CANCEL_WARNING_SECS,
            ))
            .await;
            if let Some(runtime) = runtime.upgrade() {
                let _ = runtime
                    .send(
                        super::session_runtime_driver::SessionRuntimeInput::ExternalCancelTimeout(
                            id,
                        ),
                    )
                    .await;
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests;
