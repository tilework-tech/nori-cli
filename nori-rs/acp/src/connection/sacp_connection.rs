//! SACP v11-based ACP connection layer.
//!
//! This replaces the old `AcpConnection` which required a dedicated worker thread
//! due to the `!Send` futures in `agent-client-protocol` v0.9. SACP v11's
//! `ConnectionTo<Agent>` is `Send + Sync`, allowing direct async usage from the main
//! tokio runtime without a dedicated thread or `LocalSet`.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;

use agent_client_protocol_schema as acp;
use anyhow::Context;
use anyhow::Result;
use futures::AsyncBufReadExt;
use futures::AsyncWriteExt;
use futures::StreamExt;
use futures::io::BufReader;
use sacp::Agent;
use sacp::Client;
use sacp::ConnectionTo;
use sacp::Lines;
use tokio::process::Child;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tokio_util::compat::TokioAsyncWriteCompatExt;
use tracing::debug;
use tracing::warn;

use super::AcpModelState;
use super::ApprovalEventType;
use super::ApprovalRequest;
use super::ConnectionEvent;
use super::wire_log::WireDirection;
use super::wire_log::WireLogger;
use crate::config::AcpProxyConfig;
use crate::registry::AcpAgentConfig;
use crate::translator;

#[cfg(feature = "unstable")]
use sacp::UntypedMessage;

/// Minimum supported ACP protocol version.
const MINIMUM_SUPPORTED_VERSION: acp::ProtocolVersion = acp::ProtocolVersion::V1;

#[derive(Debug, Default)]
struct SessionPromptState {
    update_seq: i64,
    draining_cancel_tail: bool,
}

/// A thread-safe connection to an ACP agent subprocess using SACP v11.
///
/// Unlike the old `AcpConnection`, this does NOT require a dedicated worker thread.
/// SACP v11's `ConnectionTo<Agent>` is `Send + Sync`, allowing all operations to run
/// directly on the main tokio runtime.
///
/// Internal architecture:
/// - A background tokio task runs the SACP connection via `connect_with`.
/// - The `ConnectionTo<Agent>` is cloned out and used for all subsequent requests.
/// - Session notifications and approval requests are forwarded via channels.
/// - All session-domain traffic flows through a single ordered inbox.
pub struct SacpConnection {
    /// Connection context for sending requests to the agent.
    cx: ConnectionTo<Agent>,

    /// Agent capabilities from the initialization handshake.
    agent_capabilities: acp::AgentCapabilities,

    /// Ordered inbox of raw ACP events from the transport layer.
    event_rx: mpsc::Receiver<ConnectionEvent>,

    /// Per-session prompt boundary state used to absorb stale terminal stop
    /// responses after cancellation without widening the public phase model.
    prompt_state: std::sync::Arc<Mutex<HashMap<String, SessionPromptState>>>,

    /// Thread-safe model state, updated on session creation and model switch.
    model_state: std::sync::Arc<std::sync::RwLock<AcpModelState>>,

    /// Handle to the background task driving the SACP connection.
    connection_task: tokio::task::JoinHandle<()>,

    /// Handle to the child process for cleanup.
    child: std::sync::Arc<Mutex<Child>>,

    /// Handle to the stderr logging task.
    stderr_task: tokio::task::JoinHandle<()>,
}

