//! SACP v10-based ACP connection layer.
//!
//! This replaces the old `AcpConnection` which required a dedicated worker thread
//! due to the `!Send` futures in `agent-client-protocol` v0.9. SACP v10's
//! `ClientToAgent` is `Send + Sync`, allowing direct async usage from the main
//! tokio runtime without a dedicated thread or `LocalSet`.

use std::path::Path;
use std::process::Stdio;

use anyhow::Context;
use anyhow::Result;
use futures::AsyncBufReadExt;
use futures::io::BufReader;
use sacp::ByteStreams;
use sacp::ClientToAgent;
use sacp::JrConnectionCx;
use sacp::schema::AgentCapabilities;
use sacp::schema::CancelNotification;
use sacp::schema::ClientCapabilities;
use sacp::schema::ContentBlock;
use sacp::schema::FileSystemCapability;
use sacp::schema::Implementation;
use sacp::schema::InitializeRequest;
use sacp::schema::LoadSessionRequest;
use sacp::schema::NewSessionRequest;
use sacp::schema::PromptRequest;
use sacp::schema::ProtocolVersion;
use sacp::schema::ReadTextFileRequest;
use sacp::schema::ReadTextFileResponse;
use sacp::schema::RequestPermissionRequest;
use sacp::schema::SessionId;
use sacp::schema::SessionNotification;
use sacp::schema::SessionUpdate;
use sacp::schema::StopReason;
use sacp::schema::ToolCall;
use sacp::schema::ToolCallId;
use sacp::schema::ToolCallStatus;
use sacp::schema::ToolKind;
use sacp::schema::WriteTextFileRequest;
use sacp::schema::WriteTextFileResponse;
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
use super::ToolCallMetadata;
use crate::registry::AcpAgentConfig;
use crate::translator;

#[cfg(feature = "unstable")]
use sacp::UntypedMessage;
#[cfg(feature = "unstable")]
use sacp::schema::ModelId;
#[cfg(feature = "unstable")]
use sacp::schema::SetSessionModelRequest;

/// Minimum supported ACP protocol version.
const MINIMUM_SUPPORTED_VERSION: ProtocolVersion = ProtocolVersion::V1;

/// A thread-safe connection to an ACP agent subprocess using SACP v10.
///
/// Unlike the old `AcpConnection`, this does NOT require a dedicated worker thread.
/// SACP v10's `ClientToAgent` is `Send + Sync`, allowing all operations to run
/// directly on the main tokio runtime.
///
/// Internal architecture:
/// - A background tokio task runs the SACP connection via `run_until`.
/// - The `JrConnectionCx` is cloned out and used for all subsequent requests.
/// - Session notifications and approval requests are forwarded via channels.
/// - The session update channel is swapped for each prompt via an `Arc<Mutex<...>>`.
pub struct SacpConnection {
    /// Connection context for sending requests to the agent.
    cx: JrConnectionCx<ClientToAgent>,

    /// Agent capabilities from the initialization handshake.
    agent_capabilities: AgentCapabilities,

    /// Channel to receive approval requests from the agent.
    approval_rx: mpsc::Receiver<ApprovalRequest>,

    /// Channel to receive inter-turn notifications.
    persistent_rx: mpsc::Receiver<SessionUpdate>,

    /// Thread-safe model state, updated on session creation and model switch.
    model_state: std::sync::Arc<std::sync::RwLock<AcpModelState>>,

    /// Shared session update sender. The notification handler routes updates
    /// to whoever currently holds the active sender. During a prompt, this
    /// contains the caller's `update_tx`. Between turns, it is `None` and
    /// notifications fall through to the persistent channel.
    active_update_tx: std::sync::Arc<Mutex<Option<mpsc::Sender<SessionUpdate>>>>,

    /// Handle to the background task driving the SACP connection.
    connection_task: tokio::task::JoinHandle<()>,

    /// Handle to the child process for cleanup.
    child: std::sync::Arc<Mutex<Child>>,

    /// Handle to the stderr logging task.
    stderr_task: tokio::task::JoinHandle<()>,
}

