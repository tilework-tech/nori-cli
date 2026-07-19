//! Session runtime: launches an ACP backend from a [`SessionLaunchSpec`] and
//! owns the orchestration that frontends previously had to implement
//! themselves — Nori config assembly, the connect/shutdown/timeout race, the
//! op-forwarding loop, the session-control command loop, and backend event
//! forwarding.
//!
//! Frontends build a spec from their own configuration source, call
//! [`launch_session`], and consume [`SessionEvent`]s; no terminal or UI
//! concepts appear here (crate-layering dependency rule 2).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use codex_protocol::protocol::Op;
use codex_rmcp_client::OAuthCredentialsStoreMode;
use futures::future::BoxFuture;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::oneshot;

use crate::backend::AcpBackend;
use crate::backend::AcpBackendConfig;
use crate::backend::BackendEvent;
use crate::connection::AcpSessionSummary;
use crate::transcript::Transcript;
use agent_client_protocol_schema::v1::SessionConfigOption;

/// Duration before showing a warning that connection is taking too long.
const CONNECT_WARNING_SECS: u64 = 8;
/// Duration after the warning before forcibly aborting the connection attempt.
const CONNECT_ABORT_SECS: u64 = 30;

/// Drain ops from the channel, discarding everything except `Op::Shutdown`.
/// Returns when `Op::Shutdown` is received or the channel is closed.
pub async fn drain_until_shutdown(rx: &mut UnboundedReceiver<Op>) {
    while let Some(op) = rx.recv().await {
        if matches!(op, Op::Shutdown) {
            return;
        }
    }
}

/// Two-phase timeout: warn after `CONNECT_WARNING_SECS`, abort after an
/// additional `CONNECT_ABORT_SECS`.
async fn spawn_timeout_sequence(event_tx: &UnboundedSender<SessionEvent>) {
    tokio::time::sleep(Duration::from_secs(CONNECT_WARNING_SECS)).await;
    let _ = event_tx.send(SessionEvent::Backend(Box::new(BackendEvent::Control(
        codex_protocol::protocol::Event {
            id: String::new(),
            msg: codex_protocol::protocol::EventMsg::Warning(
                codex_protocol::protocol::WarningEvent {
                    message: format!(
                        "Connection is taking longer than expected. \
                         Will abort in {CONNECT_ABORT_SECS}s if still unresponsive."
                    ),
                },
            ),
        },
    ))));
    tokio::time::sleep(Duration::from_secs(CONNECT_ABORT_SECS)).await;
}

/// Command for controlling ACP session state exposed by the agent.
pub enum AcpAgentCommand {
    /// Get the current ACP session config snapshot.
    GetSessionConfig {
        response_tx: oneshot::Sender<Vec<SessionConfigOption>>,
    },
    /// Set an ACP session config option.
    SetSessionConfigOption {
        config_id: String,
        value: String,
        response_tx: oneshot::Sender<anyhow::Result<Vec<SessionConfigOption>>>,
    },
    /// List the agent's known sessions via ACP `session/list`.
    ListSessions {
        cwd: PathBuf,
        response_tx: oneshot::Sender<anyhow::Result<Vec<AcpSessionSummary>>>,
    },
    /// Close (release) the active session via ACP `session/close`.
    CloseSession {
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
}

/// Handle for communicating with an ACP agent.
///
/// This handle provides access to ACP session control operations in addition
/// to the standard Op channel.
#[derive(Clone)]
pub struct AcpAgentHandle {
    command_tx: mpsc::UnboundedSender<AcpAgentCommand>,
}

impl AcpAgentHandle {
    /// Build a handle around an existing command channel. Intended for tests
    /// that fake the agent side of the channel.
    pub fn from_command_tx(command_tx: mpsc::UnboundedSender<AcpAgentCommand>) -> Self {
        Self { command_tx }
    }

    /// Get the current ACP session config snapshot from the agent.
    pub async fn get_session_config(&self) -> Option<Vec<SessionConfigOption>> {
        let (response_tx, response_rx) = oneshot::channel();
        if self
            .command_tx
            .send(AcpAgentCommand::GetSessionConfig { response_tx })
            .is_err()
        {
            return None;
        }
        response_rx.await.ok()
    }