impl SacpConnection {
    /// Spawn a new ACP agent subprocess and establish a SACP v11 connection.
    pub async fn spawn(
        config: &AcpAgentConfig,
        cwd: &Path,
        proxy_config: AcpProxyConfig,
    ) -> Result<Self> {
        debug!(
            "Spawning ACP agent (SACP v11): {} {:?} in {}",
            config.command,
            config.args,
            cwd.display()
        );

        // --- Spawn the agent subprocess ---
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .envs(&config.env)
            .env_remove("CODEX_HOME")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Process group isolation and parent death signal.
        #[cfg(unix)]
        unsafe {
            #[cfg(target_os = "linux")]
            let parent_pid = libc::getpid();

            cmd.pre_exec(move || {
                if libc::setpgid(0, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }

                #[cfg(target_os = "linux")]
                {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::getppid() != parent_pid {
                        libc::raise(libc::SIGTERM);
                    }
                }

                Ok(())
            });
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn ACP agent: {}", config.command))?;

        let stdout = child.stdout.take().context("Failed to take stdout")?;
        let stdin = child.stdin.take().context("Failed to take stdin")?;
        let stderr = child.stderr.take().context("Failed to take stderr")?;

        debug!("ACP agent spawned (pid: {:?})", child.id());

        let wire_logger = if proxy_config.enabled {
            let pid = child.id().unwrap_or(0);
            Some(WireLogger::new(&proxy_config, config, pid)?)
        } else {
            None
        };

        // Log stderr in background.
        let stderr_task = tokio::spawn(async move {
            let mut stderr = BufReader::new(stderr.compat());
            let mut line = String::new();
            while let Ok(n) = stderr.read_line(&mut line).await {
                if n == 0 {
                    break;
                }
                warn!("ACP agent stderr: {}", line.trim());
                line.clear();
            }
        });

        // --- Set up channels ---
        let (event_tx, event_rx) = mpsc::channel::<ConnectionEvent>(1024);

        let event_tx_for_notifications = event_tx.clone();
        let event_tx_for_write = event_tx.clone();
        let event_tx_for_read = event_tx.clone();
        let prompt_state =
            std::sync::Arc::new(Mutex::new(HashMap::<String, SessionPromptState>::new()));
        let prompt_state_for_notifications = prompt_state.clone();
        let approval_cwd = cwd.to_path_buf();
        let write_cwd = cwd.to_path_buf();
        let read_cwd = cwd.to_path_buf();

        // Oneshot to receive the connection context and init result from inside connect_with.
        let (init_tx, init_rx) =
            oneshot::channel::<Result<(ConnectionTo<Agent>, acp::AgentCapabilities)>>();

        let child = std::sync::Arc::new(Mutex::new(child));

        let connection_task = tokio::spawn(async move {
            let outgoing_logger = wire_logger.clone();
            let outgoing_sink = futures::sink::unfold(
                Box::pin(stdin.compat_write()),
                move |mut writer, line: String| {
                    let logger = outgoing_logger.clone();
                    async move {
                        if let Some(logger) = &logger {
                            logger.record(WireDirection::ClientToAgent, &line);
                        }
                        let mut bytes = line.into_bytes();
                        bytes.push(b'\n');
                        writer.write_all(&bytes).await?;
                        Ok::<_, std::io::Error>(writer)
                    }
                },
            );

            let incoming_logger = wire_logger;
            let incoming_lines = Box::pin(BufReader::new(stdout.compat()).lines().map(
                move |line_result| {
                    if let Ok(line) = &line_result
                        && let Some(logger) = &incoming_logger
                    {
                        logger.record(WireDirection::AgentToClient, line);
                    }
                    line_result
                },
            ));
            let transport = Lines::new(outgoing_sink, incoming_lines);

            let result = Client
                .builder()
                .on_receive_notification(
                    {
                        let event_tx = event_tx_for_notifications;
                        let prompt_state = prompt_state_for_notifications;
                        async move |notification: acp::SessionNotification, _connection| {
                            let session_id = notification.session_id.to_string();
                            {
                                let mut prompt_state = prompt_state.lock().await;
                                prompt_state
                                    .entry(session_id.clone())
                                    .or_default()
                                    .update_seq += 1;
                            }
                            debug!(
                                target: "acp_event_flow",
                                session_id,
                                update_kind = super::session_update_kind(&notification.update),
                                "Transport received ACP session/update notification"
                            );
                            if event_tx
                                .send(ConnectionEvent::SessionUpdate(notification.update))
                                .await
                                .is_err()
                            {
                                warn!("Notification channel closed, dropping update");
                            }
                            Ok(())
                        }
                    },
                    sacp::on_receive_notification!(),
                )
                .on_receive_request(
                    {
                        let event_tx = event_tx.clone();
                        let cwd = approval_cwd;
                        async move |request: acp::RequestPermissionRequest,
                                    responder: sacp::Responder<acp::RequestPermissionResponse>,
                                    connection: ConnectionTo<Agent>| {
                            // Translate ACP permission request to Codex approval event.
                            let event = if let Some(patch_event) =
                                translator::permission_request_to_patch_approval_event(&request)
                            {
                                ApprovalEventType::Patch(patch_event)
                            } else {
                                let exec_event = translator::permission_request_to_approval_event(
                                    &request, &cwd,
                                );
                                ApprovalEventType::Exec(exec_event)
                            };

                            let (response_tx, response_rx) = oneshot::channel();
                            let approval = ApprovalRequest {
                                request_id: match responder.id() {
                                    serde_json::Value::String(id) => id,
                                    other => other.to_string(),
                                },
                                event,
                                acp_request: request.clone(),
                                options: request.options.clone(),
                                response_tx,
                            };

                            if event_tx
                                .send(ConnectionEvent::ApprovalRequest(approval))
                                .await
                                .is_err()
                            {
                                responder.respond(acp::RequestPermissionResponse::new(
                                    acp::RequestPermissionOutcome::Cancelled,
                                ))?;
                                return Ok(());
                            }

                            // Spawn to avoid blocking the dispatch loop.
                            connection.spawn(async move {
                                let outcome = match response_rx.await {
                                    Ok(decision) => {
                                        translator::review_decision_to_permission_outcome(
                                            decision,
                                            &request.options,
                                        )
                                    }
                                    Err(_) => {
                                        // Response channel dropped — deny.
                                        let option_id = request
                                            .options
                                            .iter()
                                            .find(|opt| {
                                                matches!(
                                                    opt.kind,
                                                    acp::PermissionOptionKind::RejectOnce
                                                        | acp::PermissionOptionKind::RejectAlways
                                                )
                                            })
                                            .map(|opt| opt.option_id.clone())
                                            .unwrap_or_else(|| {
                                                acp::PermissionOptionId::from("deny".to_string())
                                            });
                                        acp::RequestPermissionOutcome::Selected(
                                            acp::SelectedPermissionOutcome::new(option_id),
                                        )
                                    }
                                };
                                responder.respond(acp::RequestPermissionResponse::new(outcome))?;
                                Ok(())
                            })?;

                            Ok(())
                        }
                    },
                    sacp::on_receive_request!(),
                )
                .on_receive_request(
                    {
                        let event_tx = event_tx_for_write;
                        let cwd = write_cwd;
                        async move |request: acp::WriteTextFileRequest,
                                    responder: sacp::Responder<acp::WriteTextFileResponse>,
                                    _connection: ConnectionTo<Agent>| {
                            // Emit synthetic ToolCall for TUI rendering.
                            let tool_call_id = acp::ToolCallId::from(format!(
                                "write_text_file-{}",
                                request.path.display()
                            ));
                            let title = format!("Writing {}", request.path.display());
                            let tool_call = acp::ToolCall::new(tool_call_id, title)
                                .kind(acp::ToolKind::Execute)
                                .status(acp::ToolCallStatus::Pending);
                            let _ = event_tx.try_send(ConnectionEvent::SessionUpdate(
                                acp::SessionUpdate::ToolCall(tool_call),
                            ));

                            let path = &request.path;
                            let resolved_path = if path.is_relative() {
                                cwd.join(path)
                            } else {
                                path.to_path_buf()
                            };

                            // Security: restrict writes to workspace or /tmp.
                            let allowed = if let Ok(canonical) = resolved_path.canonicalize() {
                                let in_cwd = cwd
                                    .canonicalize()
                                    .map(|c| canonical.starts_with(&c))
                                    .unwrap_or(false);
                                let in_tmp = canonical.starts_with("/tmp");
                                in_cwd || in_tmp
                            } else if let Some(parent) = resolved_path.parent() {
                                if let Ok(canonical_parent) = parent.canonicalize() {
                                    let in_cwd = cwd
                                        .canonicalize()
                                        .map(|c| canonical_parent.starts_with(&c))
                                        .unwrap_or(false);
                                    let in_tmp = canonical_parent.starts_with("/tmp");
                                    in_cwd || in_tmp
                                } else {
                                    resolved_path.starts_with(&cwd)
                                        || resolved_path.starts_with("/tmp")
                                }
                            } else {
                                false
                            };

                            if !allowed {
                                responder
                                    .respond_with_error(sacp::Error::invalid_params().data(format!(
                                    "Write restricted to working directory ({}) or /tmp. Path: {}",
                                    cwd.display(),
                                    resolved_path.display()
                                )))?;
                                return Ok(());
                            }

                            // Create parent directories if needed.
                            if let Some(parent) = resolved_path.parent()
                                && !parent.exists()
                                && let Err(e) = std::fs::create_dir_all(parent)
                            {
                                responder.respond_with_error(sacp::util::internal_error(
                                    e.to_string(),
                                ))?;
                                return Ok(());
                            }

                            match std::fs::write(&resolved_path, &request.content) {
                                Ok(()) => {
                                    responder.respond(acp::WriteTextFileResponse::new())?;
                                }
                                Err(e) => {
                                    responder.respond_with_error(sacp::util::internal_error(
                                        e.to_string(),
                                    ))?;
                                }
                            }
                            Ok(())
                        }
                    },
                    sacp::on_receive_request!(),
                )
                .on_receive_request(
                    {
                        let event_tx = event_tx_for_read;
                        let cwd = read_cwd;
                        async move |request: acp::ReadTextFileRequest,
                                    responder: sacp::Responder<acp::ReadTextFileResponse>,
                                    _connection: ConnectionTo<Agent>| {
                            // Emit synthetic ToolCall for TUI rendering.
                            let tool_call_id = acp::ToolCallId::from(format!(
                                "read_text_file-{}",
                                request.path.display()
                            ));
                            let title = format!("Reading {}", request.path.display());
                            let tool_call = acp::ToolCall::new(tool_call_id, title)
                                .kind(acp::ToolKind::Execute)
                                .status(acp::ToolCallStatus::Pending);
                            let _ = event_tx.try_send(ConnectionEvent::SessionUpdate(
                                acp::SessionUpdate::ToolCall(tool_call),
                            ));

                            // Resolve relative paths against cwd.
                            let resolved_path = if request.path.is_relative() {
                                cwd.join(&request.path)
                            } else {
                                request.path
                            };

                            match std::fs::read_to_string(&resolved_path) {
                                Ok(content) => {
                                    responder.respond(acp::ReadTextFileResponse::new(content))?;
                                }
                                Err(e) => {
                                    responder.respond_with_error(sacp::util::internal_error(
                                        e.to_string(),
                                    ))?;
                                }
                            }
                            Ok(())
                        }
                    },
                    sacp::on_receive_request!(),
                )
                .connect_with(transport, |connection: ConnectionTo<Agent>| async move {
                    // Initialization handshake.
                    let response = connection
                        .send_request(
                            acp::InitializeRequest::new(acp::ProtocolVersion::LATEST)
                                .client_capabilities(
                                    acp::ClientCapabilities::new().fs(
                                        acp::FileSystemCapabilities::new()
                                            .read_text_file(true)
                                            .write_text_file(true),
                                    ),
                                )
                                .client_info(
                                    acp::Implementation::new("codex", env!("CARGO_PKG_VERSION"))
                                        .title("Codex CLI"),
                                ),
                        )
                        .block_task()
                        .await;

                    match response {
                        Ok(resp) => {
                            if resp.protocol_version < MINIMUM_SUPPORTED_VERSION {
                                let _ = init_tx.send(Err(anyhow::anyhow!(
                                    "ACP agent version {} is too old (minimum: {})",
                                    resp.protocol_version,
                                    MINIMUM_SUPPORTED_VERSION
                                )));
                                return Err(sacp::util::internal_error("Protocol version too old"));
                            }
                            debug!(
                                "ACP connection established (SACP v11), agent: {:?}",
                                resp.agent_info
                            );
                            let _ = init_tx.send(Ok((connection.clone(), resp.agent_capabilities)));

                            // Keep connection alive until the task is aborted.
                            futures::future::pending::<Result<(), sacp::Error>>().await
                        }
                        Err(e) => {
                            let _ = init_tx
                                .send(Err(anyhow::anyhow!("ACP initialization failed: {e}")));
                            Err(e)
                        }
                    }
                })
                .await;

            if let Err(e) = result {
                debug!("SACP connection task ended: {e}");
            }
        });

        // Wait for initialization.
        let (cx, capabilities) = init_rx
            .await
            .context("SACP connection task died during initialization")??;

        Ok(Self {
            cx,
            agent_capabilities: capabilities,
            event_rx,
            prompt_state,
            model_state: std::sync::Arc::new(std::sync::RwLock::new(AcpModelState::new())),
            connection_task,
            child,
            stderr_task,
        })
    }

    /// Create a new session with the agent.
    ///
    /// `mcp_servers` are forwarded to the agent so it can connect to CLI-configured
    /// MCP servers. Pass an empty vec for sessions that don't need MCP (e.g. hooks).
    pub async fn create_session(
        &self,
        cwd: &Path,
        mcp_servers: Vec<acp::McpServer>,
    ) -> Result<acp::SessionId> {
        let response = self
            .cx
            .send_request(acp::NewSessionRequest::new(cwd).mcp_servers(mcp_servers))
            .block_task()
            .await
            .context("Failed to create ACP session")?;

        #[cfg(feature = "unstable")]
        if let Some(ref models) = response.models
            && let Ok(mut state) = self.model_state.write()
        {
            *state = AcpModelState::from_session_model_state(models);
            debug!(
                "Model state updated: current={:?}, available={}",
                state.current_model_id,
                state.available_models.len()
            );
        }

        Ok(response.session_id)
    }

    /// Load (resume) an existing session.
    ///
    /// The agent replays previous session history. Updates flow through the
    /// ordered event inbox. The returned `SessionId` is the same as
    /// the input `session_id` (the LoadSessionResponse doesn't contain one).
    pub async fn load_session(&self, session_id: &str, cwd: &Path) -> Result<acp::SessionId> {
        let response = self
            .cx
            .send_request(acp::LoadSessionRequest::new(session_id.to_string(), cwd))
            .block_task()
            .await
            .context("Failed to load ACP session")?;

        #[cfg(feature = "unstable")]
        if let Some(ref models) = response.models
            && let Ok(mut state) = self.model_state.write()
        {
            *state = AcpModelState::from_session_model_state(models);
        }

        // The session ID from the request is reused since the response
        // doesn't contain one.
        Ok(acp::SessionId::from(session_id.to_string()))
    }

    /// Send a prompt to an existing session and receive streaming updates.
    ///
    /// Updates flow through the ordered event inbox.
    pub async fn prompt(
        &self,
        session_id: acp::SessionId,
        prompt: Vec<acp::ContentBlock>,
    ) -> Result<acp::StopReason> {
        let session_key = session_id.to_string();
        let mut attempt = 0_i64;

        loop {
            attempt += 1;
            let (update_seq_before, draining_cancel_tail) = {
                let mut prompt_state = self.prompt_state.lock().await;
                let state = prompt_state.entry(session_key.clone()).or_default();
                (state.update_seq, state.draining_cancel_tail)
            };

            debug!(
                target: "acp_event_flow",
                session_id = %session_id,
                content_blocks = prompt.len(),
                attempt,
                draining_cancel_tail,
                update_seq_before,
                "Transport sending ACP session/prompt request"
            );
            let response = self
                .cx
                .send_request(acp::PromptRequest::new(session_id.clone(), prompt.clone()))
                .block_task()
                .await
                .context("ACP prompt failed");

            match response {
                Ok(response) => {
                    let absorb_cancel_tail_end_turn = {
                        let mut prompt_state = self.prompt_state.lock().await;
                        let state = prompt_state.entry(session_key.clone()).or_default();
                        let saw_updates = state.update_seq > update_seq_before;
                        let absorb = state.draining_cancel_tail
                            && !saw_updates
                            && response.stop_reason == acp::StopReason::EndTurn;

                        if response.stop_reason == acp::StopReason::Cancelled {
                            state.draining_cancel_tail = true;
                        } else if !absorb {
                            state.draining_cancel_tail = false;
                        }

                        debug!(
                            target: "acp_event_flow",
                            session_id = %session_id,
                            attempt,
                            stop_reason = ?response.stop_reason,
                            saw_updates,
                            absorb_cancel_tail_end_turn = absorb,
                            draining_cancel_tail_after = state.draining_cancel_tail,
                            update_seq_after = state.update_seq,
                            "Transport received ACP session/prompt response"
                        );

                        absorb
                    };

                    if absorb_cancel_tail_end_turn {
                        continue;
                    }

                    return Ok(response.stop_reason);
                }
                Err(err) => {
                    warn!(
                        target: "acp_event_flow",
                        error = %err,
                        session_id = %session_id,
                        attempt,
                        "Transport session/prompt request failed"
                    );
                    return Err(err);
                }
            }
        }
    }

    /// Cancel an ongoing prompt.
    pub async fn cancel(&self, session_id: &acp::SessionId) -> Result<()> {
        self.cx
            .send_notification(acp::CancelNotification::new(session_id.clone()))
            .context("Failed to cancel ACP session")
    }

    /// Get the agent's capabilities.
    pub fn capabilities(&self) -> &acp::AgentCapabilities {
        &self.agent_capabilities
    }

    /// Take ownership of the ordered ACP event receiver.
    pub fn take_event_receiver(&mut self) -> mpsc::Receiver<ConnectionEvent> {
        std::mem::replace(&mut self.event_rx, mpsc::channel(1).1)
    }

    /// Get the current model state.
    pub fn model_state(&self) -> AcpModelState {
        #[expect(
            clippy::expect_used,
            reason = "RwLock poisoning indicates a bug elsewhere"
        )]
        self.model_state
            .read()
            .expect("Model state lock poisoned")
            .clone()
    }

    /// Explicitly tear down the ACP subprocess and background tasks.
    ///
    /// Unlike `Drop`, this async path can wait for process termination so the
    /// child is reaped promptly during agent switches and shutdown.
    pub async fn shutdown(&self) {
        self.connection_task.abort();
        self.stderr_task.abort();

        let mut child = self.child.lock().await;

        #[cfg(unix)]
        if let Err(e) = kill_child_process_group(&mut child) {
            debug!("Failed to kill process group during shutdown: {e}");
        }

        if let Err(e) = child.kill().await {
            debug!("Failed to kill ACP agent child process during shutdown: {e}");
        }
    }

    /// Switch to a different model for the given session.
    #[cfg(feature = "unstable")]
    pub async fn set_model(
        &self,
        session_id: &acp::SessionId,
        model_id: &acp::ModelId,
    ) -> Result<()> {
        let request = acp::SetSessionModelRequest::new(session_id.clone(), model_id.clone());
        let untyped = UntypedMessage::new("session/set_model", &request)
            .context("Failed to serialize SetSessionModelRequest")?;
        self.cx
            .send_request(untyped)
            .block_task()
            .await
            .context("Failed to set ACP model")?;

        if let Ok(mut state) = self.model_state.write() {
            state.current_model_id = Some(model_id.clone());
            debug!(
                "Model state updated after switch: current={:?}",
                state.current_model_id
            );
        }

        Ok(())
    }
}

