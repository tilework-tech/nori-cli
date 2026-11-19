//! Optional harness services for an admitted binding's standard ACP sessions.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use nori_protocol::AcpEvent;
use nori_protocol::SessionEvent;
use nori_protocol::acp::v1 as acp;
use serde_json::Value;

use super::nori_client_mcp::NoriClientServer;
use super::nori_client_mcp::NoriClientShared;
use crate::transcript::SessionEventEntry;
use crate::transcript::TranscriptRecorder;

pub struct ManagedMcp {
    _server: NoriClientServer,
}

#[derive(Default)]
struct SessionServices {
    recorder: Option<TranscriptRecorder>,
    mcp: Option<ManagedMcp>,
}

/// Recorder handles outlive the relay until a durable close or binding release.
#[derive(Default, Clone)]
pub struct ManagedRecorderBarrier {
    writers: Arc<tokio::sync::Mutex<HashMap<String, Option<TranscriptRecorder>>>>,
}

impl ManagedRecorderBarrier {
    pub async fn checkpoint(&self) -> Result<()> {
        let writers = self.writers.lock().await;
        for (sid, writer) in writers.iter() {
            writer
                .as_ref()
                .with_context(|| format!("private recorder unavailable for {sid}"))?
                .durable_flush()
                .await?;
        }
        Ok(())
    }
}

pub struct ManagedSessionAdditions {
    nori_home: PathBuf,
    cwd: PathBuf,
    agent: String,
    record: bool,
    inject_mcp: bool,
    barrier: Option<ManagedRecorderBarrier>,
    sessions: HashMap<String, SessionServices>,
}

impl ManagedSessionAdditions {
    pub fn new(
        nori_home: PathBuf,
        cwd: PathBuf,
        agent: String,
        record: bool,
        inject_mcp: bool,
    ) -> Self {
        Self {
            nori_home,
            cwd,
            agent,
            record,
            inject_mcp,
            sessions: HashMap::new(),
            barrier: None,
        }
    }

    pub fn with_barrier(mut self, barrier: ManagedRecorderBarrier) -> Self {
        self.barrier = Some(barrier);
        self
    }

    pub async fn prepare_mcp(
        &self,
        initialize_result: &Value,
        params: &mut Value,
    ) -> Result<Option<ManagedMcp>> {
        if !self.inject_mcp
            || initialize_result.pointer("/agentCapabilities/mcpCapabilities/http")
                != Some(&Value::Bool(true))
        {
            return Ok(None);
        }
        let Some(params) = params.as_object_mut() else {
            return Ok(None);
        };
        let Some(servers) = params
            .entry("mcpServers")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
        else {
            return Ok(None);
        };
        if servers.iter().any(|server| server["name"] == "nori-client") {
            return Ok(None);
        }
        let server = NoriClientServer::spawn(NoriClientShared::default()).await?;
        servers.push(serde_json::to_value(server.as_mcp_server())?);
        Ok(Some(ManagedMcp { _server: server }))
    }

    pub async fn opened(&mut self, sid: &str, mcp: Option<ManagedMcp>) -> Result<()> {
        if !self.record && mcp.is_none() {
            return Ok(());
        }
        let session = self.sessions.entry(sid.to_owned()).or_default();
        if mcp.is_some() {
            session.mcp = mcp;
        }
        if self.record && session.recorder.is_none() {
            let mut registered = match &self.barrier {
                Some(barrier) => {
                    let mut writers = barrier.writers.lock().await;
                    if let Some(Some(recorder)) = writers.get(sid) {
                        session.recorder = Some(recorder.clone());
                        return Ok(());
                    }
                    writers.insert(sid.to_owned(), None);
                    Some(writers)
                }
                None => None,
            };
            let recorder = TranscriptRecorder::create(
                &self.nori_home,
                &self.cwd,
                Some(self.agent.clone()),
                env!("CARGO_PKG_VERSION"),
                Some(sid.to_owned()),
                None,
                self.barrier.is_some(),
            )
            .await?;
            if let Some(writers) = registered.as_mut() {
                writers.insert(sid.to_owned(), Some(recorder.clone()));
            }
            session.recorder = Some(recorder);
        }
        Ok(())
    }

