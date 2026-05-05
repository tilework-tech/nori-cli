use std::sync::Arc;
use std::time::Duration;

use codex_core::config::Config;
use codex_core::protocol::Op;
use nori_acp::AcpBackend;
use nori_acp::AcpBackendConfig;
#[cfg(feature = "unstable")]
use nori_acp::AcpModelState;
use nori_acp::HistoryPersistence;
use nori_acp::find_nori_home;
use nori_acp::get_agent_config;
use nori_acp::get_agent_display_name;
use nori_acp::list_available_agents;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::oneshot;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;

/// Duration before showing a warning that connection is taking too long.
const CONNECT_WARNING_SECS: u64 = 8;
/// Duration after the warning before forcibly aborting the connection attempt.
const CONNECT_ABORT_SECS: u64 = 30;

/// Drain ops from the channel, discarding everything except `Op::Shutdown`.
/// Returns when `Op::Shutdown` is received or the channel is closed.
pub(crate) async fn drain_until_shutdown(rx: &mut UnboundedReceiver<Op>) {
    while let Some(op) = rx.recv().await {
        if matches!(op, Op::Shutdown) {
            return;
        }
    }
}

/// Two-phase timeout: warn after `CONNECT_WARNING_SECS`, abort after an
/// additional `CONNECT_ABORT_SECS`.
async fn spawn_timeout_sequence(app_event_tx: &AppEventSender) {
    tokio::time::sleep(Duration::from_secs(CONNECT_WARNING_SECS)).await;
    app_event_tx.send(AppEvent::CodexEvent(codex_core::protocol::Event {
        id: String::new(),
        msg: codex_core::protocol::EventMsg::Warning(codex_core::protocol::WarningEvent {
            message: format!(
                "Connection is taking longer than expected. \
                 Will abort in {CONNECT_ABORT_SECS}s if still unresponsive."
            ),
        }),
    }));
    tokio::time::sleep(Duration::from_secs(CONNECT_ABORT_SECS)).await;
}

/// Command for controlling the ACP agent.
#[cfg(feature = "unstable")]
pub(crate) enum AcpModelCommand {
    /// Get the current model state (available models and current selection)
    GetModelState {
        response_tx: oneshot::Sender<AcpModelState>,
    },
    /// Set the active model
    SetModel {
        model_id: String,
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
}

/// Handle for communicating with an ACP agent.
///
/// This handle provides access to model switching operations in addition
/// to the standard Op channel.
#[cfg(feature = "unstable")]
#[derive(Clone)]
pub(crate) struct AcpAgentHandle {
    model_cmd_tx: mpsc::UnboundedSender<AcpModelCommand>,
}

#[cfg(feature = "unstable")]
impl AcpAgentHandle {
    /// Get the current model state from the ACP agent.
    pub async fn get_model_state(&self) -> Option<AcpModelState> {
        let (response_tx, response_rx) = oneshot::channel();
        if self
            .model_cmd_tx
            .send(AcpModelCommand::GetModelState { response_tx })
            .is_err()
        {
            return None;
        }
        response_rx.await.ok()
    }

    /// Set the active model in the ACP agent.
    pub async fn set_model(&self, model_id: String) -> anyhow::Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        self.model_cmd_tx
            .send(AcpModelCommand::SetModel {
                model_id,
                response_tx,
            })
            .map_err(|_| anyhow::anyhow!("ACP agent command channel closed"))?;
        response_rx
            .await
            .map_err(|_| anyhow::anyhow!("ACP agent did not respond"))?
    }
}

/// Result of spawning an agent, which may include an ACP handle for model control.
pub(crate) struct SpawnAgentResult {
    /// The Op sender for submitting operations to the agent.
    pub op_tx: UnboundedSender<Op>,
    /// Optional ACP handle for model control (only present in ACP mode).
    #[cfg(feature = "unstable")]
    pub acp_handle: Option<AcpAgentHandle>,
}

