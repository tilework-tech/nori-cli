//! The host's standard ACP v1 face on `/sessions`.
//!
//! Each initialized connection may attach to any number of sessions with
//! `session/new`, `session/load` or `session/resume`.
//! An attached connection receives that session's `session/update`
//! notifications under the Nori conversation ID, from the manager journal,
//! so several clients can watch one session and a late client still sees
//! what happened before it attached. The controller also receives the
//! session's `session/request_permission` requests. See
//! `docs/remote-control.md` §12.

pub(super) mod adapter_modes;
mod attach;
pub(super) mod config;
pub(super) mod handshake;
mod list;
mod session;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use nori_harness::ConversationId;
use nori_harness::remote_agent::HostConnection;
use nori_protocol::acp::v1 as acp;
use serde_json::Value;
use serde_json::json;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::service::HostService;

/// ACP methods this face answers. Every other method keeps its existing
/// managed-host meaning.
pub(super) fn handles(method: &str) -> bool {
    matches!(
        method,
        "session/new"
            | "session/list"
            | "session/load"
            | "session/resume"
            | "session/prompt"
            | "session/close"
            | "session/delete"
            | "session/set_mode"
            | "session/set_config_option"
    )
}

/// Methods whose response may wait on an agent, so the socket must keep
/// serving the connection's other messages meanwhile.
pub(super) fn runs_concurrently(method: &str) -> bool {
    matches!(
        method,
        "session/new"
            | "session/load"
            | "session/resume"
            | "session/prompt"
            | "session/close"
            | "session/delete"
            | "session/set_config_option"
    )
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
    /// Keep an agent's own ACP error code, such as `-32000` for sign-in.
    fn from(error: anyhow::Error) -> Self {
        let code = error
            .chain()
            .find_map(|cause| {
                cause
                    .downcast_ref::<nori_harness::runtime::AgentStartFailed>()
                    .and_then(|failed| failed.code)
                    .or_else(|| {
                        cause
                            .downcast_ref::<acp::Error>()
                            .map(|error| i32::from(error.code))
                    })
            })
            .unwrap_or(-32603);
        Self {
            code,
            message: format!("{error:#}"),
        }
    }
}

/// One connection following one session.
struct Attachment {
    forwarder: JoinHandle<()>,
    /// The latest journal sequence the forwarder has handled.
    delivered: watch::Receiver<i64>,
}

struct Connection {
    handle: HostConnection,
    attached: HashMap<ConversationId, Attachment>,
}

/// A permission request the host asked a client to answer.
pub(super) struct Ask {
    client: i64,
    session: ConversationId,
    request_id: acp::RequestId,
}

#[derive(Default)]
struct FaceState {
    connections: HashMap<i64, Connection>,
    asks: HashMap<String, Ask>,
    next_ask: i64,
}

/// Per-connection state of the ACP face. Clones share the state.
#[derive(Clone, Default)]
pub(super) struct AcpFace {
    state: Arc<Mutex<FaceState>>,
}

impl AcpFace {
    pub fn opened(&self, handle: HostConnection) {
        self.lock().connections.insert(
            handle.client,
            Connection {
                handle,
                attached: HashMap::new(),
            },
        );
    }