    /// Set an ACP session config option value.
    pub async fn set_session_config_option(
        &self,
        config_id: String,
        value: String,
    ) -> anyhow::Result<Vec<SessionConfigOption>> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(AcpAgentCommand::SetSessionConfigOption {
                config_id,
                value,
                response_tx,
            })
            .map_err(|_| anyhow::anyhow!("ACP agent command channel closed"))?;
        response_rx
            .await
            .map_err(|_| anyhow::anyhow!("ACP agent did not respond"))?
    }

    /// List the agent's known sessions via ACP `session/list`.
    pub async fn list_sessions(&self, cwd: PathBuf) -> anyhow::Result<Vec<AcpSessionSummary>> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(AcpAgentCommand::ListSessions { cwd, response_tx })
            .map_err(|_| anyhow::anyhow!("ACP agent command channel closed"))?;
        response_rx
            .await
            .map_err(|_| anyhow::anyhow!("ACP agent did not respond"))?
    }

    /// Close (release) the active session via ACP `session/close`.
    pub async fn close_session(&self) -> anyhow::Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(AcpAgentCommand::CloseSession { response_tx })
            .map_err(|_| anyhow::anyhow!("ACP agent command channel closed"))?;
        response_rx
            .await
            .map_err(|_| anyhow::anyhow!("ACP agent did not respond"))?
    }
}

/// Resume parameters for reattaching to a previous session.
#[derive(Debug, Clone)]
pub struct SessionResume {
    /// The agent-side ACP session id, when known (enables `session/load`).
    pub acp_session_id: Option<String>,
    /// Transcript for client-side replay when the agent can't load sessions.
    pub transcript: Option<Transcript>,
}

/// Everything a frontend must supply to launch a session.
pub struct SessionLaunchSpec {
    /// Fully resolved process configuration supplied by the frontend.
    pub config: Arc<crate::config::NoriConfig>,
    /// Frontend version recorded in transcript metadata.
    pub cli_version: String,
    /// Product-level context injected into the first prompt.
    pub session_context: Option<String>,
    /// Conversation history injected into the first prompt (used by fork).
    pub initial_context: Option<String>,
    /// When set, resume the given session instead of starting fresh.
    pub resume: Option<SessionResume>,
}

/// Events emitted by a launched session, in addition to the backend's own
/// control/client events.
#[derive(Debug)]
pub enum SessionEvent {
    /// A normalized backend event (control-plane or session-domain).
    Backend(Box<BackendEvent>),
    /// The backend failed to spawn/resume, or timed out while connecting.
    SpawnFailed { error: String },
    /// `Op::Shutdown` arrived while the backend was still connecting.
    ShutdownRequested,
}

/// A running session: the op channel, the session-control handle, and the
/// stream of session events.
pub struct LaunchedSession {
    /// Sender for submitting operations to the agent.
    pub op_tx: UnboundedSender<Op>,
    /// Handle for session-control operations (config options, session list).
    pub handle: AcpAgentHandle,
    /// Session event stream; closes when the backend shuts down.
    pub events: UnboundedReceiver<SessionEvent>,
}

