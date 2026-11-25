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

pub struct HostService {
    pub(super) terminal_context: Option<nori_harness::SessionContext>,
    pub(super) recovery: Option<super::recovery_control::RecoveryControl>,
    pub launches: Option<ManagedLaunches>,
    pub manager: SessionManager,
    pub(super) managed: ManagedRemoteHost,
    pub(super) config: Arc<NoriConfig>,
    /// The owning terminal's current configuration. Only its own foreground
    /// pipe prepares agents with it, so settings the person changed since
    /// launch apply there and nowhere else.
    pub(super) foreground_config: std::sync::Mutex<Option<Arc<NoriConfig>>>,
    /// The host's command-line settings, reapplied to each session folder.
    pub(super) overrides: NoriConfigOverrides,
    pub stop: Notify,
    /// Per-connection state of the standard ACP face.
    pub(super) acp: super::acp::AcpFace,
    /// Mode and options clients chose, reapplied on resume.
    pub(super) settings: super::acp::config::SettingsStore,
    /// The host instance ID clients see on upgrades and in `initialize`.
    pub instance: String,
    /// Sessions a client deleted during this host's lifetime.
    pub(super) deleted: std::sync::Mutex<std::collections::HashSet<ConversationId>>,
    /// Serializes restarting, replacing and deleting sessions, so two
    /// clients cannot start one session twice or revive a deleted one.
    pub(super) restarts: tokio::sync::Mutex<()>,
    /// Serializes permission mode changes, so the adapter's mode and the
    /// mode the host reports change together.
    pub(super) mode_changes: tokio::sync::Mutex<()>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LaunchScope {
    binding_id: String,
    generation: i64,
}

impl HostService {
    /// Give adapters time to clean up on EOF. The manager drains all sessions
    /// concurrently; the remaining time in its five-second shutdown budget
    /// lets the Unix child watcher kill and reap an uncooperative group.
    pub async fn shutdown_sessions(&self) -> anyhow::Result<()> {
        let report = self.manager.shutdown(Duration::from_secs(2)).await;
        anyhow::ensure!(
            report.timed_out.is_empty() && report.failed.is_empty(),
            "some sessions did not stop cleanly: {report:?}"
        );
        Ok(())
    }

    /// Build the single session manager served by every connection to this host.
    pub fn new(
        config: NoriConfig,
        overrides: NoriConfigOverrides,
        managed: Option<(ManagedLaunches, &Path)>,
    ) -> anyhow::Result<Arc<Self>> {
        Self::with_context(config, overrides, managed, None)
    }

    pub(super) fn with_context(
        config: NoriConfig,
        overrides: NoriConfigOverrides,
        managed: Option<(ManagedLaunches, &Path)>,
        terminal_context: Option<nori_harness::SessionContext>,
    ) -> anyhow::Result<Arc<Self>> {
        let (launches, recovery) = match managed {
            Some((launches, directory)) => (
                Some(launches),
                Some(super::recovery_control::RecoveryControl::open(directory)?),
            ),
            None => (None, None),
        };
        let manager = SessionManager::new(nori_harness::session_manager::SessionManagerConfig {
            max_sessions: config.max_sessions,
            ..Default::default()
        });
        let settings = super::acp::config::SettingsStore::open(&config.nori_home);
        Ok(Arc::new(Self {
            terminal_context,
            recovery,
            launches,
            managed: ManagedRemoteHost::authenticated_host(manager.clone()),
            manager,
            config: Arc::new(config),
            foreground_config: std::sync::Mutex::new(None),
            overrides,
            stop: tokio::sync::Notify::new(),
            acp: super::acp::AcpFace::default(),
            settings,
            instance: uuid::Uuid::new_v4().to_string(),
            deleted: Default::default(),
            restarts: tokio::sync::Mutex::new(()),
            mode_changes: tokio::sync::Mutex::new(()),
        }))
    }

    /// The host's configuration as it applies in `folder`, so the folder's
    /// own project trust and settings hold for its sessions.
    pub(super) async fn config_in(&self, folder: PathBuf) -> anyhow::Result<NoriConfig> {
        let overrides = NoriConfigOverrides {
            cwd: Some(folder),
            ..self.overrides.clone()
        };
        // Loading reads files and may ask git for the folder's repository.
        tokio::task::spawn_blocking(move || NoriConfig::load_with_overrides(overrides))
            .await?
            .context("could not load configuration for the session folder")
    }

