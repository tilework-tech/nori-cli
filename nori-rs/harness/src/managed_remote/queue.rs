//! `_nori/session/queue/*`: the controller resumes a paused queue, takes one
//! queued prompt back, or discards them all, from any terminal on the host.
//! Clients see the queue itself in `queue_changed` events and on
//! `_nori/session/list` rows.

use nori_protocol::QueueDiscardResult;
use nori_protocol::QueueParams;
use nori_protocol::QueueTakeParams;
use nori_protocol::QueueTakeResult;
use serde_json::Value;
use serde_json::json;

use super::ManagedRemoteHost;
use super::RemoteError;
use crate::runtime::HarnessHandle;

pub(super) const RESUME: &str = "_nori/session/queue/resume";
pub(super) const TAKE: &str = "_nori/session/queue/take";
pub(super) const DISCARD: &str = "_nori/session/queue/discard";

impl ManagedRemoteHost {
    pub(super) async fn queue(&self, method: &str, params: Value) -> Result<Value, RemoteError> {
        let invalid = |error: serde_json::Error| RemoteError::new(-32602, error);
        match method {
            RESUME => {
                let QueueParams { session_id } = serde_json::from_value(params).map_err(invalid)?;
                let handle = self.running(&session_id)?;
                handle.resume_queue().await.map_err(failed)?;
                Ok(json!({}))
            }
            TAKE => {
                let QueueTakeParams {
                    session_id,
                    entry_id,
                } = serde_json::from_value(params).map_err(invalid)?;
                let handle = self.running(&session_id)?;
                let text = handle
                    .reclaim_queued_prompt(entry_id)
                    .await
                    .map_err(failed)?;
                Ok(json!(QueueTakeResult { text }))
            }
            DISCARD => {
                let QueueParams { session_id } = serde_json::from_value(params).map_err(invalid)?;
                let handle = self.running(&session_id)?;
                let discarded = handle.discard_queue().await.map_err(failed)?;
                Ok(json!(QueueDiscardResult { discarded }))
            }
            _ => Err(RemoteError::new(-32601, "method not found")),
        }
    }

    fn running(&self, session_id: &str) -> Result<HarnessHandle, RemoteError> {
        let id = self.target(Some(session_id))?;
        let handle = self
            .manager
            .handle(&id)
            .ok_or_else(|| RemoteError::new(-32002, "session is not running"))?;
        Ok(handle)
    }
}

fn failed(error: anyhow::Error) -> RemoteError {
    RemoteError::new(-32000, error)
}