    pub async fn prompt(&self, sid: &str, params: &Value) -> Result<()> {
        let Some(recorder) = self
            .sessions
            .get(sid)
            .and_then(|session| session.recorder.as_ref())
        else {
            return Ok(());
        };
        let Some(prompt) = params["prompt"].as_array() else {
            return Ok(());
        };
        let id = params["_meta"][nori_protocol::PROMPT_ECHO_ID_META_KEY]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let text = prompt
            .iter()
            .filter(|block| block["type"] == "text")
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        recorder.record_user_message(&id, &text, Vec::new()).await?;
        let message_id = acp::MessageId::new(id);
        for block in prompt {
            if let Ok(content) = serde_json::from_value::<acp::ContentBlock>(block.clone()) {
                let update = acp::SessionUpdate::UserMessageChunk(
                    acp::ContentChunk::new(content)
                        .message_id(message_id.clone())
                        .meta(params["_meta"][crate::follow_up::SENDER_META].as_str().map(
                            |sender| {
                                acp::Meta::from_iter([(
                                    crate::follow_up::SENDER_META.to_string(),
                                    serde_json::json!(sender),
                                )])
                            },
                        )),
                );
                Self::record_update(
                    recorder,
                    acp::SessionNotification::new(sid.to_owned(), update),
                )
                .await?;
            }
        }
        Ok(())
    }

    pub async fn update(&self, sid: &str, params: &Value) -> Result<()> {
        let Some(recorder) = self
            .sessions
            .get(sid)
            .and_then(|session| session.recorder.as_ref())
        else {
            return Ok(());
        };
        if let Ok(notification) = serde_json::from_value::<acp::SessionNotification>(params.clone())
        {
            Self::record_update(recorder, notification).await?;
        }
        Ok(())
    }

    async fn record_update(
        recorder: &TranscriptRecorder,
        notification: acp::SessionNotification,
    ) -> Result<()> {
        let event = SessionEvent::Acp(AcpEvent::Notification(
            acp::AgentNotification::SessionNotification(notification),
        ));
        recorder
            .record_session_event(&SessionEventEntry::new(event))
            .await?;
        Ok(())
    }

    pub async fn closed(&mut self, sid: &str) -> Result<()> {
        if let Some(session) = self.sessions.remove(sid)
            && let Some(recorder) = session.recorder
        {
            if let Some(barrier) = &self.barrier {
                let mut writers = barrier.writers.lock().await;
                recorder.durable_flush().await?;
                recorder.shutdown().await?;
                writers.remove(sid);
            } else {
                recorder.shutdown().await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[tokio::test]
    async fn a_binding_barrier_drains_the_open_recorder_even_after_relay_disconnect() -> Result<()>
    {
        let temp = TempDir::new()?;
        let root = temp.path().to_owned();
        let barrier = ManagedRecorderBarrier::default();
        let mut services =
            ManagedSessionAdditions::new(root.clone(), root, "fixture".into(), true, false)
                .with_barrier(barrier.clone());
        services.opened("fixture-main", None).await?;
        let path = services.sessions["fixture-main"]
            .recorder
            .as_ref()
            .unwrap()
            .transcript_path()
            .to_owned();
        services
            .prompt(
                "fixture-main",
                &json!({"prompt":[{"type":"text","text":"recorded before detach"}]}),
            )
            .await?;
        drop(services);
        barrier.checkpoint().await?;
        let jsonl = tokio::fs::read_to_string(path).await?;
        assert!(
            jsonl
                .lines()
                .map(serde_json::from_str::<Value>)
                .collect::<serde_json::Result<Vec<_>>>()?
                .iter()
                .any(|entry| entry["content"] == "recorded before detach")
        );
        Ok(())
    }

    #[tokio::test]
    async fn a_missing_private_writer_cannot_be_reported_as_flushed_after_disconnect() -> Result<()>
    {
        let temp = TempDir::new()?;
        let root = temp.path().join("not-a-directory");
        tokio::fs::write(&root, "fixture").await?;
        let barrier = ManagedRecorderBarrier::default();
        let mut services =
            ManagedSessionAdditions::new(root, temp.path().into(), "fixture".into(), true, false)
                .with_barrier(barrier.clone());
        assert!(services.opened("fixture-main", None).await.is_err());
        drop(services);
        assert!(barrier.checkpoint().await.is_err());
        Ok(())
    }
}