/// Launch an ACP agent session (or resume one, when `spec.resume` is set).
///
/// Returns immediately; connection happens on a background task. The op and
/// handle channels queue until the backend is up. If spawning fails, times
/// out, or `Op::Shutdown` arrives first, a terminal [`SessionEvent`] is
/// emitted and the event stream closes.
pub fn launch_session(spec: SessionLaunchSpec) -> LaunchedSession {
    let (codex_op_tx, mut codex_op_rx) = unbounded_channel::<Op>();
    let (agent_cmd_tx, mut agent_cmd_rx) = unbounded_channel::<AcpAgentCommand>();
    let (event_tx, event_rx) = unbounded_channel::<SessionEvent>();

    let handle = AcpAgentHandle {
        command_tx: agent_cmd_tx,
    };

    tokio::spawn(async move {
        let SessionLaunchSpec {
            config,
            cli_version,
            session_context,
            initial_context,
            resume,
        } = spec;

        // Single ACP backend channel for both control-plane and normalized
        // session-domain events.
        let (backend_event_tx, mut backend_event_rx) = mpsc::channel(32);

        // Detect auto-worktree repo root from the cwd path.
        // When auto_worktree is enabled, cwd is {repo_root}/.worktrees/{name},
        // so we can derive repo_root by going up two directories.
        let auto_worktree_repo_root = if config.auto_worktree.is_enabled() {
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
            config.auto_worktree
        } else {
            crate::config::AutoWorktree::Off
        };
        let agent = config.active_agent.clone();

        let acp_config = AcpBackendConfig {
            agent: agent.clone(),
            cwd: config.cwd.clone(),
            approval_policy: config.approval_policy,
            sandbox_policy: config.sandbox_policy.clone(),
            notify: config.notify.clone(),
            os_notifications: config.os_notifications,
            notify_after_idle: config.notify_after_idle,
            nori_home: config.nori_home.clone(),
            history_persistence: config.history_persistence,
            acp_proxy: config.acp_proxy.clone(),
            cli_version,
            auto_worktree,
            auto_worktree_repo_root,
            prompt_summary_enabled: config.footer_segment_config.prompt_summary,
            session_start_hooks: config.session_start_hooks.clone(),
            session_end_hooks: config.session_end_hooks.clone(),
            pre_user_prompt_hooks: config.pre_user_prompt_hooks.clone(),
            post_user_prompt_hooks: config.post_user_prompt_hooks.clone(),
            pre_tool_call_hooks: config.pre_tool_call_hooks.clone(),
            post_tool_call_hooks: config.post_tool_call_hooks.clone(),
            pre_agent_response_hooks: config.pre_agent_response_hooks.clone(),
            post_agent_response_hooks: config.post_agent_response_hooks.clone(),
            async_session_start_hooks: config.async_session_start_hooks.clone(),
            async_session_end_hooks: config.async_session_end_hooks.clone(),
            async_pre_user_prompt_hooks: config.async_pre_user_prompt_hooks.clone(),
            async_post_user_prompt_hooks: config.async_post_user_prompt_hooks.clone(),
            async_pre_tool_call_hooks: config.async_pre_tool_call_hooks.clone(),
            async_post_tool_call_hooks: config.async_post_tool_call_hooks.clone(),
            async_pre_agent_response_hooks: config.async_pre_agent_response_hooks.clone(),
            async_post_agent_response_hooks: config.async_post_agent_response_hooks.clone(),
            script_timeout: config.script_timeout.as_duration(),
            default_model: config.default_models.get(&agent).cloned(),
            initial_context,
            session_context,
            mcp_servers: config.mcp_servers.clone(),
            mcp_oauth_credentials_store_mode: OAuthCredentialsStoreMode::default(),
        };

        let (connect, failure_label): (BoxFuture<'_, anyhow::Result<AcpBackend>>, &str) =
            match &resume {
                None => (
                    Box::pin(AcpBackend::spawn(&acp_config, backend_event_tx)),
                    "Failed to spawn ACP agent",
                ),
                Some(resume) => (
                    Box::pin(AcpBackend::resume_session(
                        &acp_config,
                        resume.acp_session_id.as_deref(),
                        resume.transcript.as_ref(),
                        backend_event_tx,
                    )),
                    "Failed to resume ACP session",
                ),
            };

        // Race backend init against shutdown requests and a timeout.
        // This ensures the user can always exit even if the backend hangs.
        let backend = tokio::select! {
            result = connect => {
                match result {
                    Ok(b) => Arc::new(b),
                    Err(e) => {
                        tracing::error!("{failure_label}: {e}");
                        drop(codex_op_rx);
                        let _ = event_tx.send(SessionEvent::SpawnFailed {
                            error: format!("{failure_label}: {e}"),
                        });
                        return;
                    }
                }
            }
            () = drain_until_shutdown(&mut codex_op_rx) => {
                tracing::info!("shutdown requested while ACP backend was connecting");
                drop(codex_op_rx);
                let _ = event_tx.send(SessionEvent::ShutdownRequested);
                return;
            }
            () = spawn_timeout_sequence(&event_tx) => {
                tracing::warn!("ACP backend connection timed out");
                drop(codex_op_rx);
                let _ = event_tx.send(SessionEvent::SpawnFailed {
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

        let backend_for_agent = Arc::clone(&backend);
        tokio::spawn(async move {
            while let Some(cmd) = agent_cmd_rx.recv().await {
                match cmd {
                    AcpAgentCommand::GetSessionConfig { response_tx } => {
                        let state = backend_for_agent.config_options();
                        let _ = response_tx.send(state);
                    }
                    AcpAgentCommand::SetSessionConfigOption {
                        config_id,
                        value,
                        response_tx,
                    } => {
                        let result = backend_for_agent
                            .set_config_option(config_id, value)
                            .await
                            .map(|()| backend_for_agent.config_options());
                        let _ = response_tx.send(result);
                    }
                    AcpAgentCommand::ListSessions { cwd, response_tx } => {
                        let result = backend_for_agent.connection().list_sessions(&cwd).await;
                        let _ = response_tx.send(result);
                    }
                    AcpAgentCommand::CloseSession { response_tx } => {
                        let result = backend_for_agent.close_active_session().await;
                        let _ = response_tx.send(result);
                    }
                }
            }
        });

        // Drop our Arc reference - the op and agent-control tasks have their own.
        // This is necessary so that when these tasks exit, the backend is fully dropped,
        // which drops event_tx, allowing event_rx to return None and this task to exit.
        drop(backend);

        while let Some(event) = backend_event_rx.recv().await {
            if event_tx
                .send(SessionEvent::Backend(Box::new(event)))
                .is_err()
            {
                break;
            }
        }
    });

    LaunchedSession {
        op_tx: codex_op_tx,
        handle,
        events: event_rx,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol_schema::v1::SessionConfigOptionCategory;
    use agent_client_protocol_schema::v1::SessionConfigSelectOption;
    use codex_protocol::protocol::AskForApproval;
    use codex_protocol::protocol::EventMsg;
    use codex_protocol::protocol::SandboxPolicy;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    fn mode_option(current_value: &str) -> SessionConfigOption {
        SessionConfigOption::select(
            "mode",
            "Mode",
            current_value.to_string(),
            vec![
                SessionConfigSelectOption::new("plan", "Plan"),
                SessionConfigSelectOption::new("build", "Build"),
            ],
        )
        .category(SessionConfigOptionCategory::Mode)
    }

    #[tokio::test]
    async fn set_session_config_option_returns_refreshed_config_snapshot() {
        let (command_tx, mut command_rx) = unbounded_channel::<AcpAgentCommand>();
        tokio::spawn(async move {
            while let Some(command) = command_rx.recv().await {
                if let AcpAgentCommand::SetSessionConfigOption {
                    config_id: _,
                    value,
                    response_tx,
                } = command
                {
                    let _ = response_tx.send(Ok(vec![mode_option(&value)]));
                }
            }
        });
        let handle = AcpAgentHandle { command_tx };

        let config_options = handle
            .set_session_config_option("mode".to_string(), "build".to_string())
            .await
            .unwrap();

        assert_eq!(config_options, vec![mode_option("build")]);
    }

    #[tokio::test]
    async fn launch_session_uses_the_injected_resolved_config() {
        let temp = tempfile::tempdir().expect("create session directory");
        let marker = temp.path().join("hook-ran");
        let hook = temp.path().join("session-start.sh");
        std::fs::write(&hook, format!("printf injected > {}\n", marker.display()))
            .expect("write hook");

        let config = crate::config::NoriConfig {
            active_agent: "mock-model".to_string(),
            cwd: temp.path().to_path_buf(),
            nori_home: temp.path().to_path_buf(),
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::ReadOnly,
            session_start_hooks: vec![hook],
            ..Default::default()
        };

        let spec = SessionLaunchSpec {
            config: Arc::new(config),
            cli_version: "test".to_string(),
            session_context: None,
            initial_context: None,
            resume: None,
        };
        let LaunchedSession {
            op_tx,
            handle,
            mut events,
        } = launch_session(spec);

        let configured = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match events.recv().await.expect("session event stream closed") {
                    SessionEvent::Backend(event) => {
                        if let BackendEvent::Control(event) = *event
                            && let EventMsg::SessionConfigured(configured) = event.msg
                        {
                            break configured;
                        }
                    }
                    SessionEvent::SpawnFailed { error } => panic!("session spawn failed: {error}"),
                    SessionEvent::ShutdownRequested => {
                        panic!("session shut down before configuration")
                    }
                }
            }
        })
        .await
        .expect("session should configure from injected config");

        assert_eq!(configured.model, "mock-model");
        assert_eq!(configured.cwd, temp.path());
        assert_eq!(configured.approval_policy, AskForApproval::Never);
        assert_eq!(configured.sandbox_policy, SandboxPolicy::ReadOnly);
        assert_eq!(
            std::fs::read_to_string(marker).expect("read hook marker"),
            "injected"
        );
        op_tx.send(Op::Shutdown).expect("request shutdown");
        drop(op_tx);
        drop(handle);
    }
}