    pub(super) fn prepare(config: NoriConfig) -> AgentPrepareSpec {
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
        if let (Some(launches), Some(recovery)) = (&self.launches, &self.recovery) {
            return match method {
                "initialize" => Ok(json!({
                    "agentCapabilities":{"_meta":{"nori.dev/managedHost":launches.capabilities(),"nori.dev/durableRecovery":{"version":1},"nori.dev/followUp":{"version":1}}},
                    "server_info":{"name":"nori","version":env!("CARGO_PKG_VERSION")}
                })),
                "_nori/session/list" => Ok(json!({"sessions": launches.list()?})),
                "_nori/session/ensure" => recovery.ensure(launches, params.clone()).await,
                method if method.starts_with("_nori/host/") => {
                    recovery.control(launches, method, params).await
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
                Some(cwd) => {
                    let folder = super::acp::find_paths::host_folder(cwd, &self.config.cwd);
                    self.config_in(session_folder(&folder)?).await?
                }
                None => (*self.config).clone(),
            };
            if let Some(agent) = agent {
                config.active_agent = agent.to_owned();
            }
            self.apply_host_default_mode(&mut config);
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
            tokio::spawn(super::acp::adapter_modes::apply_when_started(
                self.manager.clone(),
                session.conversation_id,
            ));
            self.managed
                .set_selected_session(Some(session.conversation_id));
            return Ok(json!({"session_id":session.conversation_id.to_string(),"cwd":cwd}));
        }
        let id = params["session_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("session_id is required"))?;
        let id = ConversationId::from_string(id)?;
        if self.is_deleted(&id) {
            anyhow::bail!("unknown session id");
        }
        let Some(_) = self.manager.get(&id) else {
            if method == "_nori/session/resume"
                && let Some(transcript) =
                    super::transcripts::load_stored(&self.config.nori_home, &id).await?
            {
                self.resume_stored(&id, transcript).await?;
                tokio::spawn(super::acp::adapter_modes::apply_when_started(
                    self.manager.clone(),
                    id,
                ));
                return Ok(json!({"session_id": id.to_string()}));
            }
            anyhow::bail!("unknown session id");
        };
        match method {
            "_nori/session/close" => self.manager.close(&id, Duration::ZERO).await?,
            "_nori/session/cancel" => {
                self.manager
                    .handle(&id)
                    .ok_or_else(|| anyhow::anyhow!("session is not running"))?
                    .cancel_current_turn()
                    .await?
            }
            "_nori/session/resume" => {
                self.resume_failed(&id).await?;
                tokio::spawn(super::acp::adapter_modes::apply_when_started(
                    self.manager.clone(),
                    id,
                ));
            }
            _ => anyhow::bail!("unknown host operation"),
        }
        Ok(json!({"session_id":id.to_string()}))
    }
}

impl HostService {
    /// Restart a failed session on a new agent process under its own ID, in
    /// its folder and with its agent.
    pub(super) async fn resume_failed(&self, id: &ConversationId) -> anyhow::Result<()> {
        let record = self
            .manager
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("unknown session id"))?;
        let mut config = self.config_in(record.cwd).await?;
        config.active_agent = record.agent;
        self.restore_settings(id, &mut config);
        self.manager.resume(id, Self::prepare(config)).await?;
        Ok(())
    }
}

impl ManagedAgent for HostService {
    fn connection_opened(&self, connection: nori_harness::remote_agent::HostConnection) {
        if self.launches.is_none() {
            self.acp.opened(connection);
        }
    }

    fn connection_closed(&self, client: i64) {
        self.acp.closed(client);
    }

    fn admit_foreground_input(
        &self,
        request: &Value,
    ) -> Option<futures::future::BoxFuture<'static, Value>> {
        self.managed.admit_input(
            request,
            nori_harness::remote_agent::HostClientOrigin::Foreground,
        )
    }

    fn runs_concurrently(&self, method: &str) -> bool {
        self.launches.is_none() && super::acp::runs_concurrently(method)
    }

    fn request_runs_concurrently(&self, request: &Value) -> bool {
        let method = request["method"].as_str().unwrap_or_default();
        self.runs_concurrently(method)
            || (self.launches.is_none()
                && (method == "session/list"
                    && request["params"]["_meta"]
                        .get(super::acp::picker::KEY)
                        .is_some()))
    }

    fn accepts_client_messages(&self) -> bool {
        self.launches.is_none()
    }

    fn instance_id(&self) -> Option<String> {
        self.launches.is_none().then(|| self.instance.clone())
    }

    async fn message_from(&self, client: i64, message: Value) {
        if self.launches.is_none() {
            self.acp_message(client, message).await;
        }
    }

    async fn request_from(&self, client: i64, request: Value) -> ManagedReply {
        let method = request["method"].as_str().unwrap_or_default();
        if self.launches.is_none() && super::acp::handles(method) {
            return ManagedReply {
                delivery_guard: None,
                response: self.acp_request(client, &request).await,
                subscription: None,
            };
        }
        if self.launches.is_none()
            && method == "_nori/session/respond"
            && let Ok(connection) = self.acp.connection(client)
        {
            return self
                .managed
                .request_with_origin(request, connection.origin)
                .await;
        }
        self.request(request).await
    }

