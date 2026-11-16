//! Host lifecycle commands layered on the existing managed-session protocol.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use nori_config::NoriConfig;
use nori_config::NoriConfigOverrides;
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
use nori_protocol::acp::v1::ContentBlock;
use nori_protocol::acp::v1::TextContent;
use serde_json::Value;
use serde_json::json;
use tokio::sync::Notify;

pub(super) struct HostService {
    pub manager: SessionManager,
    pub managed: ManagedRemoteHost,
    pub config: Arc<NoriConfig>,
    /// The host's command-line settings, reapplied to each session folder.
    pub overrides: NoriConfigOverrides,
    pub stop: Notify,
}

/// The canonical form of a requested session folder, which must already
/// exist as a directory on this machine.
fn session_folder(requested: &str) -> anyhow::Result<PathBuf> {
    let path = Path::new(requested);
    anyhow::ensure!(
        path.is_absolute(),
        "session folder must be an absolute path: {requested}"
    );
    let folder = dunce::canonicalize(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => {
            anyhow::anyhow!("session folder does not exist on the host: {requested}")
        }
        _ => anyhow::anyhow!("cannot open session folder {requested}: {error}"),
    })?;
    anyhow::ensure!(
        folder.is_dir(),
        "session folder is not a directory: {requested}"
    );
    Ok(folder)
}

impl HostService {
    /// The host's configuration as it applies in `folder`, so the folder's
    /// own project trust and settings hold for its sessions.
    fn config_in(&self, folder: PathBuf) -> anyhow::Result<NoriConfig> {
        NoriConfig::load_with_overrides(NoriConfigOverrides {
            cwd: Some(folder),
            ..self.overrides.clone()
        })
        .context("could not load configuration for the session folder")
    }

    fn prepare(config: NoriConfig) -> AgentPrepareSpec {
        AgentPrepareSpec {
            config: Arc::new(config),
            cli_version: env!("CARGO_PKG_VERSION").into(),
            session_context: None,
            initial_context: None,
        }
    }

    async fn lifecycle(&self, method: &str, params: &Value) -> anyhow::Result<Value> {
        if method == "_nori/host/stop" {
            self.stop.notify_one();
            return Ok(json!({"stopping":true}));
        }
        if method == "_nori/session/new" {
            let string = |key: &str| {
                params
                    .get(key)
                    .map(|value| {
                        value
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("{key} must be a string"))
                    })
                    .transpose()
            };
            let mut config = match string("cwd")? {
                Some(cwd) => self.config_in(session_folder(cwd)?)?,
                None => (*self.config).clone(),
            };
            if let Some(agent) = string("agent")? {
                config.active_agent = agent.to_owned();
            }
            let cwd = config.cwd.clone();
            let session = self.manager.start(StartSession {
                agent: SessionAgent::Prepare(Self::prepare(config)),
                start: SessionStart::New,
                parent: None,
                source: SessionSource::User,
            })?;
            if let Some(prompt) = string("prompt")? {
                let handle = session.handle.clone();
                let prompt = vec![ContentBlock::Text(TextContent::new(prompt))];
                tokio::spawn(async move {
                    if let Err(error) = handle.prompt(prompt).await {
                        tracing::warn!("initial prompt was not accepted: {error}");
                    }
                });
            }
            self.managed
                .set_selected_session(Some(session.conversation_id));
            return Ok(json!({"session_id":session.conversation_id.to_string(),"cwd":cwd}));
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
                let mut config = self.config_in(record.cwd)?;
                config.active_agent = record.agent;
                self.manager.resume(&id, Self::prepare(config)).await?;
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
            let initialize = method == "initialize";
            let mut reply = self.managed.request(request).await;
            if initialize
                && let Some(sessions) = reply
                    .response
                    .pointer_mut("/result/capabilities/nori_sessions")
            {
                sessions["session_cwd"] = json!(true);
            }
            return reply;
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