/// Spawn the agent bootstrapper and op forwarding loop, returning a result
/// that includes the Op sender and optionally an ACP handle for model control.
///
/// Looks up the agent in the ACP registry. If found, spawns an ACP agent.
/// Otherwise, emits an error and opens the agent picker.
pub(crate) fn spawn_agent(
    config: Config,
    app_event_tx: AppEventSender,
    fork_context: Option<String>,
) -> SpawnAgentResult {
    match get_agent_config(&config.model) {
        Ok(_) => spawn_acp_agent(config, app_event_tx, fork_context),
        Err(_) => {
            let agent_name = config.model;
            let known: Vec<String> = list_available_agents()
                .iter()
                .map(|a| a.agent_name.clone())
                .collect();
            let error_msg = format!(
                "Agent '{agent_name}' is not registered as an ACP agent. \
                 Known ACP agents: {}",
                known.join(", ")
            );
            let op_tx = spawn_error_agent(agent_name, error_msg, app_event_tx);
            SpawnAgentResult {
                op_tx,
                #[cfg(feature = "unstable")]
                acp_handle: None,
            }
        }
    }
}

/// Spawn an agent that emits an error and opens the agent picker.
///
/// This is used when the requested agent is not a valid ACP agent.
fn spawn_error_agent(
    agent_name: String,
    error_msg: String,
    app_event_tx: AppEventSender,
) -> UnboundedSender<Op> {
    let (codex_op_tx, _codex_op_rx) = unbounded_channel::<Op>();

    tokio::spawn(async move {
        tracing::error!("{}", error_msg);
        // Send AgentSpawnFailed so the user can select a different agent
        app_event_tx.send(AppEvent::AgentSpawnFailed {
            agent_name,
            error: error_msg,
        });
    });

    codex_op_tx
}