    fn recovery(&self) -> Option<nori_harness::remote_agent::recovery::SharedRecovery> {
        self.recovery.as_ref().map(|r| r.shared.clone())
    }

    fn standard_acp_enabled(&self) -> bool {
        self.launches.is_some()
    }

    async fn open_standard_acp(
        &self,
        binding_id: &str,
        generation: i64,
    ) -> anyhow::Result<nori_harness::managed_acp::StandardAcpConnection> {
        use nori_harness::backend::managed_additions::ManagedSessionAdditions;
        let launches = self.launches.as_ref().context("managed host is disabled")?;
        let launch = launches.take_connection(binding_id, generation)?;
        let options = &launch.spec.additions;
        let additions = ManagedSessionAdditions::new(
            launch.home.join(".nori/cli"),
            launch.cwd.clone(),
            launch.spec.agent.clone(),
            options.recording,
            options.client_mcp,
        );
        let additions = if let Some(recovery) = &self.recovery
            && let Some(writers) = recovery.writers(binding_id, generation).await
        {
            additions.with_barrier(writers)
        } else {
            additions
        };
        if !launches.side_sessions_enabled() {
            return Ok(nori_harness::managed_acp::connect(
                launch.stdout,
                launch.stdin,
                launch.cwd,
                additions,
                options.prompt_echo,
                options.status_events,
            ));
        }
        let (parent, forks) = nori_harness::managed_acp::connect_with_forks(
            launch.stdout,
            launch.stdin,
            launch.cwd.clone(),
            additions,
            options.prompt_echo,
            options.status_events,
            launch.spec.agent.clone(),
        );
        Ok(nori_harness::managed_acp::route_side_sessions(
            parent,
            forks,
            launches.clone(),
            binding_id.to_owned(),
            generation,
            launch.cwd,
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
                sessions["find_paths"] = json!(true);
                sessions["queue"] = json!(true);
                sessions["history"] = json!(true);
                sessions["prompts"] = json!(true);
                sessions["goal"] = json!(true);
                sessions["follow_up"] = json!(1);
            }
            if initialize && let Some(result) = reply.response.get_mut("result") {
                super::acp::handshake::initialize(
                    result,
                    &nori_harness::resolve_agent_slug(&self.config.active_agent),
                );
            }
            return reply;
        }
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        let mut delivery_guard = None;
        let mut checkpoint_fence = None;
        let result = if request["jsonrpc"] != "2.0"
            || request.get("id").is_none()
            || !params.is_object()
        {
            Err(anyhow::anyhow!(
                "expected a JSON-RPC request with an id and object params"
            ))
        } else if method == "_nori/host/checkpoint"
            && let Some(recovery) = &self.recovery
        {
            let attempt = params
                .get("attemptId")
                .cloned()
                .unwrap_or_else(|| json!(uuid::Uuid::new_v4()));
            if attempt.as_str().is_some_and(|id| !id.is_empty()) {
                let fence = json!({"bindingId":params["bindingId"],"generation":params["generation"],"attemptId":attempt});
                checkpoint_fence = Some(fence.clone());
                recovery
                    .checkpoint(&params)
                    .await
                    .map(|(mut result, guard)| {
                        delivery_guard = Some(guard);
                        result["fence"] = fence;
                        result
                    })
            } else {
                Err(anyhow::anyhow!("attemptId must be a nonempty string"))
            }
        } else {
            self.lifecycle(method, &params).await
        };
        let response = match result {
            Ok(result) => json!({"jsonrpc":"2.0","id":request["id"],"result":result}),
            Err(error) => {
                let code = match error.downcast_ref::<LaunchRefusal>() {
                    Some(LaunchRefusal::AtCapacity) => -32017,
                    Some(LaunchRefusal::InvalidParams(_)) => -32602,
                    None => nori_harness::remote_agent::recovery::StorageFailure::rpc_code(
                        &error, -32000,
                    ),
                };
                let mut body = json!({"code":code,"message":error.to_string()});
                if matches!(method, "_nori/host/release" | "_nori/host/checkpoint")
                    && let Some(recovery) = &self.recovery
                    && let Some(watermarks) = recovery.saved_through(&params).await
                {
                    body["data"] = json!({"savedThrough":watermarks});
                }
                if let Some(fence) = checkpoint_fence {
                    body["data"]["fence"] = fence;
                }
                json!({"jsonrpc":"2.0","id":request["id"],"error":body})
            }
        };
        ManagedReply {
            delivery_guard,
            response,
            subscription: None,
        }
    }
}
