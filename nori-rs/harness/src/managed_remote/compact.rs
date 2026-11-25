//! `_nori/session/compact`: the controller asks the host to compact one
//! session. The reply means the compaction was admitted, behind any turn the
//! session is running. Every terminal following the session sees it finish
//! as that session's `context_compacted`, or as a "Compact failed" error,
//! whether the agent compacts natively or the host summarizes and swaps.

use nori_protocol::CompactParams;
use serde_json::Value;
use serde_json::json;

use super::ManagedRemoteHost;
use super::RemoteError;

pub(super) const COMPACT: &str = "_nori/session/compact";

impl ManagedRemoteHost {
    pub(super) async fn compact(&self, params: Value) -> Result<Value, RemoteError> {
        let CompactParams { session_id } =
            serde_json::from_value(params).map_err(|error| RemoteError::new(-32602, error))?;
        self.running(&session_id)?
            .compact()
            .await
            .map_err(|error| RemoteError::new(-32000, format!("{error:#}")))?;
        Ok(json!({}))
    }
}
