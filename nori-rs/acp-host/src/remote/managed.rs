//! Nori's managed-session wire boundary, shared by WebSocket and stdio frontends.

use std::future::Future;

use nori_protocol::SessionEnvelope;
use serde_json::Value;
use tokio::sync::mpsc;

/// History and live events obtained at one atomic subscription boundary.
pub struct ManagedSubscription {
    pub replay: Vec<SessionEnvelope>,
    pub events: mpsc::Receiver<SessionEnvelope>,
}

/// One JSON-RPC response and an optional replacement event subscription.
pub struct ManagedReply {
    pub response: Value,
    pub subscription: Option<ManagedSubscription>,
}

/// Bidirectional ACP frames for one admitted binding and generation.
pub struct StandardAcpConnection {
    pub input: mpsc::Sender<Value>,
    pub output: mpsc::Receiver<Value>,
}

/// Transport-independent command handling for a managed session host.
pub trait ManagedAgent: Send + Sync + 'static {
    fn request(&self, request: Value) -> impl Future<Output = ManagedReply> + Send;

    fn standard_acp_enabled(&self) -> bool {
        false
    }

    fn open_standard_acp(
        &self,
        _binding_id: &str,
        _generation: i64,
    ) -> impl Future<Output = anyhow::Result<StandardAcpConnection>> + Send {
        async { anyhow::bail!("standard ACP is disabled") }
    }
}

/// Encode the complete manager envelope without changing its event vocabulary.
pub fn managed_event(envelope: &SessionEnvelope) -> Value {
    serde_json::json!({"jsonrpc":"2.0", "method":"_nori/session/event", "params":envelope})
}