/// Spawn an ACP agent backend.
///
/// This uses the `nori_acp` crate to spawn an agent subprocess and handle
/// communication via the Agent Client Protocol.
fn spawn_acp_agent(
    config: Config,
    app_event_tx: AppEventSender,
    fork_context: Option<String>,
) -> SpawnAgentResult {
    let (codex_op_tx, mut codex_op_rx) = unbounded_channel::<Op>();

    // Create the model command channel for model switching operations
    #[cfg(feature = "unstable")]
    let (model_cmd_tx, mut model_cmd_rx) = unbounded_channel::<AcpModelCommand>();

    #[cfg(feature = "unstable")]
    let acp_handle = Some(AcpAgentHandle { model_cmd_tx });

    // Emit "Connecting" status before spawning the backend
    let display_name = get_agent_display_name(&config.model);
    app_event_tx.send(AppEvent::AgentConnecting { display_name });

    tokio::spawn(async move {
        // Create a single ACP backend → TUI channel for both control-plane
        // and normalized session-domain events.
        let (backend_event_tx, mut backend_event_rx) = mpsc::channel(32);

        // Create ACP backend config from codex config
        let nori_home = find_nori_home().unwrap_or_else(|_| config.cwd.clone());
        // Load NoriConfig for ACP-specific settings (os_notifications)
        let nori_config = nori_acp::config::NoriConfig::load().unwrap_or_default();
        // Detect auto-worktree repo root from the cwd path.
        // When auto_worktree is enabled, cwd is {repo_root}/.worktrees/{name},
        // so we can derive repo_root by going up two directories.
        let auto_worktree_repo_root = if nori_config.auto_worktree.is_enabled() {
            config
                .cwd
                .parent()
                .filter(|p| p.file_name().is_some_and(|n| n == ".worktrees"))
                .and_then(|p| p.parent())
                .map(std::path::Path::to_path_buf)
        } else {
            None
        };
        // Resolve to Off if no worktree actually exists (e.g. "ask" mode
        // where the user declined).
        let auto_worktree = if auto_worktree_repo_root.is_some() {
            nori_config.auto_worktree
        } else {
            nori_acp::config::AutoWorktree::Off
        };

        let acp_config = AcpBackendConfig {
            agent: config.model.clone(),
            cwd: config.cwd.clone(),
            approval_policy: config.approval_policy,
            sandbox_policy: config.sandbox_policy.clone(),
            notify: config.notify.clone(),
            os_notifications: nori_config.os_notifications,
            notify_after_idle: nori_config.notify_after_idle,
            nori_home,
            history_persistence: HistoryPersistence::SaveAll,
            acp_proxy: nori_config.acp_proxy.clone(),
            cli_version: env!("CARGO_PKG_VERSION").to_string(),
            auto_worktree,
            auto_worktree_repo_root,
            session_start_hooks: nori_config.session_start_hooks.clone(),
            session_end_hooks: nori_config.session_end_hooks.clone(),
            pre_user_prompt_hooks: nori_config.pre_user_prompt_hooks.clone(),
            post_user_prompt_hooks: nori_config.post_user_prompt_hooks.clone(),
            pre_tool_call_hooks: nori_config.pre_tool_call_hooks.clone(),
            post_tool_call_hooks: nori_config.post_tool_call_hooks.clone(),
            pre_agent_response_hooks: nori_config.pre_agent_response_hooks.clone(),
            post_agent_response_hooks: nori_config.post_agent_response_hooks.clone(),
            async_session_start_hooks: nori_config.async_session_start_hooks.clone(),
            async_session_end_hooks: nori_config.async_session_end_hooks.clone(),
            async_pre_user_prompt_hooks: nori_config.async_pre_user_prompt_hooks.clone(),
            async_post_user_prompt_hooks: nori_config.async_post_user_prompt_hooks.clone(),
            async_pre_tool_call_hooks: nori_config.async_pre_tool_call_hooks.clone(),
            async_post_tool_call_hooks: nori_config.async_post_tool_call_hooks.clone(),
            async_pre_agent_response_hooks: nori_config.async_pre_agent_response_hooks.clone(),
            async_post_agent_response_hooks: nori_config.async_post_agent_response_hooks.clone(),
            script_timeout: nori_config.script_timeout.as_duration(),
            default_model: nori_config.default_models.get(&config.model).cloned(),
            initial_context: fork_context,
            session_context: Some(include_str!("../../session_context.md").to_string()),
            mcp_servers: config.mcp_servers.clone(),
            mcp_oauth_credentials_store_mode: config.mcp_oauth_credentials_store_mode,
        };

        // Race backend init against shutdown requests and a timeout.
        // This ensures the user can always exit even if the backend hangs.
        let backend = tokio::select! {
            result = AcpBackend::spawn(&acp_config, backend_event_tx) => {
                match result {
                    Ok(b) => Arc::new(b),
                    Err(e) => {
                        tracing::error!("failed to spawn ACP backend: {e}");
                        drop(codex_op_rx);
                        app_event_tx.send(AppEvent::AgentSpawnFailed {
                            agent_name: config.model.clone(),
                            error: format!("Failed to spawn ACP agent: {e}"),
                        });
                        return;
                    }
                }
            }
            () = drain_until_shutdown(&mut codex_op_rx) => {
                tracing::info!("shutdown requested while ACP backend was connecting");
                drop(codex_op_rx);
                app_event_tx.send(AppEvent::ExitRequest);
                return;
            }
            () = spawn_timeout_sequence(&app_event_tx) => {
                tracing::warn!("ACP backend connection timed out");
                drop(codex_op_rx);
                app_event_tx.send(AppEvent::AgentSpawnFailed {
                    agent_name: config.model.clone(),
                    error: "Connection timed out. The agent did not respond.".to_string(),
                });
                return;
            }
        };

        // Forward ops to backend
        let backend_for_ops = Arc::clone(&backend);
        tokio::spawn(async move {
            while let Some(op) = codex_op_rx.recv().await {
                if let Err(e) = backend_for_ops.submit(op).await {
                    tracing::error!("failed to submit op: {e}");
                }
            }
        });

        // Handle model commands in a separate task
        #[cfg(feature = "unstable")]
        {
            let backend_for_model = Arc::clone(&backend);
            tokio::spawn(async move {
                while let Some(cmd) = model_cmd_rx.recv().await {
                    match cmd {
                        AcpModelCommand::GetModelState { response_tx } => {
                            let state = backend_for_model.model_state();
                            let _ = response_tx.send(state);
                        }
                        AcpModelCommand::SetModel {
                            model_id,
                            response_tx,
                        } => {
                            let model_id = nori_acp::ModelId::from(model_id);
                            let result = backend_for_model.set_model(&model_id).await;
                            let _ = response_tx.send(result);
                        }
                    }
                }
            });
        }

        // Drop our Arc reference - the op and model tasks have their own.
        // This is necessary so that when these tasks exit, the backend is fully dropped,
        // which drops event_tx, allowing event_rx to return None and this task to exit.
        drop(backend);

        while let Some(event) = backend_event_rx.recv().await {
            match event {
                nori_acp::BackendEvent::Control(event) => {
                    app_event_tx.send(AppEvent::CodexEvent(event));
                }
                nori_acp::BackendEvent::Client(client_event) => {
                    app_event_tx.send(AppEvent::ClientEvent(client_event));
                }
            }
        }
    });

    SpawnAgentResult {
        op_tx: codex_op_tx,
        #[cfg(feature = "unstable")]
        acp_handle,
    }
}