impl Drop for SacpConnection {
    fn drop(&mut self) {
        self.connection_task.abort();
        self.stderr_task.abort();

        let child = std::sync::Arc::clone(&self.child);
        if let Ok(mut child) = child.try_lock() {
            #[cfg(unix)]
            if let Err(e) = kill_child_process_group(&mut child) {
                debug!("Failed to kill process group: {e}");
            }

            if let Err(e) = child.start_kill() {
                debug!("Failed to kill ACP agent child process: {e}");
            }
        }
    }
}

/// Kill the entire process group to ensure grandchildren are terminated.
#[cfg(unix)]
fn kill_child_process_group(child: &mut Child) -> std::io::Result<()> {
    use std::io::ErrorKind;

    if let Some(pid) = child.id() {
        let pid = pid as libc::pid_t;

        let pgid = unsafe { libc::getpgid(pid) };
        if pgid == -1 {
            let err = std::io::Error::last_os_error();
            if err.kind() != ErrorKind::NotFound {
                return Err(err);
            }
            return Ok(());
        }

        let result = unsafe { libc::killpg(pgid, libc::SIGKILL) };
        if result == -1 {
            let err = std::io::Error::last_os_error();
            if err.kind() != ErrorKind::NotFound {
                return Err(err);
            }
        }
    }

    Ok(())
}
