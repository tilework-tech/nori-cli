//! Host lifecycle commands layered on the existing managed-session protocol.

use std::sync::Arc;
use std::time::Duration;

use nori_config::NoriConfig;
use nori_harness::ConversationId;
use nori_harness::remote_agent::ManagedAgent;
use nori_harness::remote_agent::ManagedRemoteHost;
use nori_harness::remote_agent::ManagedReply;
use nori_harness::runtime::AgentPrepareSpec;
use nori_harness::runtime::SessionStart;
use nori_harness::session_manager::SessionAgent;
use nori_harness::session_manager::SessionManager;
use nori_harness::session_manager::SessionSource;
use nori_harness::session_manager::StartSession;
use serde_json::Value;
use serde_json::json;
use tokio::sync::Notify;

pub(super) struct HostService {
    pub manager: SessionManager,
    pub managed: ManagedRemoteHost,
    pub config: Arc<NoriConfig>,
    pub stop: Notify,
}

impl HostService {
    async fn lifecycle(&self, method: &str, params: &Value) -> anyhow::Result<Value> {
        if method == "_nori/host/stop" {
            self.stop.notify_one();
            return Ok(json!({"stopping":true}));
        }
        let mut config = (*self.config).clone();
        if method == "_nori/session/new" {
            if let Some(agent) = params.get("agent") {
                config.active_agent = agent
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("agent must be a string"))?
                    .to_owned();
            }
            let session = self.manager.start(StartSession {
                agent: SessionAgent::Prepare(AgentPrepareSpec {
                    config: Arc::new(config),
                    cli_version: env!("CARGO_PKG_VERSION").into(),
                    session_context: None,
                    initial_context: None,
                }),
                start: SessionStart::New,
                parent: None,
                source: SessionSource::User,
            })?;
            self.managed
                .set_selected_session(Some(session.conversation_id));
            return Ok(json!({"session_id":session.conversation_id.to_string()}));
        }
        let id = params["session_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("session_id is required"))?;
        let id = ConversationId::from_string(id)?;
        let record = self
            .manager
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("unknown session id"))?;
        match method {
            "_nori/session/close" => self.manager.close(&id, Duration::ZERO).await?,
            "_nori/session/cancel" => {
                self.manager
                    .handle(&id)
                    .ok_or_else(|| anyhow::anyhow!("session is not running"))?
                    .cancel()
                    .await?
            }
            "_nori/session/resume" => {
                config.active_agent = record.agent;
                config.cwd = record.cwd;
                self.manager
                    .resume(
                        &id,
                        AgentPrepareSpec {
                            config: Arc::new(config),
                            cli_version: env!("CARGO_PKG_VERSION").into(),
                            session_context: None,
                            initial_context: None,
                        },
                    )
                    .await?;
            }
            _ => anyhow::bail!("unknown host operation"),
        }
        Ok(json!({"session_id":id.to_string()}))
    }
}

impl ManagedAgent for HostService {
    async fn request(&self, request: Value) -> ManagedReply {
        let method = request["method"].as_str().unwrap_or_default();
        if !matches!(
            method,
            "_nori/host/stop"
                | "_nori/session/new"
                | "_nori/session/close"
                | "_nori/session/cancel"
                | "_nori/session/resume"
        ) {
            return self.managed.request(request).await;
        }
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        let result =
            if request["jsonrpc"] == "2.0" && request.get("id").is_some() && params.is_object() {
                self.lifecycle(method, &params).await
            } else {
                Err(anyhow::anyhow!(
                    "expected a JSON-RPC request with an id and object params"
                ))
            };
        let response = match result {
            Ok(result) => json!({"jsonrpc":"2.0","id":request["id"],"result":result}),
            Err(error) => {
                json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32000,"message":error.to_string()}})
            }
        };
        ManagedReply {
            response,
            subscription: None,
        }
    }
}
