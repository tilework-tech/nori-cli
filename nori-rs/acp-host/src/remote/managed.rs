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
    /// Keep a checkpoint current until the transport flushes its response.
    pub delivery_guard: Option<tokio::sync::OwnedMutexGuard<()>>,
    pub subscription: Option<ManagedSubscription>,
}

/// Bidirectional ACP frames for one admitted binding and generation.
pub struct StandardAcpConnection {
    pub input: mpsc::Sender<Value>,
    pub output: mpsc::Receiver<Value>,
    pub finished: tokio::task::JoinHandle<anyhow::Result<()>>,
}

/// One initialized client connection on an authenticated host socket.
///
/// The host may write any JSON-RPC message to `outbound` at any time:
/// notifications, its own requests to the client, and responses to
/// requests it chose to run concurrently. Messages are delivered in send
/// order. When the queue is full the host should cancel `close` rather than
/// wait, so a slow client never holds up a session.
#[derive(Clone)]
pub struct HostConnection {
    pub client: i64,
    pub outbound: mpsc::Sender<Value>,
    pub close: tokio_util::sync::CancellationToken,
    /// `"controller"` or `"watcher"`; changes when control moves.
    pub role: tokio::sync::watch::Receiver<&'static str>,
}

impl HostConnection {
    /// Queue a message for the client, closing the connection if it cannot
    /// keep up. Returns whether the message was queued.
    pub fn deliver(&self, message: Value) -> bool {
        match self.outbound.try_send(message) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!(client = self.client, "host client fell behind; closing it");
                self.close.cancel();
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }

    pub fn is_controller(&self) -> bool {
        *self.role.borrow() == "controller"
    }
}

/// Transport-independent command handling for a managed session host.
pub trait ManagedAgent: Send + Sync + 'static {
    fn request(&self, request: Value) -> impl Future<Output = ManagedReply> + Send;

    /// A client finished `initialize`. Hosts that serve per-connection
    /// protocol state keep the handle until `connection_closed`.
    fn connection_opened(&self, _connection: HostConnection) {}

    fn connection_closed(&self, _client: i64) {}

    /// Handle an admitted request from a known connection.
    fn request_from(
        &self,
        _client: i64,
        request: Value,
    ) -> impl Future<Output = ManagedReply> + Send {
        self.request(request)
    }

    /// Whether `method` may take long enough that it must not hold up the
    /// connection's other requests. Its response is delivered through the
    /// connection's outbound queue instead, and any subscription it returns
    /// is ignored.
    fn runs_concurrently(&self, _method: &str) -> bool {
        false
    }

    /// A notification, or a response to one of the host's own requests,
    /// from an initialized connection. Runs concurrently with the
    /// connection's other messages.
    fn message_from(&self, _client: i64, _message: Value) -> impl Future<Output = ()> + Send {
        async {}
    }

    /// The host instance ID to announce on upgrades, when the host chose
    /// one itself. Otherwise a recovery journal's incarnation or a fresh ID
    /// is used.
    fn instance_id(&self) -> Option<String> {
        None
    }

    fn recovery(&self) -> Option<crate::recovery::SharedRecovery> {
        None
    }

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
