//! The host's standard ACP v1 face on `/sessions`.
//!
//! Each initialized connection may attach to any number of sessions with
//! `session/load` or `session/resume`. An attached connection receives that
//! session's `session/update` notifications under the Nori conversation ID,
//! from the manager journal, so several clients can watch one session and a
//! late client still sees what happened before it attached. See
//! `docs/remote-control.md` §12.

mod attach;
mod list;

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::PoisonError;

use nori_harness::ConversationId;
use nori_harness::remote_agent::HostConnection;
use serde_json::Value;
use serde_json::json;
use tokio::task::JoinHandle;

use super::service::HostService;

/// ACP methods this face answers. Every other method keeps its existing
/// managed-host meaning.
pub(super) fn handles(method: &str) -> bool {
    matches!(method, "session/list" | "session/load" | "session/resume")
}

/// Methods whose response may wait on an agent, so the socket must keep
/// serving the connection's other messages meanwhile.
pub(super) fn runs_concurrently(method: &str) -> bool {
    matches!(method, "session/load" | "session/resume")
}

/// A JSON-RPC error for the client.
#[derive(Debug)]
pub(super) struct AcpError {
    pub code: i32,
    pub message: String,
}

impl AcpError {
    pub fn invalid_params(message: impl ToString) -> Self {
        Self {
            code: -32602,
            message: message.to_string(),
        }
    }

    pub fn not_found(message: impl ToString) -> Self {
        Self {
            code: -32002,
            message: message.to_string(),
        }
    }

    pub fn internal(error: impl std::fmt::Display) -> Self {
        Self {
            code: -32603,
            message: error.to_string(),
        }
    }
}

impl From<anyhow::Error> for AcpError {
    fn from(error: anyhow::Error) -> Self {
        Self::internal(format!("{error:#}"))
    }
}

struct Connection {
    handle: HostConnection,
    /// Forwarders of attached sessions' events to this connection.
    attached: HashMap<ConversationId, JoinHandle<()>>,
}

/// Per-connection state of the ACP face.
#[derive(Default)]
pub(super) struct AcpFace {
    connections: Mutex<HashMap<i64, Connection>>,
}

impl AcpFace {
    pub fn opened(&self, handle: HostConnection) {
        self.lock().insert(
            handle.client,
            Connection {
                handle,
                attached: HashMap::new(),
            },
        );
    }

    pub fn closed(&self, client: i64) {
        if let Some(connection) = self.lock().remove(&client) {
            for forwarder in connection.attached.into_values() {
                forwarder.abort();
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<i64, Connection>> {
        self.connections
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn connection(&self, client: i64) -> Result<HostConnection, AcpError> {
        self.lock()
            .get(&client)
            .map(|connection| connection.handle.clone())
            .ok_or_else(|| AcpError::internal("the connection is not initialized"))
    }

    /// Replace the connection's forwarder for `session`.
    fn attach(&self, client: i64, session: ConversationId, forwarder: JoinHandle<()>) {
        let mut connections = self.lock();
        match connections.get_mut(&client) {
            Some(connection) => {
                if let Some(previous) = connection.attached.insert(session, forwarder) {
                    previous.abort();
                }
            }
            None => forwarder.abort(),
        }
    }
}

impl HostService {
    pub(super) async fn acp_request(&self, client: i64, request: &Value) -> Value {
        let method = request["method"].as_str().unwrap_or_default();
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        let result = match method {
            "session/list" => self.list_sessions(&params).await,
            "session/load" => self.load_session(client, &params, true).await,
            "session/resume" => self.load_session(client, &params, false).await,
            _ => Err(AcpError {
                code: -32601,
                message: "method not found".into(),
            }),
        };
        match result {
            Ok(result) => json!({"jsonrpc":"2.0","id":request["id"],"result":result}),
            Err(error) => json!({
                "jsonrpc":"2.0",
                "id":request["id"],
                "error":{"code":error.code,"message":error.message},
            }),
        }
    }
}
