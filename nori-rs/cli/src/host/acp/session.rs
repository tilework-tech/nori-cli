//! Session lifecycle on the ACP face: `session/new`, `session/prompt`,
//! `session/close`, `session/delete`, and answers to permission requests.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use nori_config::McpServerConfig;
use nori_harness::ConversationId;
use nori_harness::managed_launch::session_folder;
use nori_harness::permission_mode::PermissionMode;
use nori_harness::runtime::SessionStart;
use nori_harness::session_manager::SessionAgent;
use nori_harness::session_manager::SessionSource;
use nori_harness::session_manager::StartError;
use nori_harness::session_manager::StartSession;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use nori_protocol::acp::v1 as acp;
use serde_json::Value;
use serde_json::json;

use super::AcpError;
use super::attach::ATTACHED_QUEUE;
use crate::host::service::HostService;

/// An ACP `mcpServers` entry as the agent configuration it becomes.
fn mcp_server(server: &acp::McpServer) -> Result<(String, McpServerConfig), AcpError> {
    let (name, raw) = match server {
        acp::McpServer::Stdio(server) => (
            server.name.clone(),
            json!({
                "command": server.command,
                "args": server.args,
                "env": server
                    .env
                    .iter()
                    .map(|variable| (variable.name.clone(), variable.value.clone()))
                    .collect::<HashMap<_, _>>(),
            }),
        ),
        acp::McpServer::Http(server) => (
            server.name.clone(),
            json!({
                "url": server.url,
                "http_headers": server
                    .headers
                    .iter()
                    .map(|header| (header.name.clone(), header.value.clone()))
                    .collect::<HashMap<_, _>>(),
            }),
        ),
        _ => {
            return Err(AcpError::invalid_params(
                "only stdio and http MCP servers are supported",
            ));
        }
    };
    let config = serde_json::from_value(raw).map_err(AcpError::invalid_params)?;
    Ok((name, config))
}

impl HostService {
    pub(super) fn is_deleted(&self, id: &ConversationId) -> bool {
        self.deleted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(id)
    }

