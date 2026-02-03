use std::sync::Arc;

use codex_acp::AcpBackend;
use codex_acp::AcpBackendConfig;
#[cfg(feature = "unstable")]
use codex_acp::AcpModelState;
use codex_acp::HistoryPersistence;
use codex_acp::find_nori_home;
use codex_acp::get_agent_config;
use codex_acp::get_agent_display_name;
use codex_core::CodexConversation;
use codex_core::ConversationManager;
use codex_core::NewConversation;
use codex_core::config::Config;
use codex_core::protocol::Op;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::oneshot;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;

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
/// This function detects whether to use ACP mode or HTTP mode based on:
/// 1. If the model is registered in the ACP registry, use ACP mode
/// 2. If the model is NOT registered and `acp_allow_http_fallback` is true, use HTTP mode
/// 3. If the model is NOT registered and `acp_allow_http_fallback` is false (default), error
pub(crate) fn spawn_agent(
    config: Config,
    app_event_tx: AppEventSender,
    server: Arc<ConversationManager>,
) -> SpawnAgentResult {
    let acp_agent_result = get_agent_config(&config.model);

    match (acp_agent_result.is_ok(), config.acp_allow_http_fallback) {
        // Model is registered in ACP registry -> use ACP
        (true, _) => spawn_acp_agent(config, app_event_tx),

        // Model NOT registered, but HTTP fallback is allowed -> use HTTP
        (false, true) => {
            let op_tx = spawn_http_agent(config, app_event_tx, server);
            SpawnAgentResult {
                op_tx,
                #[cfg(feature = "unstable")]
                acp_handle: None,
            }
        }

        // Model NOT registered and HTTP fallback NOT allowed -> error
        (false, false) => {
            let model_name = config.model;
            let error_msg = format!(
                "Model '{model_name}' is not registered as an ACP agent. \
                 Set acp.allow_http_fallback = true to allow HTTP providers. \
                 Known ACP models: mock-model, mock-model-alt, claude, claude-acp, gemini-2.5-flash, gemini-acp"
            );
            let op_tx = spawn_error_agent(model_name, error_msg, app_event_tx);
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
/// This is used when the requested model is not a valid ACP agent.
fn spawn_error_agent(
    model_name: String,
    error_msg: String,
    app_event_tx: AppEventSender,
) -> UnboundedSender<Op> {
    let (codex_op_tx, _codex_op_rx) = unbounded_channel::<Op>();

    tokio::spawn(async move {
        tracing::error!("{}", error_msg);
        // Send AgentSpawnFailed so the user can select a different agent
        app_event_tx.send(AppEvent::AgentSpawnFailed {
            model_name,
            error: error_msg,
        });
    });

    codex_op_tx
}

/// Spawn an ACP agent backend.
///
/// This uses the `codex_acp` crate to spawn an agent subprocess and handle
/// communication via the Agent Client Protocol.
fn spawn_acp_agent(config: Config, app_event_tx: AppEventSender) -> SpawnAgentResult {
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
        // Create event channel for backend → TUI
        let (event_tx, mut event_rx) = mpsc::channel(32);

        // Create ACP backend config from codex config
        let nori_home = find_nori_home().unwrap_or_else(|_| config.cwd.clone());
        // Load NoriConfig for ACP-specific settings (os_notifications)
        let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
        // Detect auto-worktree repo root from the cwd path.
        // When auto_worktree is enabled, cwd is {repo_root}/.worktrees/{name},
        // so we can derive repo_root by going up two directories.
        let auto_worktree_enabled = nori_config.auto_worktree;
        let auto_worktree_repo_root = if auto_worktree_enabled {
            config
                .cwd
                .parent()
                .filter(|p| p.file_name().is_some_and(|n| n == ".worktrees"))
                .and_then(|p| p.parent())
                .map(std::path::Path::to_path_buf)
        } else {
            None
        };

        let acp_config = AcpBackendConfig {
            model: config.model.clone(),
            cwd: config.cwd.clone(),
            approval_policy: config.approval_policy,
            sandbox_policy: config.sandbox_policy.clone(),
            notify: config.notify.clone(),
            os_notifications: nori_config.os_notifications,
            notify_after_idle: nori_config.notify_after_idle,
            nori_home,
            history_persistence: HistoryPersistence::SaveAll,
            cli_version: env!("CARGO_PKG_VERSION").to_string(),
            auto_worktree: auto_worktree_enabled,
            auto_worktree_repo_root,
            session_start_hooks: nori_config.session_start_hooks.clone(),
            session_end_hooks: nori_config.session_end_hooks.clone(),
            script_timeout: nori_config.script_timeout.as_duration(),
        };

        let backend = match AcpBackend::spawn(&acp_config, event_tx).await {
            Ok(b) => Arc::new(b),
            Err(e) => {
                tracing::error!("failed to spawn ACP backend: {e}");
                // Send AgentSpawnFailed so the user can select a different agent
                app_event_tx.send(AppEvent::AgentSpawnFailed {
                    model_name: config.model.clone(),
                    error: format!("Failed to spawn ACP agent: {e}"),
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
                            let model_id = codex_acp::ModelId::from(model_id);
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

        // Forward events to TUI
        while let Some(event) = event_rx.recv().await {
            app_event_tx.send(AppEvent::CodexEvent(event));
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
    transcript: codex_acp::transcript::Transcript,
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
        let (event_tx, mut event_rx) = mpsc::channel(32);

        let nori_home = find_nori_home().unwrap_or_else(|_| config.cwd.clone());
        let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
        let auto_worktree_enabled = nori_config.auto_worktree;
        let auto_worktree_repo_root = if auto_worktree_enabled {
            config
                .cwd
                .parent()
                .filter(|p| p.file_name().is_some_and(|n| n == ".worktrees"))
                .and_then(|p| p.parent())
                .map(std::path::Path::to_path_buf)
        } else {
            None
        };

        let acp_config = AcpBackendConfig {
            model: config.model.clone(),
            cwd: config.cwd.clone(),
            approval_policy: config.approval_policy,
            sandbox_policy: config.sandbox_policy.clone(),
            notify: config.notify.clone(),
            os_notifications: nori_config.os_notifications,
            notify_after_idle: nori_config.notify_after_idle,
            nori_home,
            history_persistence: HistoryPersistence::SaveAll,
            cli_version: env!("CARGO_PKG_VERSION").to_string(),
            auto_worktree: auto_worktree_enabled,
            auto_worktree_repo_root,
            session_start_hooks: nori_config.session_start_hooks.clone(),
            session_end_hooks: nori_config.session_end_hooks.clone(),
            script_timeout: nori_config.script_timeout.as_duration(),
        };

        let backend = match AcpBackend::resume_session(
            &acp_config,
            acp_session_id.as_deref(),
            Some(&transcript),
            event_tx,
        )
        .await
        {
            Ok(b) => Arc::new(b),
            Err(e) => {
                tracing::error!("failed to resume ACP session: {e}");
                app_event_tx.send(AppEvent::AgentSpawnFailed {
                    model_name: config.model.clone(),
                    error: format!("Failed to resume ACP session: {e}"),
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
                            let model_id = codex_acp::ModelId::from(model_id);
                            let result = backend_for_model.set_model(&model_id).await;
                            let _ = response_tx.send(result);
                        }
                    }
                }
            });
        }

        drop(backend);

        while let Some(event) = event_rx.recv().await {
            app_event_tx.send(AppEvent::CodexEvent(event));
        }
    });

    SpawnAgentResult {
        op_tx: codex_op_tx,
        #[cfg(feature = "unstable")]
        acp_handle,
    }
}

/// Spawn an HTTP agent (the original implementation).
///
/// This uses `codex_core` to communicate with LLM providers via HTTP APIs.
fn spawn_http_agent(
    config: Config,
    app_event_tx: AppEventSender,
    server: Arc<ConversationManager>,
) -> UnboundedSender<Op> {
    let (codex_op_tx, mut codex_op_rx) = unbounded_channel::<Op>();

    // Clone model name before config is moved
    let model_name = config.model.clone();
    let app_event_tx_clone = app_event_tx;
    tokio::spawn(async move {
        let NewConversation {
            conversation_id: _,
            conversation,
            session_configured,
        } = match server.new_conversation(config).await {
            Ok(v) => v,
            #[allow(clippy::print_stderr)]
            Err(err) => {
                let message = err.to_string();
                eprintln!("{message}");
                // Send AgentSpawnFailed so the user can select a different agent
                app_event_tx_clone.send(AppEvent::AgentSpawnFailed {
                    model_name,
                    error: format!("Failed to initialize HTTP agent: {err}"),
                });
                tracing::error!("failed to initialize codex: {err}");
                return;
            }
        };

        // Forward the captured `SessionConfigured` event so it can be rendered in the UI.
        let ev = codex_core::protocol::Event {
            // The `id` does not matter for rendering, so we can use a fake value.
            id: "".to_string(),
            msg: codex_core::protocol::EventMsg::SessionConfigured(session_configured),
        };
        app_event_tx_clone.send(AppEvent::CodexEvent(ev));

        let conversation_clone = conversation.clone();
        tokio::spawn(async move {
            while let Some(op) = codex_op_rx.recv().await {
                let id = conversation_clone.submit(op).await;
                if let Err(e) = id {
                    tracing::error!("failed to submit op: {e}");
                }
            }
        });

        while let Ok(event) = conversation.next_event().await {
            app_event_tx_clone.send(AppEvent::CodexEvent(event));
        }
    });

    codex_op_tx
}

/// Spawn agent loops for an existing conversation (e.g., a forked conversation).
/// Sends the provided `SessionConfiguredEvent` immediately, then forwards subsequent
/// events and accepts Ops for submission.
pub(crate) fn spawn_agent_from_existing(
    conversation: std::sync::Arc<CodexConversation>,
    session_configured: codex_core::protocol::SessionConfiguredEvent,
    app_event_tx: AppEventSender,
) -> UnboundedSender<Op> {
    let (codex_op_tx, mut codex_op_rx) = unbounded_channel::<Op>();

    let app_event_tx_clone = app_event_tx;
    tokio::spawn(async move {
        // Forward the captured `SessionConfigured` event so it can be rendered in the UI.
        let ev = codex_core::protocol::Event {
            id: "".to_string(),
            msg: codex_core::protocol::EventMsg::SessionConfigured(session_configured),
        };
        app_event_tx_clone.send(AppEvent::CodexEvent(ev));

        let conversation_clone = conversation.clone();
        tokio::spawn(async move {
            while let Some(op) = codex_op_rx.recv().await {
                let id = conversation_clone.submit(op).await;
                if let Err(e) = id {
                    tracing::error!("failed to submit op: {e}");
                }
            }
        });

        while let Ok(event) = conversation.next_event().await {
            app_event_tx_clone.send(AppEvent::CodexEvent(event));
        }
    });

    codex_op_tx
}