/// Spawn an ACP agent backend that resumes a previous session.
///
/// Similar to `spawn_acp_agent`, but calls `AcpBackend::resume_session`
/// instead of `AcpBackend::spawn`. If the agent supports `session/load`,
/// server-side resume is used. Otherwise, falls back to client-side replay
/// using the provided transcript.
pub(crate) fn spawn_acp_agent_resume(
    config: Config,
    acp_session_id: Option<String>,
    transcript: nori_acp::transcript::Transcript,
    app_event_tx: AppEventSender,
) -> SpawnAgentResult {
    let (codex_op_tx, mut codex_op_rx) = unbounded_channel::<Op>();

    #[cfg(feature = "unstable")]
    let (model_cmd_tx, mut model_cmd_rx) = unbounded_channel::<AcpModelCommand>();

    #[cfg(feature = "unstable")]
    let acp_handle = Some(AcpAgentHandle { model_cmd_tx });

    let display_name = get_agent_display_name(&config.model);
    app_event_tx.send(AppEvent::AgentConnecting { display_name });

    tokio::spawn(async move {
        let (backend_event_tx, mut backend_event_rx) = mpsc::channel(32);

        let nori_home = find_nori_home().unwrap_or_else(|_| config.cwd.clone());
        let nori_config = nori_acp::config::NoriConfig::load().unwrap_or_default();
        let auto_worktree_repo_root = if nori_config.auto_worktree.is_enabled() {
            config
                .cwd
                .parent()
                .filter(|p| p.file_name().is_some_and(|n| n == ".worktrees"))
                .and_then(|p| p.parent())
                .map(std::path::Path::to_path_buf)
        } else {
            None
        };
        // Resolve to Off if no worktree actually exists (e.g. "ask" mode
        // where the user declined).
        let auto_worktree = if auto_worktree_repo_root.is_some() {
            nori_config.auto_worktree
        } else {
            nori_acp::config::AutoWorktree::Off
        };

        let acp_config = AcpBackendConfig {
            agent: config.model.clone(),
            cwd: config.cwd.clone(),
            approval_policy: config.approval_policy,
            sandbox_policy: config.sandbox_policy.clone(),
            notify: config.notify.clone(),
            os_notifications: nori_config.os_notifications,
            notify_after_idle: nori_config.notify_after_idle,
            nori_home,
            history_persistence: HistoryPersistence::SaveAll,
            acp_proxy: nori_config.acp_proxy.clone(),
            cli_version: env!("CARGO_PKG_VERSION").to_string(),
            auto_worktree,
            auto_worktree_repo_root,
            session_start_hooks: nori_config.session_start_hooks.clone(),
            session_end_hooks: nori_config.session_end_hooks.clone(),
            pre_user_prompt_hooks: nori_config.pre_user_prompt_hooks.clone(),
            post_user_prompt_hooks: nori_config.post_user_prompt_hooks.clone(),
            pre_tool_call_hooks: nori_config.pre_tool_call_hooks.clone(),
            post_tool_call_hooks: nori_config.post_tool_call_hooks.clone(),
            pre_agent_response_hooks: nori_config.pre_agent_response_hooks.clone(),
            post_agent_response_hooks: nori_config.post_agent_response_hooks.clone(),
            async_session_start_hooks: nori_config.async_session_start_hooks.clone(),
            async_session_end_hooks: nori_config.async_session_end_hooks.clone(),
            async_pre_user_prompt_hooks: nori_config.async_pre_user_prompt_hooks.clone(),
            async_post_user_prompt_hooks: nori_config.async_post_user_prompt_hooks.clone(),
            async_pre_tool_call_hooks: nori_config.async_pre_tool_call_hooks.clone(),
            async_post_tool_call_hooks: nori_config.async_post_tool_call_hooks.clone(),
            async_pre_agent_response_hooks: nori_config.async_pre_agent_response_hooks.clone(),
            async_post_agent_response_hooks: nori_config.async_post_agent_response_hooks.clone(),
            script_timeout: nori_config.script_timeout.as_duration(),
            default_model: nori_config.default_models.get(&config.model).cloned(),
            initial_context: None,
            session_context: Some(include_str!("../../session_context.md").to_string()),
            mcp_servers: config.mcp_servers.clone(),
            mcp_oauth_credentials_store_mode: config.mcp_oauth_credentials_store_mode,
        };

        // Race backend resume against shutdown requests and a timeout.
        let backend = tokio::select! {
            result = AcpBackend::resume_session(
                &acp_config,
                acp_session_id.as_deref(),
                Some(&transcript),
                backend_event_tx,
            ) => {
                match result {
                    Ok(b) => Arc::new(b),
                    Err(e) => {
                        tracing::error!("failed to resume ACP session: {e}");
                        drop(codex_op_rx);
                        app_event_tx.send(AppEvent::AgentSpawnFailed {
                            agent_name: config.model.clone(),
                            error: format!("Failed to resume ACP session: {e}"),
                        });
                        return;
                    }
                }
            }
            () = drain_until_shutdown(&mut codex_op_rx) => {
                tracing::info!("shutdown requested while resuming ACP session");
                drop(codex_op_rx);
                app_event_tx.send(AppEvent::ExitRequest);
                return;
            }
            () = spawn_timeout_sequence(&app_event_tx) => {
                tracing::warn!("ACP session resume timed out");
                drop(codex_op_rx);
                app_event_tx.send(AppEvent::AgentSpawnFailed {
                    agent_name: config.model.clone(),
                    error: "Connection timed out. The agent did not respond.".to_string(),
                });
                return;
            }
        };

        let backend_for_ops = Arc::clone(&backend);
        tokio::spawn(async move {
            while let Some(op) = codex_op_rx.recv().await {
                if let Err(e) = backend_for_ops.submit(op).await {
                    tracing::error!("failed to submit op: {e}");
                }
            }
        });

        #[cfg(feature = "unstable")]
        {
            let backend_for_model = Arc::clone(&backend);
            tokio::spawn(async move {
                while let Some(cmd) = model_cmd_rx.recv().await {
                    match cmd {
                        AcpModelCommand::GetModelState { response_tx } => {
                            let state = backend_for_model.model_state();
                            let _ = response_tx.send(state);
                        }
                        AcpModelCommand::SetModel {
                            model_id,
                            response_tx,
                        } => {
                            let model_id = nori_acp::ModelId::from(model_id);
                            let result = backend_for_model.set_model(&model_id).await;
                            let _ = response_tx.send(result);
                        }
                    }
                }
            });
        }

        drop(backend);

        while let Some(event) = backend_event_rx.recv().await {
            match event {
                nori_acp::BackendEvent::Control(event) => {
                    app_event_tx.send(AppEvent::CodexEvent(event));
                }
                nori_acp::BackendEvent::Client(client_event) => {
                    app_event_tx.send(AppEvent::ClientEvent(client_event));
                }
            }
        }
    });

    SpawnAgentResult {
        op_tx: codex_op_tx,
        #[cfg(feature = "unstable")]
        acp_handle,
    }
}