    fn mark_deleted(&self, id: ConversationId) {
        self.deleted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id);
    }

    /// `session/new`: start a session in `cwd` with the client's MCP servers,
    /// attach the connection, and answer once the agent has started.
    pub(super) async fn new_session(&self, client: i64, params: &Value) -> Result<Value, AcpError> {
        let cwd = params["cwd"]
            .as_str()
            .map(Path::new)
            .filter(|cwd| cwd.is_absolute())
            .ok_or_else(|| AcpError::invalid_params("cwd must be an absolute path"))?;
        let folder =
            session_folder(cwd).map_err(|error| AcpError::invalid_params(format!("{error:#}")))?;
        let servers: Vec<acp::McpServer> = match params.get("mcpServers") {
            None | Some(Value::Null) => Vec::new(),
            Some(servers) => serde_json::from_value(servers.clone())
                .map_err(|error| AcpError::invalid_params(format!("mcpServers: {error}")))?,
        };
        let options: nori_protocol::NewSessionOptions =
            match params.pointer("/_meta/nori.dev~1session") {
                None | Some(Value::Null) => Default::default(),
                Some(options) => serde_json::from_value(options.clone()).map_err(|error| {
                    AcpError::invalid_params(format!("_meta.nori.dev/session: {error}"))
                })?,
            };
        if let Some(agent) = &options.agent {
            let known = nori_harness::registry::list_available_agents()
                .iter()
                .any(|available| available.agent_name == *agent);
            if !known {
                return Err(AcpError::invalid_params(format!("unknown agent {agent}")));
            }
        }
        let agent = options.agent;
        let mut config = self.config_in(folder).await?;
        if let Some(agent) = agent {
            config.active_agent = agent;
        }
        for server in &servers {
            let (name, server) = mcp_server(server)?;
            config.mcp_servers.insert(name, server);
        }
        if let Some(mode) = self.config.host.default_mode {
            let mode = match mode {
                nori_config::HostDefaultMode::Ask => PermissionMode::Ask,
                nori_config::HostDefaultMode::Agent => PermissionMode::Agent,
                nori_config::HostDefaultMode::ApproveAll => PermissionMode::ApproveAll,
            };
            config.approval_policy = mode.approval_policy();
        }
        let session = self
            .manager
            .start(StartSession {
                agent: SessionAgent::Prepare(Self::prepare(config)),
                start: SessionStart::New,
                parent: None,
                source: SessionSource::User,
            })
            .map_err(|error| match error {
                StartError::CapacityReached { .. } => AcpError {
                    code: -32017,
                    message: error.to_string(),
                },
                error => AcpError::internal(error),
            })?;
        let id = session.conversation_id;
        if let Err(error) = self.attach_new(client, &id).await {
            // A session that never started is not the client's to see.
            self.mark_deleted(id);
            return Err(error);
        }
        self.managed.set_selected_session(Some(id));
        let (options, modes) = self.session_configuration(&id).await?;
        Ok(json!({"sessionId": id.to_string(), "configOptions": options, "modes": modes}))
    }

    /// `session/prompt`: run one turn and answer with its stop reason once
    /// this connection has received the turn's updates.
    pub(super) async fn prompt(&self, client: i64, params: &Value) -> Result<Value, AcpError> {
        let id = super::session_id(params)?;
        if self.is_deleted(&id) {
            return Err(AcpError::not_found("unknown session"));
        }
        let prompt: Vec<acp::ContentBlock> = serde_json::from_value(params["prompt"].clone())
            .map_err(|error| AcpError::invalid_params(format!("prompt: {error}")))?;
        let meta: Option<acp::Meta> = params
            .get("_meta")
            .and_then(|meta| serde_json::from_value(meta.clone()).ok());
        let handle = self.manager.handle(&id).ok_or_else(|| {
            AcpError::invalid_params("the session is not running; load or resume it")
        })?;
        let delivered = match self.acp.attached(client, &id) {
            Some(delivered) => delivered,
            None => {
                let session = id.to_string();
                let cwd = self
                    .manager
                    .get(&id)
                    .map(|record| record.cwd)
                    .ok_or_else(|| AcpError::not_found("unknown session"))?;
                self.load_session(client, &json!({"sessionId": session, "cwd": cwd}), false)
                    .await?;
                self.acp
                    .attached(client, &id)
                    .ok_or_else(|| AcpError::internal("the connection closed"))?
            }
        };
        let latest = self
            .manager
            .replay_info(&id)
            .map_or(0, |info| info.latest_seq);
        let mut events = self
            .manager
            .subscribe_from(
                Some(id),
                &HashMap::from([(id.to_string(), latest)]),
                ATTACHED_QUEUE,
            )
            .map_err(AcpError::internal)?
            .events;
        let request_id = handle
            .prompt_with_meta_if_idle(prompt, meta)
            .await
            .map_err(|error| {
                if error
                    .downcast_ref::<nori_harness::backend::PromptBusy>()
                    .is_some()
                {
                    AcpError {
                        code: -32015,
                        message: "the session already has an active turn".to_owned(),
                    }
                } else {
                    AcpError::from(error)
                }
            })?;
        let (seq, result) = loop {
            let Some(envelope) = events.recv().await else {
                return Err(AcpError::internal("the session stopped reporting events"));
            };
            match envelope.event {
                SessionEvent::Acp(AcpEvent::Response {
                    request_id: answered,
                    response,
                }) if answered == request_id => {
                    let result = match response {
                        Ok(acp::AgentResponse::PromptResponse(response)) => {
                            serde_json::to_value(response).map_err(AcpError::internal)
                        }
                        Ok(_) => Err(AcpError::internal("the agent answered the prompt oddly")),
                        Err(error) => Err(AcpError {
                            code: i32::from(error.code),
                            message: error.message,
                        }),
                    };
                    break (envelope.seq, result);
                }
                SessionEvent::Nori(NoriEvent::RequestFailed(failure))
                    if failure.request_id.as_ref() == Some(&request_id) =>
                {
                    break (
                        envelope.seq,
                        Err(AcpError {
                            code: -32000,
                            message: failure.message,
                        }),
                    );
                }
                SessionEvent::Nori(NoriEvent::SessionEnded(ended)) => {
                    break (
                        envelope.seq,
                        Err(AcpError::internal(ended.message.unwrap_or_else(|| {
                            "the session ended during the turn".to_owned()
                        }))),
                    );
                }
                _ => {}
            }
        };
        // Answer only after this connection has the turn's last update.
        let mut delivered = delivered;
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            delivered.wait_for(|delivered| *delivered >= seq),
        )
        .await;
        result
    }

    /// `session/close`: stop the session's agent. Its transcript stays, so
    /// it remains listed and can be loaded again.
    pub(super) async fn close_session(&self, id: &ConversationId) -> Result<Value, AcpError> {
        if self.manager.get(id).is_none() || self.is_deleted(id) {
            return Err(AcpError::not_found("unknown session"));
        }
        self.manager.close(id, Duration::ZERO).await?;
        Ok(json!({}))
    }

    /// `session/delete`: close the session if it runs, remove its transcript
    /// under `$NORI_HOME`, and leave it out of `session/list`.
    pub(super) async fn delete_session(&self, id: &ConversationId) -> Result<Value, AcpError> {
        if self.is_deleted(id) {
            return Err(AcpError::not_found("unknown session"));
        }
        let path = match self.manager.get(id) {
            Some(record) => {
                self.manager.close(id, Duration::ZERO).await?;
                record.transcript_path
            }
            None => {
                let loader = nori_harness::TranscriptLoader::new(self.config.nori_home.clone());
                loader
                    .find_stored_session(&id.to_string())
                    .await
                    .map_err(AcpError::internal)?
                    .map(|stored| stored.path)
            }
        };
        let Some(path) = path else {
            return Err(AcpError::not_found("unknown session"));
        };
        if path.starts_with(&self.config.nori_home) {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(AcpError::internal(error)),
            }
        }
        self.mark_deleted(*id);
        self.settings
            .update(id, |settings| *settings = Default::default());
        Ok(json!({}))
    }

    /// A client's answer to `session/request_permission`. The first answer
    /// to a request wins; later ones are ignored.
    pub(super) async fn answer_permission(&self, client: i64, id: &str, message: &Value) {
        let Some(ask) = self.acp.take_ask(client, id) else {
            return;
        };
        let Ok(connection) = self.acp.connection(client) else {
            return;
        };
        if !connection.is_controller() {
            return;
        }
        let response = message
            .get("result")
            .cloned()
            .filter(|result| {
                serde_json::from_value::<acp::RequestPermissionResponse>(result.clone()).is_ok()
            })
            .unwrap_or_else(|| json!({"outcome": {"outcome": "cancelled"}}));
        let reply = nori_harness::remote_agent::ManagedAgent::request(
            &self.managed,
            json!({
                "jsonrpc": "2.0",
                "id": 0,
                "method": "_nori/session/respond",
                "params": {
                    "session_id": ask.session.to_string(),
                    "request_id": ask.request_id,
                    "response": response,
                },
            }),
        )
        .await;
        if let Some(error) = reply.response.get("error") {
            tracing::debug!(%error, "a permission answer was not used");
        }
    }
}