    pub fn closed(&self, client: i64) {
        let mut state = self.lock();
        if let Some(connection) = state.connections.remove(&client) {
            for attachment in connection.attached.into_values() {
                attachment.forwarder.abort();
            }
        }
        state.asks.retain(|_, ask| ask.client != client);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FaceState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn connection(&self, client: i64) -> Result<HostConnection, AcpError> {
        self.lock()
            .connections
            .get(&client)
            .map(|connection| connection.handle.clone())
            .ok_or_else(|| AcpError::internal("the connection is not initialized"))
    }

    fn attached(&self, client: i64, session: &ConversationId) -> Option<watch::Receiver<i64>> {
        self.lock()
            .connections
            .get(&client)?
            .attached
            .get(session)
            .map(|attachment| attachment.delivered.clone())
    }

    /// Replace the connection's forwarder for `session`.
    fn attach(
        &self,
        client: i64,
        session: ConversationId,
        forwarder: JoinHandle<()>,
        delivered: watch::Receiver<i64>,
    ) {
        let mut state = self.lock();
        match state.connections.get_mut(&client) {
            Some(connection) => {
                let attachment = Attachment {
                    forwarder,
                    delivered,
                };
                if let Some(previous) = connection.attached.insert(session, attachment) {
                    previous.forwarder.abort();
                }
            }
            None => forwarder.abort(),
        }
    }

    /// Send a host-originated message to every connection following
    /// `session`.
    fn broadcast(&self, session: &ConversationId, message: Value) {
        for connection in self.lock().connections.values() {
            if connection.attached.contains_key(session) {
                connection.handle.deliver(message.clone());
            }
        }
    }

    /// Record a permission request sent to `client`, returning the JSON-RPC
    /// ID to send it under.
    fn register_ask(
        &self,
        client: i64,
        session: ConversationId,
        request_id: acp::RequestId,
    ) -> String {
        let mut state = self.lock();
        state.next_ask += 1;
        let id = format!("nori-host-{}", state.next_ask);
        state.asks.insert(
            id.clone(),
            Ask {
                client,
                session,
                request_id,
            },
        );
        id
    }

    fn take_ask(&self, client: i64, id: &str) -> Option<Ask> {
        let mut state = self.lock();
        let ask = state.asks.remove(id)?;
        (ask.client == client).then_some(ask)
    }
}

impl HostService {
    pub(super) async fn acp_request(&self, client: i64, request: &Value) -> Value {
        let method = request["method"].as_str().unwrap_or_default();
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        match self.dispatch_acp(client, method, &params).await {
            Ok(result) => json!({"jsonrpc":"2.0","id":request["id"],"result":result}),
            Err(error) => error_response(request, error),
        }
    }

    async fn dispatch_acp(
        &self,
        client: i64,
        method: &str,
        params: &Value,
    ) -> Result<Value, AcpError> {
        let params = params.clone();
        match method {
            "session/new" => self.new_session(client, &params).await,
            "session/list" => self.list_sessions(&params).await,
            "session/load" => self.load_session(client, &params, true).await,
            "session/resume" => self.load_session(client, &params, false).await,
            "session/prompt" => self.prompt(client, &params).await,
            "session/close" => self.close_session(&session_id(&params)?).await,
            "session/delete" => self.delete_session(&session_id(&params)?).await,
            "session/set_mode" => {
                let id = session_id(&params)?;
                let mode = params["modeId"]
                    .as_str()
                    .ok_or_else(|| AcpError::invalid_params("modeId is required"))?;
                self.set_mode(&id, mode).await.map(|()| json!({}))
            }
            "session/set_config_option" => {
                let id = session_id(&params)?;
                let (Some(config_id), Some(value)) =
                    (params["configId"].as_str(), params["value"].as_str())
                else {
                    return Err(AcpError::invalid_params(
                        "configId and a string value are required",
                    ));
                };
                self.set_config_option(&id, config_id, value)
                    .await
                    .map(|options| json!({"configOptions": options}))
            }
            _ => Err(AcpError {
                code: -32601,
                message: "method not found".into(),
            }),
        }
    }

    /// A client notification, or its answer to a request the host sent.
    pub(super) async fn acp_message(&self, client: i64, message: Value) {
        if message["method"] == "session/cancel" {
            let Ok(connection) = self.acp.connection(client) else {
                return;
            };
            if !connection.is_controller() {
                return;
            }
            if let Ok(id) = session_id(&message["params"])
                && let Some(handle) = self.manager.handle(&id)
            {
                let admitted = handle.submit_cancel_current_turn();
                tokio::spawn(async move {
                    if let Err(error) = admitted.await {
                        tracing::debug!(%id, %error, "session cancellation was not forwarded");
                    }
                });
            }
            return;
        }
        if let Some(id) = message["id"].as_str() {
            self.answer_permission(client, id, &message).await;
        }
    }
}

fn error_response(request: &Value, error: AcpError) -> Value {
    json!({
        "jsonrpc":"2.0",
        "id":request["id"],
        "error":{"code":error.code,"message":error.message},
    })
}

/// The session a request names.
fn session_id(params: &Value) -> Result<ConversationId, AcpError> {
    params["sessionId"]
        .as_str()
        .and_then(|id| ConversationId::from_string(id).ok())
        .ok_or_else(|| AcpError::not_found("unknown session"))
}