impl SacpConnection {
    /// Spawn a new ACP agent subprocess and establish a SACP v10 connection.
    pub async fn spawn(config: &AcpAgentConfig, cwd: &Path) -> Result<Self> {
        debug!(
            "Spawning ACP agent (SACP v10): {} {:?} in {}",
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
        let (approval_tx, approval_rx) = mpsc::channel::<ApprovalRequest>(16);
        let (persistent_tx, persistent_rx) = mpsc::channel::<SessionUpdate>(64);
        let active_update_tx: std::sync::Arc<Mutex<Option<mpsc::Sender<SessionUpdate>>>> =
            std::sync::Arc::new(Mutex::new(None));

        // --- Build SACP connection ---
        let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());

        let notification_update_tx = std::sync::Arc::clone(&active_update_tx);
        let notification_persistent_tx = persistent_tx.clone();
        let write_update_tx = std::sync::Arc::clone(&active_update_tx);
        let write_persistent_tx = persistent_tx.clone();
        let read_update_tx = std::sync::Arc::clone(&active_update_tx);
        let read_persistent_tx = persistent_tx.clone();
        let approval_cwd = cwd.to_path_buf();
        let write_cwd = cwd.to_path_buf();
        let read_cwd = cwd.to_path_buf();

        // Oneshot to receive the JrConnectionCx and init result from inside run_until.
        let (init_tx, init_rx) =
            oneshot::channel::<Result<(JrConnectionCx<ClientToAgent>, AgentCapabilities)>>();

        let child = std::sync::Arc::new(Mutex::new(child));

        let connection_task = tokio::spawn(async move {
            let result = ClientToAgent::builder()
                .on_receive_notification(
                    {
                        let update_tx = notification_update_tx;
                        let persistent_tx = notification_persistent_tx;
                        async move |notification: SessionNotification, _cx| {
                            let update = notification.update;
                            let guard = update_tx.lock().await;
                            if let Some(tx) = guard.as_ref() {
                                let _ = tx.try_send(update);
                            } else {
                                let _ = persistent_tx.try_send(update);
                            }
                            Ok(())
                        }
                    },
                    sacp::on_receive_notification!(),
                )
                .on_receive_request(
                    {
                        let approval_tx = approval_tx;
                        let cwd = approval_cwd;
                        async move |request: RequestPermissionRequest,
                                    request_cx: sacp::JrRequestCx<
                            sacp::schema::RequestPermissionResponse,
                        >,
                                    cx: JrConnectionCx<ClientToAgent>| {
                            // Translate ACP permission request to Codex approval event.
                            let event = if let Some(patch_event) =
                                translator::permission_request_to_patch_approval_event(&request)
                            {
                                ApprovalEventType::Patch(patch_event)
                            } else {
                                let exec_event =
                                    translator::permission_request_to_approval_event(
                                        &request, &cwd,
                                    );
                                ApprovalEventType::Exec(exec_event)
                            };

                            // Extract tool call metadata for event translation.
                            // When the subsequent ToolCallUpdate(completed) arrives
                            // (often with empty title/kind from Gemini agents), this
                            // metadata allows the event translator to resolve a proper
                            // command name instead of falling back to "Tool".
                            let tool_call_metadata =
                                if request.tool_call.fields.title.is_some()
                                    || request.tool_call.fields.kind.is_some()
                                    || request.tool_call.fields.raw_input.is_some()
                                {
                                    Some(ToolCallMetadata {
                                        title: request.tool_call.fields.title.clone(),
                                        kind: request.tool_call.fields.kind,
                                        raw_input: request
                                            .tool_call
                                            .fields
                                            .raw_input
                                            .clone(),
                                    })
                                } else {
                                    None
                                };

                            let (response_tx, response_rx) = oneshot::channel();
                            let approval = ApprovalRequest {
                                event,
                                acp_request: request.clone(),
                                options: request.options.clone(),
                                response_tx,
                                tool_call_metadata,
                            };

                            if approval_tx.send(approval).await.is_err() {
                                request_cx.respond(
                                    sacp::schema::RequestPermissionResponse::new(
                                        sacp::schema::RequestPermissionOutcome::Cancelled,
                                    ),
                                )?;
                                return Ok(());
                            }

                            // Spawn to avoid blocking the dispatch loop.
                            cx.spawn(async move {
                                let outcome =
                                    match response_rx.await {
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
                                                .find(|opt| matches!(
                                                    opt.kind,
                                                    sacp::schema::PermissionOptionKind::RejectOnce
                                                    | sacp::schema::PermissionOptionKind::RejectAlways
                                                ))
                                                .map(|opt| opt.option_id.clone())
                                                .unwrap_or_else(|| sacp::schema::PermissionOptionId::from("deny".to_string()));
                                            sacp::schema::RequestPermissionOutcome::Selected(
                                                sacp::schema::SelectedPermissionOutcome::new(option_id),
                                            )
                                        }
                                    };
                                request_cx.respond(
                                    sacp::schema::RequestPermissionResponse::new(outcome),
                                )?;
                                Ok(())
                            })?;

                            Ok(())
                        }
                    },
                    sacp::on_receive_request!(),
                )
                .on_receive_request(
                    {
                        let update_tx = write_update_tx;
                        let persistent_tx = write_persistent_tx;
                        let cwd = write_cwd;
                        async move |request: WriteTextFileRequest,
                                    request_cx: sacp::JrRequestCx<WriteTextFileResponse>,
                                    _cx: JrConnectionCx<ClientToAgent>| {
                            // Emit synthetic ToolCall for TUI rendering.
                            let tool_call_id = ToolCallId::from(format!(
                                "write_text_file-{}",
                                request.path.display()
                            ));
                            let title =
                                format!("Writing {}", request.path.display());
                            let tool_call = ToolCall::new(tool_call_id, title)
                                .kind(ToolKind::Execute)
                                .status(ToolCallStatus::Pending);
                            {
                                let guard = update_tx.lock().await;
                                if let Some(tx) = guard.as_ref() {
                                    let _ =
                                        tx.try_send(SessionUpdate::ToolCall(tool_call));
                                } else {
                                    let _ = persistent_tx
                                        .try_send(SessionUpdate::ToolCall(tool_call));
                                }
                            }

                            let path = &request.path;
                            let resolved_path = if path.is_relative() {
                                cwd.join(path)
                            } else {
                                path.to_path_buf()
                            };

                            // Security: restrict writes to workspace or /tmp.
                            let allowed =
                                if let Ok(canonical) = resolved_path.canonicalize() {
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
                                            .map(|c| {
                                                canonical_parent.starts_with(&c)
                                            })
                                            .unwrap_or(false);
                                        let in_tmp =
                                            canonical_parent.starts_with("/tmp");
                                        in_cwd || in_tmp
                                    } else {
                                        resolved_path.starts_with(&cwd)
                                            || resolved_path.starts_with("/tmp")
                                    }
                                } else {
                                    false
                                };

                            if !allowed {
                                request_cx.respond_with_error(
                                    sacp::Error::invalid_params().data(format!(
                                        "Write restricted to working directory ({}) or /tmp. Path: {}",
                                        cwd.display(),
                                        resolved_path.display()
                                    )),
                                )?;
                                return Ok(());
                            }

                            // Create parent directories if needed.
                            if let Some(parent) = resolved_path.parent()
                                && !parent.exists()
                                    && let Err(e) = std::fs::create_dir_all(parent) {
                                        request_cx.respond_with_error(
                                            sacp::util::internal_error(
                                                e.to_string(),
                                            ),
                                        )?;
                                        return Ok(());
                                    }

                            match std::fs::write(&resolved_path, &request.content) {
                                Ok(()) => {
                                    request_cx
                                        .respond(WriteTextFileResponse::new())?;
                                }
                                Err(e) => {
                                    request_cx.respond_with_error(
                                        sacp::util::internal_error(e.to_string()),
                                    )?;
                                }
                            }
                            Ok(())
                        }
                    },
                    sacp::on_receive_request!(),
                )
                .on_receive_request(
                    {
                        let update_tx = read_update_tx;
                        let persistent_tx = read_persistent_tx;
                        let cwd = read_cwd;
                        async move |request: ReadTextFileRequest,
                                    request_cx: sacp::JrRequestCx<ReadTextFileResponse>,
                                    _cx: JrConnectionCx<ClientToAgent>| {
                            // Emit synthetic ToolCall for TUI rendering.
                            let tool_call_id = ToolCallId::from(format!(
                                "read_text_file-{}",
                                request.path.display()
                            ));
                            let title =
                                format!("Reading {}", request.path.display());
                            let tool_call = ToolCall::new(tool_call_id, title)
                                .kind(ToolKind::Execute)
                                .status(ToolCallStatus::Pending);
                            {
                                let guard = update_tx.lock().await;
                                if let Some(tx) = guard.as_ref() {
                                    let _ =
                                        tx.try_send(SessionUpdate::ToolCall(tool_call));
                                } else {
                                    let _ = persistent_tx
                                        .try_send(SessionUpdate::ToolCall(tool_call));
                                }
                            }

                            // Resolve relative paths against cwd.
                            let resolved_path = if request.path.is_relative() {
                                cwd.join(&request.path)
                            } else {
                                request.path.clone()
                            };

                            match std::fs::read_to_string(&resolved_path) {
                                Ok(content) => {
                                    request_cx
                                        .respond(ReadTextFileResponse::new(content))?;
                                }
                                Err(e) => {
                                    request_cx.respond_with_error(
                                        sacp::util::internal_error(e.to_string()),
                                    )?;
                                }
                            }
                            Ok(())
                        }
                    },
                    sacp::on_receive_request!(),
                )
                .run_until(transport, |cx: JrConnectionCx<ClientToAgent>| async move {
                    // Initialization handshake.
                    let response = cx
                        .send_request(
                            InitializeRequest::new(ProtocolVersion::LATEST)
                                .client_capabilities(
                                    ClientCapabilities::new().fs(
                                        FileSystemCapability::new()
                                            .read_text_file(true)
                                            .write_text_file(true),
                                    ),
                                )
                                .client_info(
                                    Implementation::new("codex", env!("CARGO_PKG_VERSION"))
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
                                return Err(sacp::util::internal_error(
                                    "Protocol version too old",
                                ));
                            }
                            debug!(
                                "ACP connection established (SACP v10), agent: {:?}",
                                resp.agent_info
                            );
                            let _ = init_tx.send(Ok((cx.clone(), resp.agent_capabilities)));

                            // Keep connection alive until the task is aborted.
                            futures::future::pending::<Result<(), sacp::Error>>().await
                        }
                        Err(e) => {
                            let _ = init_tx.send(Err(anyhow::anyhow!(
                                "ACP initialization failed: {e}"
                            )));
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
            approval_rx,
            persistent_rx,
            model_state: std::sync::Arc::new(std::sync::RwLock::new(AcpModelState::new())),
            active_update_tx,
            connection_task,
            child,
            stderr_task,
        })
    }

    /// Create a new session with the agent.
    pub async fn create_session(&self, cwd: &Path) -> Result<SessionId> {
        let response = self
            .cx
            .send_request(NewSessionRequest::new(cwd))
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
    /// The agent replays previous session history, streaming updates via
    /// the provided `update_tx` channel. The returned `SessionId` is the
    /// same as the input `session_id` (the LoadSessionResponse doesn't
    /// contain one).
    pub async fn load_session(
        &self,
        session_id: &str,
        cwd: &Path,
        update_tx: mpsc::Sender<SessionUpdate>,
    ) -> Result<SessionId> {
        // Install the update channel for replay events.
        {
            let mut guard = self.active_update_tx.lock().await;
            *guard = Some(update_tx);
        }

        let result = self
            .cx
            .send_request(LoadSessionRequest::new(session_id.to_string(), cwd))
            .block_task()
            .await
            .context("Failed to load ACP session");

        // Uninstall so replay events stop flowing to the caller's channel.
        {
            let mut guard = self.active_update_tx.lock().await;
            *guard = None;
        }

        let response = result?;

        #[cfg(feature = "unstable")]
        if let Some(ref models) = response.models
            && let Ok(mut state) = self.model_state.write()
        {
            *state = AcpModelState::from_session_model_state(models);
        }

        // The session ID from the request is reused since the response
        // doesn't contain one.
        Ok(SessionId::from(session_id.to_string()))
    }

    /// Send a prompt to an existing session and receive streaming updates.
    pub async fn prompt(
        &self,
        session_id: SessionId,
        prompt: Vec<ContentBlock>,
        update_tx: mpsc::Sender<SessionUpdate>,
    ) -> Result<StopReason> {
        // Install the update channel.
        {
            let mut guard = self.active_update_tx.lock().await;
            *guard = Some(update_tx);
        }

        let result = self
            .cx
            .send_request(PromptRequest::new(session_id, prompt))
            .block_task()
            .await
            .context("ACP prompt failed");

        // Uninstall so inter-turn notifications go to persistent.
        {
            let mut guard = self.active_update_tx.lock().await;
            *guard = None;
        }

        result.map(|r| r.stop_reason)
    }

    /// Cancel an ongoing prompt.
    pub async fn cancel(&self, session_id: &SessionId) -> Result<()> {
        self.cx
            .send_notification(CancelNotification::new(session_id.clone()))
            .context("Failed to cancel ACP session")
    }

    /// Get the agent's capabilities.
    pub fn capabilities(&self) -> &AgentCapabilities {
        &self.agent_capabilities
    }

    /// Take ownership of the approval request receiver.
    pub fn take_approval_receiver(&mut self) -> mpsc::Receiver<ApprovalRequest> {
        std::mem::replace(&mut self.approval_rx, mpsc::channel(1).1)
    }

    /// Take ownership of the persistent notification receiver.
    pub fn take_persistent_receiver(&mut self) -> mpsc::Receiver<SessionUpdate> {
        std::mem::replace(&mut self.persistent_rx, mpsc::channel(1).1)
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

    /// Switch to a different model for the given session.
    #[cfg(feature = "unstable")]
    pub async fn set_model(&self, session_id: &SessionId, model_id: &ModelId) -> Result<()> {
        // SetSessionModelRequest doesn't have JrRequest impl in sacp v10,
        // so we send it as an UntypedMessage with the correct method name.
        let request = SetSessionModelRequest::new(session_id.clone(), model_id.clone());
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
