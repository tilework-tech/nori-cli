//! Managed session commands shared by the remote WebSocket and exec frontends.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::PoisonError;

use nori_acp_host::remote::ManagedAgent;
use nori_acp_host::remote::ManagedReply;
use nori_acp_host::remote::ManagedSubscription;
use nori_protocol::acp::v1 as acp;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

use crate::ConversationId;
use crate::remote_agent::REMOTE_CONTROL_RESPONDER;
use crate::session_manager::SessionManager;

const REMOTE_QUEUE_EVENTS: i32 = 256;

/// A transport-independent view of a manager. Disconnection does not stop sessions.
pub struct ManagedRemoteHost {
    manager: SessionManager,
    authenticated_controller: bool,
    selected: Mutex<Option<ConversationId>>,
    answers: tokio::sync::Mutex<Vec<(ConversationId, acp::RequestId)>>,
}

#[derive(Deserialize)]
struct SubscribeParams {
    session_id: Option<String>,
    #[serde(default)]
    cursors: HashMap<String, i64>,
}

#[derive(Deserialize)]
struct PromptParams {
    session_id: Option<String>,
    prompt: Vec<acp::ContentBlock>,
    meta: Option<acp::Meta>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CancelObservedParams {
    session_id: String,
    observed_turn_id: String,
}

#[derive(Deserialize)]
struct RespondParams {
    session_id: Option<String>,
    request_id: acp::RequestId,
    response: acp::RequestPermissionResponse,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DelegateParams {
    session_id: Option<String>,
    agent: String,
    task: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolveDelegationParams {
    session_id: Option<String>,
    request_id: String,
    approved: bool,
}

struct RemoteError {
    code: i32,
    message: String,
}

impl RemoteError {
    fn new(code: i32, message: impl ToString) -> Self {
        Self {
            code,
            message: message.to_string(),
        }
    }
}

impl ManagedRemoteHost {
    pub fn new(manager: SessionManager) -> Self {
        Self {
            manager,
            authenticated_controller: false,
            selected: Mutex::new(None),
            answers: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    /// The background host checks the bearer and controller role before dispatch.
    pub fn authenticated_host(manager: SessionManager) -> Self {
        Self {
            authenticated_controller: true,
            ..Self::new(manager)
        }
    }

    /// Change the default for commands that omit an explicit target.
    /// Explicit targets and existing subscriptions retain their own identity.
    pub fn set_selected_session(&self, selected: Option<ConversationId>) {
        *self.selected.lock().unwrap_or_else(PoisonError::into_inner) = selected;
    }

    fn target(&self, requested: Option<&str>) -> Result<ConversationId, RemoteError> {
        let id = match requested {
            Some(id) => ConversationId::from_string(id)
                .map_err(|_| RemoteError::new(-32002, "unknown session id"))?,
            None => self
                .selected
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .ok_or_else(|| {
                    RemoteError::new(
                        -32002,
                        "no selected session; choose a session_id from _nori/session/list",
                    )
                })?,
        };
        if self.manager.get(&id).is_none() {
            return Err(RemoteError::new(-32002, "unknown session id"));
        }
        Ok(id)
    }

    async fn dispatch(
        &self,
        method: &str,
        params: Value,
    ) -> Result<(Value, Option<ManagedSubscription>), RemoteError> {
        match method {
            "initialize" => Ok((
                json!({"capabilities":{"nori_sessions":{"version":1,"envelopes":true,"reconnect":true}},"server_info":{"name":"nori","version":env!("CARGO_PKG_VERSION")}}),
                None,
            )),
            "_nori/session/list" => {
                let selected = *self.selected.lock().unwrap_or_else(PoisonError::into_inner);
                let sessions = self.manager.list().into_iter().map(|record| {
                    let replay = self.manager.replay_info(&record.conversation_id);
                    let mut depth = 0;
                    let mut ancestor = Some(record.clone());
                    while let Some(current) = ancestor {
                        if current.source == crate::session_manager::SessionSource::Session {
                            depth += 1;
                        }
                        ancestor = current.parent.and_then(|parent| self.manager.get(&parent));
                    }
                    json!({
                        "session_id":record.conversation_id.to_string(),
                        "agent":record.agent,
                        "status":record.status,
                        "family_status":self.manager.family_status(&record.conversation_id).unwrap_or(record.status),
                        "cwd":record.cwd,
                        "parent":record.parent.map(|id| id.to_string()),
                        "pending_requests":record.pending_requests,
                        "source": match record.source {
                            crate::session_manager::SessionSource::User => "user",
                            crate::session_manager::SessionSource::Fork => "fork",
                            crate::session_manager::SessionSource::Session => "session",
                        },
                        "depth": depth,
                        "delegations": self.manager.delegations(&record.conversation_id),
                        "latest_seq":replay.as_ref().map_or(0, |info| info.latest_seq),
                        "first_available_seq":replay.as_ref().map_or(1, |info| info.first_available_seq),
                        "age_seconds":replay.as_ref().map_or(0, |info| info.age_seconds),
                        "title":replay.as_ref().and_then(|info| info.title.clone()),
                        "reply_preview":replay.as_ref().and_then(|info| info.reply_preview.clone()),
                        "waiting_request":replay.as_ref().and_then(|info| info.waiting_request.clone()),
                        "idle_seconds":replay.as_ref().map(|info| info.idle_seconds),
                        "context_used":replay.as_ref().and_then(|info| info.context).map(|(used, _)| used),
                        "context_size":replay.as_ref().and_then(|info| info.context).map(|(_, size)| size),
                        "model":record.model_name,
                    })
                }).collect::<Vec<_>>();
                Ok((
                    json!({"selected_session_id":selected.map(|id| id.to_string()),"sessions":sessions,"replay_cursors":self.manager.replay_cursors()}),
                    None,
                ))
            }
            "_nori/session/subscribe" => {
                let params: SubscribeParams = serde_json::from_value(params)
                    .map_err(|error| RemoteError::new(-32602, error))?;
                let session = if params.session_id.as_deref() == Some("*") {
                    None
                } else {
                    Some(self.target(params.session_id.as_deref())?)
                };
                let subscription = self
                    .manager
                    .subscribe_from(session, &params.cursors, REMOTE_QUEUE_EVENTS)
                    .map_err(|error| RemoteError::new(-32003, error))?;
                Ok((json!({"subscribed":true}), Some(subscription)))
            }
            crate::follow_up_rpc::METHOD if self.authenticated_controller => {
                let params = crate::follow_up_rpc::FollowUpParams::parse(params)
                    .map_err(|error| RemoteError::new(-32602, error))?;
                let id = self.target(Some(&params.session_id))?;
                let handle = self
                    .manager
                    .handle(&id)
                    .ok_or_else(|| RemoteError::new(-32002, "session is not running"))?;
                let receipt = handle
                    .follow_up(
                        params.message_id,
                        params.expected_turn,
                        params.intent,
                        params.prompt,
                        "host-controller".to_owned(),
                    )
                    .await
                    .map_err(|error| RemoteError::new(-32000, error))?;
                Ok((json!(receipt), None))
            }
            "_nori/session/cancel_observed" if self.authenticated_controller => {
                let params: CancelObservedParams = serde_json::from_value(params)
                    .map_err(|error| RemoteError::new(-32602, error))?;
                if params.observed_turn_id.trim().is_empty() {
                    return Err(RemoteError::new(-32602, "observedTurnId is required"));
                }
                let id = self.target(Some(&params.session_id))?;
                let handle = self
                    .manager
                    .handle(&id)
                    .ok_or_else(|| RemoteError::new(-32002, "session is not running"))?;
                handle
                    .cancel_external_turn(params.observed_turn_id)
                    .await
                    .map_err(|error| RemoteError::new(-32000, error))?;
                Ok((json!({"sessionId": id.to_string()}), None))
            }
            "_nori/session/prompt" => {
                let params: PromptParams = serde_json::from_value(params)
                    .map_err(|error| RemoteError::new(-32602, error))?;
                let id = self.target(params.session_id.as_deref())?;
                let handle = self
                    .manager
                    .handle(&id)
                    .ok_or_else(|| RemoteError::new(-32002, "session is not running"))?;
                let request_id = handle
                    .prompt_with_meta_if_idle(params.prompt, params.meta)
                    .await
                    .map_err(|error| {
                        let code = if error.downcast_ref::<crate::backend::PromptBusy>().is_some() {
                            -32015
                        } else {
                            -32000
                        };
                        RemoteError::new(code, error)
                    })?;
                Ok((
                    json!({"session_id":id.to_string(),"request_id":request_id}),
                    None,
                ))
            }
            "_nori/session/delegate" => {
                let params: DelegateParams = serde_json::from_value(params)
                    .map_err(|error| RemoteError::new(-32602, error))?;
                let parent = self.target(params.session_id.as_deref())?;
                let delegation = self
                    .manager
                    .delegate(&parent, params.agent, params.task)
                    .await
                    .map_err(|error| RemoteError::new(-32000, error))?;
                Ok((json!(delegation), None))
            }
            "_nori/session/delegation/resolve" => {
                let params: ResolveDelegationParams = serde_json::from_value(params)
                    .map_err(|error| RemoteError::new(-32602, error))?;
                let parent = self.target(params.session_id.as_deref())?;
                let delegation = self
                    .manager
                    .resolve_delegation(&parent, &params.request_id, params.approved)
                    .await
                    .map_err(|error| RemoteError::new(-32000, error))?;
                Ok((json!(delegation), None))
            }
            "_nori/session/respond" => {
                let params: RespondParams = serde_json::from_value(params)
                    .map_err(|error| RemoteError::new(-32602, error))?;
                let id = self.target(params.session_id.as_deref())?;
                let mut answers = self.answers.lock().await;
                // Keep claims until the manager observes their resolution, so
                // two answers cannot race its asynchronous event pump.
                answers.retain(|(session, request)| {
                    self.manager
                        .get(session)
                        .is_some_and(|record| record.pending_requests.contains(request))
                });
                let record = self
                    .manager
                    .get(&id)
                    .ok_or_else(|| RemoteError::new(-32002, "unknown session id"))?;
                if !record.pending_requests.contains(&params.request_id)
                    || answers.contains(&(id, params.request_id.clone()))
                {
                    return Err(RemoteError::new(
                        -32600,
                        "no unanswered permission request with this session and request id",
                    ));
                }
                let handle = self
                    .manager
                    .handle(&id)
                    .ok_or_else(|| RemoteError::new(-32002, "session is not running"))?
                    .with_responder(REMOTE_CONTROL_RESPONDER);
                handle
                    .respond_to_agent(
                        params.request_id.clone(),
                        Ok(acp::ClientResponse::RequestPermissionResponse(
                            params.response,
                        )),
                    )
                    .await
                    .map_err(|error| RemoteError::new(-32000, error))?;
                answers.push((id, params.request_id));
                Ok((json!({"session_id":id.to_string(),"answered":true}), None))
            }
            _ => Err(RemoteError::new(-32601, "method not found")),
        }
    }
}

impl ManagedAgent for ManagedRemoteHost {
    async fn request(&self, request: Value) -> ManagedReply {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let valid = request["jsonrpc"] == "2.0"
            && request["method"].is_string()
            && request.get("id").is_some()
            && (id.is_string() || id.is_i64() || id.is_null())
            && request.get("params").is_none_or(Value::is_object);
        let result = if valid {
            self.dispatch(
                request["method"].as_str().unwrap_or_default(),
                request.get("params").cloned().unwrap_or_else(|| json!({})),
            )
            .await
        } else {
            Err(RemoteError::new(
                -32600,
                "expected a JSON-RPC 2.0 request with an id and object params",
            ))
        };
        match result {
            Ok((result, subscription)) => ManagedReply {
                delivery_guard: None,
                response: json!({"jsonrpc":"2.0","id":id,"result":result}),
                subscription,
            },
            Err(error) => ManagedReply {
                delivery_guard: None,
                response: json!({"jsonrpc":"2.0","id":id,"error":{"code":error.code,"message":error.message}}),
                subscription: None,
            },
        }
    }
}
