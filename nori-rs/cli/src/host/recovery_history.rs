use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

#[cfg(test)]
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use nori_config::AskForApproval;
use nori_harness::ConversationId;
use nori_harness::session_manager::SessionCheckpoint;
use nori_harness::session_manager::SessionManager;

pub(super) struct History {
    manager: SessionManager,
    imports: Mutex<HashMap<ConversationId, String>>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            manager: SessionManager::new(Default::default()),
            imports: Mutex::default(),
        }
    }
}

impl History {
    pub(super) async fn export(&self, stable: &str) -> Result<Option<SessionCheckpoint>> {
        let id = ConversationId::from_string(stable)?;
        if self.manager.get(&id).is_none() {
            return Ok(None);
        }
        self.manager.checkpoint_session(&id, false).await.map(Some)
    }

    pub(super) fn import(
        &self,
        stable: &str,
        historical: Option<&SessionCheckpoint>,
        workspace: &Path,
    ) -> Result<()> {
        let Some(checkpoint) = historical else {
            return Ok(());
        };
        let id = ConversationId::from_string(stable)?;
        ensure!(
            checkpoint.conversation_id == id,
            "historical metadata belongs to a different conversation"
        );
        let identity = serde_json::to_string(&(
            &checkpoint.agent,
            &checkpoint.acp_session_id,
            checkpoint.parent,
            checkpoint.source,
            checkpoint.status,
            &checkpoint.delegations,
        ))?;
        let mut imports = self
            .imports
            .lock()
            .map_err(|_| anyhow::anyhow!("historical import interrupted"))?;
        if let Some(previous) = imports.get(&id) {
            ensure!(
                *previous == identity,
                "conflicting historical metadata for {id}"
            );
            return Ok(());
        }
        self.manager
            .import_checkpoint(checkpoint, workspace, AskForApproval::Never)?;
        imports.insert(id, identity);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nori_harness::session_manager::SessionSource;
    use nori_protocol::SessionStatus;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn imported_history_is_idempotent_and_scoped_to_its_registry() -> anyhow::Result<()> {
        let stable = ConversationId::default();
        let id = stable.to_string();
        let checkpoint = SessionCheckpoint {
            conversation_id: stable,
            acp_session_id: Some("fixture-acp".into()),
            agent: "fixture".into(),
            parent: Some(ConversationId::default()),
            source: SessionSource::Session,
            status: SessionStatus::Done,
            delegations: vec![],
            private_transcript: Some("synthetic private recorder image".into()),
        };
        let workspace = tempfile::tempdir()?;
        let desktop = SessionManager::new(Default::default());
        desktop.import_checkpoint(&checkpoint, workspace.path(), AskForApproval::Never)?;
        let history = History::default();
        history.import(&id, None, workspace.path())?;
        assert!(history.export(&id).await?.is_none());
        history.import(&id, Some(&checkpoint), workspace.path())?;
        history.import(&id, Some(&checkpoint), workspace.path())?;
        let persisted = history
            .export(&id)
            .await?
            .context("imported historical record")?;
        assert_eq!(persisted.status, SessionStatus::Done);
        assert_eq!(persisted.parent, checkpoint.parent);
        assert!(persisted.private_transcript.is_none());
        let mut changed = checkpoint.clone();
        changed.status = SessionStatus::Failed;
        assert!(
            history
                .import(&id, Some(&changed), workspace.path())
                .is_err()
        );
        assert!(
            history
                .import(
                    &ConversationId::default().to_string(),
                    Some(&checkpoint),
                    workspace.path()
                )
                .is_err()
        );
        Ok(())
    }
}
