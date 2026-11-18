//! Host lifecycle commands layered on the existing managed-session protocol.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use nori_config::NoriConfig;
use nori_config::NoriConfigOverrides;
use nori_harness::ConversationId;
use nori_harness::managed_launch::LaunchRefusal;
use nori_harness::managed_launch::ManagedLaunches;
use nori_harness::managed_launch::session_folder;
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
    pub launches: Option<ManagedLaunches>,
    pub manager: SessionManager,
    pub managed: ManagedRemoteHost,
    pub config: Arc<NoriConfig>,
    /// The host's command-line settings, reapplied to each session folder.
    pub overrides: NoriConfigOverrides,
    pub stop: Notify,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LaunchScope {
    binding_id: String,
    generation: i64,
}

impl HostService {
    /// The host's configuration as it applies in `folder`, so the folder's
    /// own project trust and settings hold for its sessions.
    async fn config_in(&self, folder: PathBuf) -> anyhow::Result<NoriConfig> {
        let overrides = NoriConfigOverrides {
            cwd: Some(folder),
            ..self.overrides.clone()
        };
        // Loading reads files and may ask git for the folder's repository.
        tokio::task::spawn_blocking(move || NoriConfig::load_with_overrides(overrides))
            .await?
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
        if let Some(launches) = &self.launches {
            return match method {
                "initialize" => Ok(json!({
                    "agentCapabilities":{"_meta":{"nori.dev/managedHost":launches.capabilities()}},
                    "server_info":{"name":"nori","version":env!("CARGO_PKG_VERSION")}
                })),
                "_nori/session/ensure" => {
                    let request = serde_json::from_value(params.clone())
                        .map_err(|error| LaunchRefusal::InvalidParams(error.into()))?;
                    Ok(serde_json::to_value(launches.ensure(request)?)?)
                }
                "_nori/session/status" => {
                    let scope: LaunchScope = serde_json::from_value(params.clone())?;
                    Ok(serde_json::to_value(
                        launches.status(&scope.binding_id, scope.generation)?,
                    )?)
                }
                _ => anyhow::bail!("unsupported managed host operation"),
            };
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
            let (cwd, agent, prompt) = (string("cwd")?, string("agent")?, string("prompt")?);
            let mut config = match cwd {
                Some(cwd) => self.config_in(session_folder(Path::new(cwd))?).await?,
                None => (*self.config).clone(),
            };
            if let Some(agent) = agent {
                config.active_agent = agent.to_owned();
            }
            let cwd = config
                .cwd
                .to_str()
                .context("session folder is not valid UTF-8")?
                .to_owned();
            let session = self.manager.start(StartSession {
                agent: SessionAgent::Prepare(Self::prepare(config)),
                start: SessionStart::New,
                parent: None,
                source: SessionSource::User,
            })?;
            if let Some(prompt) = prompt {
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
                let mut config = self.config_in(record.cwd).await?;
                config.active_agent = record.agent;
                self.manager.resume(&id, Self::prepare(config)).await?;
            }
            _ => anyhow::bail!("unknown host operation"),
        }
        Ok(json!({"session_id":id.to_string()}))
    }
}

impl ManagedAgent for HostService {
    fn standard_acp_enabled(&self) -> bool {
        self.launches.is_some()
    }

    async fn open_standard_acp(
        &self,
        binding_id: &str,
        generation: i64,
    ) -> anyhow::Result<nori_harness::managed_acp::StandardAcpConnection> {
        use nori_harness::backend::managed_additions::ManagedSessionAdditions;
        let launch = self
            .launches
            .as_ref()
            .context("managed host is disabled")?
            .take_connection(binding_id, generation)?;
        let options = &launch.spec.additions;
        let additions = ManagedSessionAdditions::new(
            launch.home.join(".nori/cli"),
            launch.cwd.clone(),
            launch.spec.agent.clone(),
            options.recording,
            options.client_mcp,
        );
        Ok(nori_harness::managed_acp::connect(
            launch.stdout,
            launch.stdin,
            launch.cwd,
            additions,
            options.prompt_echo,
            options.status_events,
        ))
    }

    async fn request(&self, request: Value) -> ManagedReply {
        let method = request["method"].as_str().unwrap_or_default();
        if self.launches.is_none()
            && !matches!(
                method,
                "_nori/host/stop"
                    | "_nori/session/new"
                    | "_nori/session/close"
                    | "_nori/session/cancel"
                    | "_nori/session/resume"
            )
        {
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
                let code = match error.downcast_ref::<LaunchRefusal>() {
                    Some(LaunchRefusal::AtCapacity) => -32017,
                    Some(LaunchRefusal::InvalidParams(_)) => -32602,
                    None => -32000,
                };
                json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":code,"message":error.to_string()}})
            }
        };
        ManagedReply {
            response,
            subscription: None,
        }
    }
}
