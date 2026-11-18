use std::path::Path;

use anyhow::Context;
use anyhow::ensure;
use nori_protocol::Delegation;
use nori_protocol::SessionStatus;
use serde::Deserialize;
use serde::Serialize;

use super::SessionManager;
use super::SessionRecord;
use super::SessionSettings;
use super::SessionSource;
use crate::ConversationId;
use nori_config::AskForApproval;

/// One conversation's recovery metadata. A caller may request its private
/// recorder image when this session requires local continuation evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionCheckpoint {
    pub conversation_id: ConversationId,
    pub agent: String,
    pub acp_session_id: Option<String>,
    pub parent: Option<ConversationId>,
    pub source: SessionSource,
    pub status: SessionStatus,
    pub delegations: Vec<Delegation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_transcript: Option<String>,
}

impl SessionManager {
    /// Caller holds the binding's admission barrier throughout this operation.
    pub async fn checkpoint_session(
        &self,
        id: &ConversationId,
        include_private_transcript: bool,
    ) -> anyhow::Result<SessionCheckpoint> {
        let (record, process, cursor, handle) = {
            let state = self.shared.state();
            let record = state
                .registry
                .record(id)
                .context("session is not registered")?;
            ensure!(
                matches!(
                    record.status,
                    SessionStatus::Ready
                        | SessionStatus::Failed
                        | SessionStatus::Done
                        | SessionStatus::Closed
                ),
                "session {id} has not reached a checkpoint boundary"
            );
            let process = state.registry.live_process(id);
            let cursor = state.journal.info(&id.to_string()).map(|r| r.latest_seq);
            let handle = process.and_then(|p| state.handles.get(&p)).cloned();
            (record, process, cursor, handle)
        };
        let private_transcript = if include_private_transcript {
            Some(
                handle
                    .context("private recovery recorder is no longer live")?
                    .snapshot_transcript()
                    .await?,
            )
        } else {
            None
        };
        let state = self.shared.state();
        ensure!(
            state.registry.live_process(id) == process
                && state.journal.info(&id.to_string()).map(|r| r.latest_seq) == cursor,
            "session {id} changed during checkpoint; retry at a turn boundary"
        );
        let SessionRecord {
            conversation_id,
            parent,
            source,
            agent,
            status,
            acp_session_id,
            ..
        } = record;
        let owner = id.to_string();
        let delegations = state
            .delegation
            .entries
            .iter()
            .map(|entry| &entry.value)
            .chain(&state.delegation.history)
            .filter(|value| value.child_id == owner || value.parent_id == owner)
            .cloned()
            .collect();
        Ok(SessionCheckpoint {
            conversation_id,
            parent,
            source,
            agent,
            status,
            acp_session_id,
            delegations,
            private_transcript,
        })
    }

    /// Recover historical identities only. The authenticated launch owns the
    /// destination workspace, policy and any subsequent provider connection.
    pub fn import_checkpoint(
        &self,
        checkpoint: &SessionCheckpoint,
        cwd: &Path,
        approval_policy: AskForApproval,
    ) -> anyhow::Result<()> {
        let mut state = self.shared.state();
        let owner = checkpoint.conversation_id.to_string();
        let mut history = Vec::new();
        for value in &checkpoint.delegations {
            ensure!(
                value.child_id == owner || value.parent_id == owner,
                "delegation does not belong to checkpoint session"
            );
            let parent = ConversationId::from_string(&value.parent_id)?;
            let child = ConversationId::from_string(&value.child_id)?;
            ensure!(parent != child, "delegation child and parent must differ");
            let existing = state
                .delegation
                .entries
                .iter()
                .map(|entry| &entry.value)
                .chain(&state.delegation.history)
                .chain(&history)
                .find(|entry| entry.request_id == value.request_id);
            if let Some(existing) = existing {
                ensure!(
                    existing == value,
                    "conflicting checkpoint for delegation {}",
                    value.request_id
                );
            } else {
                history.push(value.clone());
            }
        }
        state.registry.import_history(
            SessionSettings {
                conversation_id: checkpoint.conversation_id,
                agent: checkpoint.agent.clone(),
                parent: checkpoint.parent,
                source: checkpoint.source,
                resumed_from: None,
                cwd: cwd.to_owned(),
                model: None,
                approval_policy,
            },
            checkpoint.status,
            checkpoint.acp_session_id.clone(),
        )?;
        state.delegation.history.extend(history);
        state.journal.register(owner);
        Ok(())
    }
}
