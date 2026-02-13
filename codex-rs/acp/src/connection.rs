//! ACP Connection management
//!
//! Handles spawning and communicating with ACP agent subprocesses.
//!
//! The ACP protocol library uses `LocalBoxFuture` which requires `!Send` futures.
//! To integrate with codex-core's multi-threaded tokio runtime, we spawn a dedicated
//! single-threaded runtime for each ACP connection and communicate via channels.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::thread;
use std::time::Duration;

use agent_client_protocol as acp;
use anyhow::Context;
use anyhow::Result;
use codex_protocol::approvals::ApplyPatchApprovalRequestEvent;
use codex_protocol::approvals::ExecApprovalRequestEvent;
use codex_protocol::protocol::ReviewDecision;
use futures::AsyncBufReadExt;
use futures::io::BufReader;
use tokio::process::Child;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tokio_util::compat::TokioAsyncWriteCompatExt;
use tracing::debug;
use tracing::warn;

use crate::registry::AcpAgentConfig;
use crate::translator;

/// The type of approval event to send to the UI.
///
/// This enum allows us to use the more appropriate approval UI for different
/// operation types - exec approval for shell commands, patch approval for
/// file edits/writes/deletes.
#[derive(Debug)]
pub enum ApprovalEventType {
    /// Exec approval for shell commands and other operations
    Exec(ExecApprovalRequestEvent),
    /// Patch approval for file edit/write/delete operations
    Patch(ApplyPatchApprovalRequestEvent),
}

impl ApprovalEventType {
    /// Get the call_id from the event
    pub fn call_id(&self) -> &str {
        match self {
            ApprovalEventType::Exec(e) => &e.call_id,
            ApprovalEventType::Patch(e) => &e.call_id,
        }
    }
}

/// An approval request sent from the ACP layer to the UI layer.
///
/// When an ACP agent requests permission to perform an operation,
/// this struct is sent to the UI layer which should display the request
/// to the user and return their decision via the response channel.
#[derive(Debug)]
pub struct ApprovalRequest {
    /// The translated Codex approval event (either exec or patch)
    pub event: ApprovalEventType,
    /// The original ACP permission options for translating the response
    pub options: Vec<acp::PermissionOption>,
    /// Channel to send the user's decision back
    pub response_tx: oneshot::Sender<ReviewDecision>,
}

/// Minimum supported ACP protocol version
const MINIMUM_SUPPORTED_VERSION: acp::ProtocolVersion = acp::ProtocolVersion::V1;

/// Model state captured from the ACP session.
///
/// This is populated when a session is created (from `NewSessionResponse`)
/// and can be updated when the model is changed.
#[derive(Debug, Clone, Default)]
pub struct AcpModelState {
    /// The ID of the currently active model
    pub current_model_id: Option<acp::ModelId>,
    /// List of available models from the agent
    pub available_models: Vec<acp::ModelInfo>,
}

impl AcpModelState {
    /// Create a new empty model state
    pub fn new() -> Self {
        Self::default()
    }

    /// Update from an ACP SessionModelState
    #[cfg(feature = "unstable")]
    pub fn from_session_model_state(state: &acp::SessionModelState) -> Self {
        Self {
            current_model_id: Some(state.current_model_id.clone()),
            available_models: state.available_models.clone(),
        }
    }
}

/// Commands sent from the main thread to the ACP worker thread.
enum AcpCommand {
    CreateSession {
        cwd: PathBuf,
        response_tx: oneshot::Sender<Result<acp::SessionId>>,
    },
    Prompt {
        session_id: acp::SessionId,
        prompt: Vec<acp::ContentBlock>,
        update_tx: mpsc::Sender<acp::SessionUpdate>,
        response_tx: oneshot::Sender<Result<acp::StopReason>>,
    },
    LoadSession {
        session_id: String,
        cwd: PathBuf,
        update_tx: mpsc::Sender<acp::SessionUpdate>,
        response_tx: oneshot::Sender<Result<acp::SessionId>>,
    },
    Cancel {
        session_id: acp::SessionId,
        response_tx: oneshot::Sender<Result<()>>,
    },
    #[cfg(feature = "unstable")]
    SetModel {
        session_id: acp::SessionId,
        model_id: acp::ModelId,
        response_tx: oneshot::Sender<Result<()>>,
    },
}

/// Timeout for waiting for worker thread cleanup during Drop.
/// This should be long enough for the child process kill to complete,
/// but not so long that it blocks shutdown indefinitely.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

/// A thread-safe wrapper around an ACP agent subprocess.
///
/// This spawns a dedicated single-threaded tokio runtime on a background thread
/// to handle the ACP protocol (which requires `!Send` futures), and communicates
/// with the main runtime via channels.
///
/// When dropped, this struct ensures the worker thread completes its cleanup
/// (including killing the child process) before returning. This prevents
/// orphaned agent subprocesses when the TUI exits.
pub struct AcpConnection {
    command_tx: mpsc::Sender<AcpCommand>,
    agent_capabilities: acp::AgentCapabilities,
    /// Channel to receive approval requests from the agent.
    /// The UI layer should listen on this channel and respond via the oneshot sender.
    approval_rx: mpsc::Receiver<ApprovalRequest>,
    /// Thread-safe model state shared between the main thread and worker thread.
    /// Updated when sessions are created or models are switched.
    model_state: Arc<RwLock<AcpModelState>>,
    /// Worker thread handle. Stored as Option inside Mutex to allow taking in Drop.
    worker_thread: Mutex<Option<thread::JoinHandle<()>>>,
    /// Synchronous channel to receive notification when worker thread cleanup is complete.
    /// This allows Drop to wait for the child process kill to finish.
    /// Wrapped in Mutex<Option<>> for Sync (required by Arc<AcpConnection>).
    shutdown_complete_rx: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

impl AcpConnection {
    /// Spawn a new ACP agent subprocess and establish a connection.
    ///
    /// This spawns a dedicated worker thread with a single-threaded tokio runtime
    /// to handle the ACP protocol, which uses `!Send` futures.
    ///
    /// # Arguments
    /// * `config` - Agent configuration (command, args, provider info)
    /// * `cwd` - Working directory for the agent subprocess
    ///
    /// # Returns
    /// A connected `AcpConnection` ready for creating sessions.
    pub async fn spawn(config: &AcpAgentConfig, cwd: &Path) -> Result<Self> {
        let config = config.clone();
        let cwd = cwd.to_path_buf();

        // Use a oneshot channel to receive the initialization result
        let (init_tx, init_rx) = oneshot::channel();
        let (command_tx, command_rx) = mpsc::channel::<AcpCommand>(32);

        // Create approval channel - sender goes to worker, receiver stays here
        let (approval_tx, approval_rx) = mpsc::channel::<ApprovalRequest>(16);

        // Create shared model state - accessible from both main thread and worker
        let model_state = Arc::new(RwLock::new(AcpModelState::new()));
        let model_state_for_worker = Arc::clone(&model_state);

        // Create synchronous channel for shutdown completion notification.
        // This allows Drop to wait for worker thread cleanup to complete.
        let (shutdown_complete_tx, shutdown_complete_rx) = std::sync::mpsc::channel();

        // Spawn a dedicated thread with a single-threaded tokio runtime
        let worker_thread = thread::spawn(move || {
            #[expect(
                clippy::expect_used,
                reason = "Runtime creation in dedicated thread is infallible in practice"
            )]
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime for ACP worker");

            rt.block_on(async move {
                let local = tokio::task::LocalSet::new();
                local
                    .run_until(async move {
                        match spawn_connection_internal(&config, &cwd, approval_tx).await {
                            Ok((inner, capabilities)) => {
                                let _ = init_tx.send(Ok(capabilities));
                                run_command_loop(
                                    inner,
                                    command_rx,
                                    model_state_for_worker,
                                    shutdown_complete_tx,
                                )
                                .await;
                            }
                            Err(e) => {
                                let _ = init_tx.send(Err(e));
                                // Signal completion even on error so Drop doesn't hang
                                let _ = shutdown_complete_tx.send(());
                            }
                        }
                    })
                    .await;
            });
        });

        // Wait for initialization to complete
        let capabilities = init_rx
            .await
            .context("ACP worker thread died during initialization")??;

        Ok(Self {
            command_tx,
            agent_capabilities: capabilities,
            approval_rx,
            model_state,
            worker_thread: Mutex::new(Some(worker_thread)),
            shutdown_complete_rx: Mutex::new(Some(shutdown_complete_rx)),
        })
    }

    /// Create a new session with the agent.
    pub async fn create_session(&self, cwd: &Path) -> Result<acp::SessionId> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(AcpCommand::CreateSession {
                cwd: cwd.to_path_buf(),
                response_tx,
            })
            .await
            .context("ACP worker thread died")?;
        response_rx.await.context("ACP worker thread died")?
    }

    /// Load (resume) a previous session by its ACP session ID.
    ///
    /// The agent will stream `SessionUpdate` notifications as it replays
    /// conversation history, then return the session ID on success.
    pub async fn load_session(
        &self,
        session_id: &str,
        cwd: &Path,
        update_tx: mpsc::Sender<acp::SessionUpdate>,
    ) -> Result<acp::SessionId> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(AcpCommand::LoadSession {
                session_id: session_id.to_string(),
                cwd: cwd.to_path_buf(),
                update_tx,
                response_tx,
            })
            .await
            .context("ACP worker thread died")?;
        response_rx.await.context("ACP worker thread died")?
    }

    /// Send a prompt to an existing session and receive streaming updates.
    ///
    /// Returns the stop reason when the prompt completes.
    /// Session updates are streamed via the provided `update_tx` channel.
    pub async fn prompt(
        &self,
        session_id: acp::SessionId,
        prompt: Vec<acp::ContentBlock>,
        update_tx: mpsc::Sender<acp::SessionUpdate>,
    ) -> Result<acp::StopReason> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(AcpCommand::Prompt {
                session_id,
                prompt,
                update_tx,
                response_tx,
            })
            .await
            .context("ACP worker thread died")?;
        response_rx.await.context("ACP worker thread died")?
    }

    /// Cancel an ongoing prompt.
    pub async fn cancel(&self, session_id: &acp::SessionId) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(AcpCommand::Cancel {
                session_id: session_id.clone(),
                response_tx,
            })
            .await
            .context("ACP worker thread died")?;
        response_rx.await.context("ACP worker thread died")?
    }

    /// Get the agent's capabilities.
    pub fn capabilities(&self) -> &acp::AgentCapabilities {
        &self.agent_capabilities
    }

    /// Take ownership of the approval request receiver.
    ///
    /// This should be called once by the UI layer to receive approval requests.
    /// When an ACP agent requests permission, an `ApprovalRequest` will be sent
    /// through this channel. The UI should:
    /// 1. Display the request to the user (using `ApprovalRequest::event`)
    /// 2. Get the user's decision
    /// 3. Send the decision back via `ApprovalRequest::response_tx`
    ///
    /// # Panics
    /// This method can only be called once. Calling it again will panic.
    pub fn take_approval_receiver(&mut self) -> mpsc::Receiver<ApprovalRequest> {
        std::mem::replace(&mut self.approval_rx, mpsc::channel(1).1)
    }

    /// Get the current model state.
    ///
    /// Returns a clone of the current model state, which includes the current model ID
    /// and list of available models. This state is updated when a session is created
    /// or when the model is switched.
    ///
    /// # Panics
    /// This will panic if the RwLock is poisoned (i.e., a thread panicked while holding the lock).
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
    ///
    /// This sends a `session/set_model` request to the ACP agent. The model state
    /// will be updated automatically when the response is received.
    ///
    /// # Arguments
    /// * `session_id` - The session to switch models for
    /// * `model_id` - The ID of the model to switch to (must be in `available_models`)
    ///
    /// # Errors
    /// Returns an error if:
    /// - The model ID is not in the list of available models
    /// - The ACP agent doesn't support model switching
    /// - The worker thread has died
    #[cfg(feature = "unstable")]
    pub async fn set_model(
        &self,
        session_id: &acp::SessionId,
        model_id: &acp::ModelId,
    ) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(AcpCommand::SetModel {
                session_id: session_id.clone(),
                model_id: model_id.clone(),
                response_tx,
            })
            .await
            .context("ACP worker thread died")?;
        response_rx.await.context("ACP worker thread died")?
    }

    // TODO: [Future] History Export for Handoff
    // Add a method to export session history in Codex format for handoff to HTTP mode:
    //
    // ```rust
    // pub async fn export_history(&self, session_id: &SessionId) -> Result<Vec<ResponseItem>> {
    //     // 1. Retrieve accumulated history from ACP agent (if supported)
    //     // 2. Convert ACP format to Codex ResponseItem format
    //     // 3. Return for use in HTTP mode continuation
    // }
    // ```
    //
    // This would enable:
    // - Switching from ACP mode to HTTP mode mid-session
    // - Continuing a conversation started with one backend using another
    // - Debugging by replaying history through a different backend
}

impl Drop for AcpConnection {
    fn drop(&mut self) {
        // Drop command_tx first to signal the worker thread to exit.
        // This is implicit (field ordering doesn't matter for drop order in Rust),
        // but we make it explicit by taking ownership to ensure it's dropped early.
        drop(std::mem::replace(&mut self.command_tx, mpsc::channel(1).0));

        // Take the shutdown completion receiver from the mutex.
        // We use lock().ok() to handle poisoned mutex gracefully.
        let shutdown_rx = self
            .shutdown_complete_rx
            .lock()
            .ok()
            .and_then(|mut guard| guard.take());

        // Wait for the worker thread to signal that cleanup is complete.
        // This ensures the child process is killed before we return.
        // Use a timeout to avoid hanging indefinitely if something goes wrong.
        if let Some(rx) = shutdown_rx {
            match rx.recv_timeout(SHUTDOWN_TIMEOUT) {
                Ok(()) => {
                    debug!("ACP worker thread signaled cleanup complete");
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    warn!(
                        "Timeout waiting for ACP worker thread cleanup ({}s)",
                        SHUTDOWN_TIMEOUT.as_secs()
                    );
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    // Worker thread already exited (channel was dropped)
                    debug!("ACP worker thread already exited (channel disconnected)");
                }
            }
        }

        // Take the worker thread handle from the mutex.
        let worker_handle = self
            .worker_thread
            .lock()
            .ok()
            .and_then(|mut guard| guard.take());

        // Join the worker thread to ensure it has fully exited.
        // This prevents any lingering operations after Drop returns.
        if let Some(handle) = worker_handle {
            // Use a short timeout for the join - if the thread hasn't exited
            // after cleanup completion was signaled, something is wrong.
            // Note: std::thread::JoinHandle doesn't have join_timeout, so we
            // rely on the recv_timeout above and just join here.
            if let Err(e) = handle.join() {
                warn!("ACP worker thread panicked: {:?}", e);
            } else {
                debug!("ACP worker thread joined successfully");
            }
        }
    }
}

/// Internal connection state that lives on the worker thread.
struct AcpConnectionInner {
    connection: acp::ClientSideConnection,
    #[allow(dead_code)]
    client_delegate: Rc<ClientDelegate>,
    child: Child,
    /// IO task that handles reading from the agent's stdout.
    /// Aborted during cleanup to prevent hanging on orphaned pipes.
    io_task: tokio::task::JoinHandle<acp::Result<()>>,
    /// Stderr task that logs the agent's stderr output.
    /// Aborted during cleanup to prevent hanging on orphaned pipes.
    stderr_task: tokio::task::JoinHandle<()>,
}

/// Spawns the connection on the current LocalSet.
async fn spawn_connection_internal(
    config: &AcpAgentConfig,
    cwd: &Path,
    approval_tx: mpsc::Sender<ApprovalRequest>,
) -> Result<(AcpConnectionInner, acp::AgentCapabilities)> {
    debug!(
        "Spawning ACP agent: {} {:?} in {}",
        config.command,
        config.args,
        cwd.display()
    );

    let mut cmd = Command::new(&config.command);
    cmd.args(&config.args)
        .envs(&config.env)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    // Configure process group isolation and parent death signal for robust cleanup.
    // This provides kernel-level guarantees that the agent subprocess is terminated
    // even if the parent process crashes (not just clean exit).
    #[cfg(unix)]
    unsafe {
        #[cfg(target_os = "linux")]
        let parent_pid = libc::getpid();

        cmd.pre_exec(move || {
            // Create new process group for isolation.
            // This allows killing the entire process tree (including grandchildren)
            // by sending signals to the process group.
            if libc::setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }

            // Linux: Set PR_SET_PDEATHSIG to deliver SIGTERM when parent dies.
            // This is a kernel-level guarantee - if the parent process is killed
            // (even with SIGKILL), the kernel will send SIGTERM to this child.
            #[cfg(target_os = "linux")]
            {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                    return Err(std::io::Error::last_os_error());
                }

                // Race condition check: if parent already died during setup,
                // terminate immediately.
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

    // Log stderr in background (on the local set)
    let stderr_task = tokio::task::spawn_local(async move {
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

    // Create client delegate for handling agent requests
    let client_delegate = Rc::new(ClientDelegate::new(cwd.to_path_buf(), approval_tx));

    // Establish JSON-RPC connection
    let (connection, io_task) = acp::ClientSideConnection::new(
        Rc::clone(&client_delegate),
        stdin.compat_write(),
        stdout.compat(),
        |fut| {
            tokio::task::spawn_local(fut);
        },
    );

    let io_task = tokio::task::spawn_local(io_task);

    // Perform initialization handshake using the Agent trait
    use acp::Agent;
    let response = connection
        .initialize(
            acp::InitializeRequest::new(acp::ProtocolVersion::LATEST)
                .client_capabilities(
                    acp::ClientCapabilities::new().fs(acp::FileSystemCapability::new()
                        .read_text_file(true)
                        .write_text_file(true)),
                )
                .client_info(
                    acp::Implementation::new("codex", env!("CARGO_PKG_VERSION")).title("Codex CLI"),
                ),
        )
        .await
        .context("ACP initialization failed")?;

    if response.protocol_version < MINIMUM_SUPPORTED_VERSION {
        anyhow::bail!(
            "ACP agent version {} is too old (minimum: {})",
            response.protocol_version,
            MINIMUM_SUPPORTED_VERSION
        );
    }

    debug!(
        "ACP connection established, agent: {:?}",
        response.agent_info
    );

    let inner = AcpConnectionInner {
        connection,
        client_delegate,
        child,
        io_task,
        stderr_task,
    };

    Ok((inner, response.agent_capabilities))
}

/// Main command loop running on the worker thread.
///
/// This loop processes commands from the main thread until the command channel
/// is closed (when AcpConnection is dropped). After the loop exits, it kills
/// the child process and signals completion via `shutdown_complete_tx`.
async fn run_command_loop(
    mut inner: AcpConnectionInner,
    mut command_rx: mpsc::Receiver<AcpCommand>,
    model_state: Arc<RwLock<AcpModelState>>,
    shutdown_complete_tx: std::sync::mpsc::Sender<()>,
) {
    use acp::Agent;

    while let Some(cmd) = command_rx.recv().await {
        match cmd {
            AcpCommand::CreateSession { cwd, response_tx } => {
                // TODO: [Future] Resume/Fork Integration
                // When creating a session, check if there's an existing session to resume.
                // This would require:
                // 1. Accepting an optional session_id parameter to resume
                // 2. Loading persisted history from Codex rollout format
                // 3. Sending history to the agent via the session initialization
                // See: codex-core/src/rollout.rs for the persistence format

                let result = inner
                    .connection
                    .new_session(acp::NewSessionRequest::new(cwd))
                    .await;

                // Capture model state from the response if available
                #[cfg(feature = "unstable")]
                if let Ok(ref response) = result
                    && let Some(ref models) = response.models
                    && let Ok(mut state) = model_state.write()
                {
                    *state = AcpModelState::from_session_model_state(models);
                    debug!(
                        "Model state updated: current={:?}, available={}",
                        state.current_model_id,
                        state.available_models.len()
                    );
                }

                let result = result
                    .map(|r| r.session_id)
                    .context("Failed to create ACP session");
                let _ = response_tx.send(result);
            }
            AcpCommand::LoadSession {
                session_id,
                cwd,
                update_tx,
                response_tx,
            } => {
                // Register the update channel so session notifications are forwarded
                // during the load_session call (history replay).
                let acp_session_id: acp::SessionId = session_id.clone().into();
                inner
                    .client_delegate
                    .register_session(acp_session_id.clone(), update_tx);

                let result = inner
                    .connection
                    .load_session(acp::LoadSessionRequest::new(session_id, cwd))
                    .await;

                // Capture model state from the response if available
                #[cfg(feature = "unstable")]
                if let Ok(ref response) = result
                    && let Some(ref models) = response.models
                    && let Ok(mut state) = model_state.write()
                {
                    *state = AcpModelState::from_session_model_state(models);
                }

                // Unregister the session so the update channel is closed,
                // allowing the caller's forwarding task to complete.
                inner.client_delegate.unregister_session(&acp_session_id);

                // LoadSessionResponse doesn't contain a session_id; the
                // session ID from the request is reused.
                let result = result
                    .map(|_| acp_session_id)
                    .context("Failed to load ACP session");
                let _ = response_tx.send(result);
            }
            AcpCommand::Prompt {
                session_id,
                prompt,
                update_tx,
                response_tx,
            } => {
                inner
                    .client_delegate
                    .register_session(session_id.clone(), update_tx);

                // Use tokio::select! to allow Cancel commands to be processed while prompting
                let prompt_future = inner
                    .connection
                    .prompt(acp::PromptRequest::new(session_id.clone(), prompt));
                tokio::pin!(prompt_future);

                let result = loop {
                    tokio::select! {
                        prompt_result = &mut prompt_future => {
                            // Prompt completed normally
                            break prompt_result
                                .map(|r| r.stop_reason)
                                .context("ACP prompt failed");
                        }
                        cmd = command_rx.recv() => {
                            // Received another command while prompting
                            match cmd {
                                Some(AcpCommand::Cancel { session_id: cancel_session_id, response_tx: cancel_response_tx }) => {
                                    // Process the cancel command immediately
                                    let cancel_result = inner
                                        .connection
                                        .cancel(acp::CancelNotification::new(cancel_session_id))
                                        .await
                                        .context("Failed to cancel ACP session");
                                    let _ = cancel_response_tx.send(cancel_result);
                                    // Continue waiting for the prompt to complete (it should stop soon)
                                }
                                Some(other_cmd) => {
                                    // For other commands, we can't process them while prompting
                                    // This is a limitation - CreateSession during prompt will be dropped
                                    tracing::warn!("Dropping command received during prompt: {:?}", std::mem::discriminant(&other_cmd));
                                }
                                None => {
                                    // Channel closed, abort
                                    break Err(anyhow::anyhow!("Command channel closed during prompt"));
                                }
                            }
                        }
                    }
                };

                // TODO: [Future] Codex-format History Persistence
                // After a successful prompt, persist the conversation history in Codex's rollout
                // format. This would enable:
                // 1. Session resume after restart
                // 2. History browsing in the TUI
                // 3. Conversation forking
                // Implementation would involve:
                // - Collecting all SessionUpdates received during the prompt
                // - Converting them to Codex ResponseItem format using translator functions
                // - Writing to rollout storage (see codex-core/src/rollout.rs)

                inner.client_delegate.unregister_session(&session_id);
                let _ = response_tx.send(result);
            }
            AcpCommand::Cancel {
                session_id,
                response_tx,
            } => {
                let result = inner
                    .connection
                    .cancel(acp::CancelNotification::new(session_id))
                    .await
                    .context("Failed to cancel ACP session");
                let _ = response_tx.send(result);
            }
            #[cfg(feature = "unstable")]
            AcpCommand::SetModel {
                session_id,
                model_id,
                response_tx,
            } => {
                let result = inner
                    .connection
                    .set_session_model(acp::SetSessionModelRequest::new(
                        session_id,
                        model_id.clone(),
                    ))
                    .await;

                // Update the current model ID on success
                // The SetSessionModelResponse doesn't include model state,
                // so we manually update the current model ID.
                if result.is_ok()
                    && let Ok(mut state) = model_state.write()
                {
                    state.current_model_id = Some(model_id);
                    debug!(
                        "Model state updated after switch: current={:?}",
                        state.current_model_id
                    );
                }

                let result = result.map(|_| ()).context("Failed to set ACP model");
                let _ = response_tx.send(result);
            }
        }
    }

    // Cleanup: terminate the child process when command channel is closed
    // This happens when the AcpConnection is dropped (e.g., during session switch or exit)
    debug!("ACP command loop exiting, aborting IO tasks and terminating child process");

    // First, abort IO tasks to prevent hanging on orphaned file descriptors.
    // If the agent spawned grandchildren that kept stdout/stderr open, the IO tasks
    // could block indefinitely waiting for those pipes to close. Aborting them
    // ensures we don't hang during cleanup.
    inner.io_task.abort();
    inner.stderr_task.abort();

    // Give tasks a brief moment to abort cleanly before killing the process.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Second, kill the entire process group to handle grandchildren.
    // This is critical if the agent spawned its own subprocesses.
    #[cfg(unix)]
    if let Err(e) = kill_child_process_group(&mut inner.child) {
        debug!("Failed to kill process group: {}", e);
    }

    // Then kill the direct child (this is a no-op if process group kill succeeded).
    if let Err(e) = inner.child.start_kill() {
        debug!("Failed to kill ACP agent child process: {}", e);
    }

    // Wait for actual termination with a short timeout.
    // If grandchildren kept pipes open, this prevents hanging indefinitely.
    match tokio::time::timeout(Duration::from_millis(500), inner.child.wait()).await {
        Ok(Ok(status)) => {
            debug!("ACP agent exited with status: {:?}", status);
        }
        Ok(Err(e)) => {
            debug!("Error waiting for ACP agent exit: {}", e);
        }
        Err(_) => {
            warn!("Timeout waiting for ACP agent to exit after kill");
        }
    }

    // Signal that cleanup is complete so Drop can return
    // This ensures the main thread waits for the child process to be killed
    let _ = shutdown_complete_tx.send(());
}

/// Kill the entire process group to ensure grandchildren are terminated.
///
/// This is critical for agents that spawn their own subprocesses. When we kill
/// only the direct child, grandchildren can remain running and become orphaned.
/// By killing the entire process group, we ensure all descendants are terminated.
///
/// This function gracefully handles "process not found" errors (ESRCH), which
/// occur if the process has already exited.
#[cfg(unix)]
fn kill_child_process_group(child: &mut Child) -> std::io::Result<()> {
    use std::io::ErrorKind;

    if let Some(pid) = child.id() {
        let pid = pid as libc::pid_t;

        // Get the process group ID for this process.
        // Because we used setpgid(0, 0) during spawn, the child is its own process group leader.
        let pgid = unsafe { libc::getpgid(pid) };
        if pgid == -1 {
            let err = std::io::Error::last_os_error();
            // ESRCH means process not found - it already exited, which is fine
            if err.kind() != ErrorKind::NotFound {
                return Err(err);
            }
            return Ok(());
        }

        // Send SIGKILL to the entire process group.
        // The negative PGID syntax (-pgid) sends the signal to all processes in the group.
        let result = unsafe { libc::killpg(pgid, libc::SIGKILL) };
        if result == -1 {
            let err = std::io::Error::last_os_error();
            // ESRCH means process group doesn't exist - already exited, which is fine
            if err.kind() != ErrorKind::NotFound {
                return Err(err);
            }
        }
    }

    Ok(())
}

/// Client delegate that handles requests from the ACP agent.
///
/// This implements the `acp::Client` trait to handle:
/// - Session update notifications
/// - Permission requests
/// - File system operations
/// - Terminal operations (stubbed)
pub struct ClientDelegate {
    sessions: RefCell<HashMap<acp::SessionId, mpsc::Sender<acp::SessionUpdate>>>,
    /// Working directory for approval events
    cwd: PathBuf,
    /// Channel to send approval requests to the UI layer
    approval_tx: mpsc::Sender<ApprovalRequest>,
}

impl ClientDelegate {
    fn new(cwd: PathBuf, approval_tx: mpsc::Sender<ApprovalRequest>) -> Self {
        Self {
            sessions: RefCell::new(HashMap::new()),
            cwd,
            approval_tx,
        }
    }

    fn register_session(&self, session_id: acp::SessionId, tx: mpsc::Sender<acp::SessionUpdate>) {
        self.sessions.borrow_mut().insert(session_id, tx);
    }

    fn unregister_session(&self, session_id: &acp::SessionId) {
        self.sessions.borrow_mut().remove(session_id);
    }
}

#[async_trait::async_trait(?Send)]
impl acp::Client for ClientDelegate {
    async fn request_permission(
        &self,
        arguments: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        // Translate ACP permission request to Codex approval event.
        // Use patch approval for Edit/Write/Delete operations for better TUI rendering.
        let event = if let Some(patch_event) =
            translator::permission_request_to_patch_approval_event(&arguments)
        {
            ApprovalEventType::Patch(patch_event)
        } else {
            let exec_event =
                translator::permission_request_to_approval_event(&arguments, &self.cwd);
            ApprovalEventType::Exec(exec_event)
        };

        // Create a response channel for the UI to send the decision
        let (response_tx, response_rx) = oneshot::channel();

        // Send the approval request to the UI layer
        let approval_request = ApprovalRequest {
            event,
            options: arguments.options.clone(),
            response_tx,
        };

        if self.approval_tx.send(approval_request).await.is_err() {
            // If the receiver is dropped (UI not listening), fall back to auto-approve
            warn!("Approval channel closed, auto-approving permission request");
            let option_id = arguments
                .options
                .first()
                .map(|opt| opt.option_id.clone())
                .unwrap_or_else(|| acp::PermissionOptionId::from("allow".to_string()));

            return Ok(acp::RequestPermissionResponse::new(
                acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                    option_id,
                )),
            ));
        }

        // Wait for the UI's decision
        match response_rx.await {
            Ok(decision) => {
                // Translate the Codex ReviewDecision back to ACP outcome
                let outcome =
                    translator::review_decision_to_permission_outcome(decision, &arguments.options);
                Ok(acp::RequestPermissionResponse::new(outcome))
            }
            Err(_) => {
                // Response channel was dropped (UI didn't respond), fall back to deny
                warn!("Approval response channel dropped, denying permission request");
                let option_id = arguments
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
                    .unwrap_or_else(|| acp::PermissionOptionId::from("deny".to_string()));

                Ok(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                        option_id,
                    )),
                ))
            }
        }
    }

    async fn write_text_file(
        &self,
        arguments: acp::WriteTextFileRequest,
    ) -> acp::Result<acp::WriteTextFileResponse> {
        // Emit synthetic ToolCall event for TUI rendering (Gemini compatibility)
        // Gemini agents use client capability methods instead of session/update notifications,
        // so we synthesize the events here to enable proper TUI display.
        let tool_call_id =
            acp::ToolCallId::from(format!("write_text_file-{}", arguments.path.display()));
        let title = format!("Writing {}", arguments.path.display());

        let tool_call = acp::ToolCall::new(tool_call_id, title)
            .kind(acp::ToolKind::Execute)
            .status(acp::ToolCallStatus::Pending);

        // Send the ToolCall update to the session if registered
        let sessions = self.sessions.borrow();
        if let Some(tx) = sessions.get(&arguments.session_id) {
            let _ = tx.try_send(acp::SessionUpdate::ToolCall(tool_call));
        }
        drop(sessions); // Release borrow before performing I/O

        let path = &arguments.path;

        // Resolve relative paths against the working directory
        let resolved_path = if path.is_relative() {
            self.cwd.join(path)
        } else {
            path.to_path_buf()
        };

        // TEMPORARY PATH RESTRICTION:
        // This application-level path check provides basic safety until the ACP agent
        // subprocess is launched with OS-level sandboxing (Seatbelt on macOS, Landlock
        // on Linux, restricted tokens on Windows) as implemented in codex-core's
        // sandboxing module. Once subprocess sandboxing is in place, these checks
        // should be removed as the OS will enforce write restrictions more robustly.
        //
        // For now, restrict writes to:
        // 1. Within the working directory (typical workspace operations)
        // 2. Within /tmp (temporary files, common for agent workflows)
        let allowed = if let Ok(canonical) = resolved_path.canonicalize() {
            let in_cwd = self
                .cwd
                .canonicalize()
                .map(|cwd| canonical.starts_with(&cwd))
                .unwrap_or(false);
            let in_tmp = canonical.starts_with("/tmp");
            in_cwd || in_tmp
        } else {
            // Path doesn't exist yet - check if parent is within allowed directories
            // This handles the case of creating new files
            if let Some(parent) = resolved_path.parent() {
                if let Ok(canonical_parent) = parent.canonicalize() {
                    let in_cwd = self
                        .cwd
                        .canonicalize()
                        .map(|cwd| canonical_parent.starts_with(&cwd))
                        .unwrap_or(false);
                    let in_tmp = canonical_parent.starts_with("/tmp");
                    in_cwd || in_tmp
                } else {
                    // Parent also doesn't exist - only allow if resolved path starts with cwd or /tmp
                    resolved_path.starts_with(&self.cwd) || resolved_path.starts_with("/tmp")
                }
            } else {
                false
            }
        };

        if !allowed {
            return Err(acp::Error::invalid_params().data(format!(
                "Write restricted to working directory ({}) or /tmp. Path: {}",
                self.cwd.display(),
                resolved_path.display()
            )));
        }
        // END TEMPORARY PATH RESTRICTION

        // Create parent directories if they don't exist
        if let Some(parent) = resolved_path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).map_err(acp::Error::into_internal_error)?;
        }

        std::fs::write(&resolved_path, &arguments.content)
            .map_err(acp::Error::into_internal_error)?;
        Ok(acp::WriteTextFileResponse::new())
    }

    async fn read_text_file(
        &self,
        arguments: acp::ReadTextFileRequest,
    ) -> acp::Result<acp::ReadTextFileResponse> {
        // Emit synthetic ToolCall event for TUI rendering (Gemini compatibility)
        // Gemini agents use client capability methods instead of session/update notifications,
        // so we synthesize the events here to enable proper TUI display.
        let tool_call_id =
            acp::ToolCallId::from(format!("read_text_file-{}", arguments.path.display()));
        let title = format!("Reading {}", arguments.path.display());

        let tool_call = acp::ToolCall::new(tool_call_id, title)
            .kind(acp::ToolKind::Execute)
            .status(acp::ToolCallStatus::Pending);

        // Send the ToolCall update to the session if registered
        let sessions = self.sessions.borrow();
        if let Some(tx) = sessions.get(&arguments.session_id) {
            let _ = tx.try_send(acp::SessionUpdate::ToolCall(tool_call));
        }
        drop(sessions); // Release borrow before performing I/O

        // Read file content
        let content =
            std::fs::read_to_string(&arguments.path).map_err(acp::Error::into_internal_error)?;
        Ok(acp::ReadTextFileResponse::new(content))
    }

    async fn session_notification(
        &self,
        notification: acp::SessionNotification,
    ) -> acp::Result<()> {
        let sessions = self.sessions.borrow();
        if let Some(tx) = sessions.get(&notification.session_id) {
            // Non-blocking send - if channel is full or closed, we log and drop the update
            if let Err(e) = tx.try_send(notification.update) {
                debug!(
                    target: "acp_message_draining",
                    session_id = %notification.session_id,
                    error = %e,
                    "Session notification dropped (channel full or closed)"
                );
            }
        } else {
            // This else-branch is diagnostic for unregistered notifications.
            debug!(
                target: "acp_message_draining",
                session_id = %notification.session_id,
                "Notification for unregistered session (late arrival)"
            );
        }
        Ok(())
    }

    async fn create_terminal(
        &self,
        _args: acp::CreateTerminalRequest,
    ) -> acp::Result<acp::CreateTerminalResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn kill_terminal_command(
        &self,
        _args: acp::KillTerminalCommandRequest,
    ) -> acp::Result<acp::KillTerminalCommandResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_method(&self, _args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> acp::Result<()> {
        Ok(())
    }

    async fn release_terminal(
        &self,
        _args: acp::ReleaseTerminalRequest,
    ) -> acp::Result<acp::ReleaseTerminalResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn terminal_output(
        &self,
        _args: acp::TerminalOutputRequest,
    ) -> acp::Result<acp::TerminalOutputResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn wait_for_terminal_exit(
        &self,
        _args: acp::WaitForTerminalExitRequest,
    ) -> acp::Result<acp::WaitForTerminalExitResponse> {
        Err(acp::Error::method_not_found())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    /// Test that we can spawn an ACP connection and receive responses from the mock agent.
    /// This is an integration test using the real mock-acp-agent binary.
    #[tokio::test]
    #[serial]
    async fn test_spawn_connection_and_receive_response() {
        // Get the mock agent config
        let config = crate::registry::get_agent_config("mock-model")
            .expect("mock-model should be registered");

        // Check if mock agent binary exists
        if !std::path::Path::new(&config.command).exists() {
            // Skip test if binary not built
            eprintln!(
                "Skipping test: mock_acp_agent not found at {}",
                config.command
            );
            return;
        }

        let temp_dir = tempdir().expect("Failed to create temp dir");

        // Spawn connection
        let conn = AcpConnection::spawn(&config, temp_dir.path())
            .await
            .expect("Failed to spawn ACP connection");

        // Create session
        let session_id = conn
            .create_session(temp_dir.path())
            .await
            .expect("Failed to create session");

        // Send prompt and collect updates
        let (tx, mut rx) = mpsc::channel(32);
        let prompt = vec![acp::ContentBlock::Text(acp::TextContent::new("Hello"))];

        let stop_reason = conn
            .prompt(session_id, prompt, tx)
            .await
            .expect("Prompt failed");

        // Should have received responses
        let mut messages = Vec::new();
        while let Ok(update) = rx.try_recv() {
            if let acp::SessionUpdate::AgentMessageChunk(chunk) = update
                && let acp::ContentBlock::Text(text) = chunk.content
            {
                messages.push(text.text);
            }
        }

        // Mock agent sends "Test message 1" and "Test message 2"
        assert!(
            !messages.is_empty(),
            "Should have received at least one message"
        );
        assert!(
            messages.iter().any(|m| m.contains("Test message")),
            "Should contain test message, got: {messages:?}"
        );
        assert_eq!(stop_reason, acp::StopReason::EndTurn);
    }

    /// Test that read_text_file emits a ToolCall SessionUpdate event.
    /// This enables TUI rendering of file read operations for agents like Gemini
    /// that use client capability methods instead of session/update notifications.
    #[tokio::test]
    async fn test_read_text_file_emits_tool_call_event() {
        use acp::Client;

        let temp_dir = tempdir().expect("Failed to create temp dir");
        let test_file = temp_dir.path().join("test.txt");
        std::fs::write(&test_file, "test content").expect("Failed to write test file");

        // Create ClientDelegate with a session registered
        let (approval_tx, _approval_rx) = mpsc::channel(16);
        let delegate = ClientDelegate::new(temp_dir.path().to_path_buf(), approval_tx);

        // Register a session and capture updates
        let session_id = acp::SessionId::from("test-session-123".to_string());
        let (update_tx, mut update_rx) = mpsc::channel(32);
        delegate.register_session(session_id.clone(), update_tx);

        // Call read_text_file
        let request = acp::ReadTextFileRequest::new(session_id.clone(), test_file.clone());
        let response = delegate
            .read_text_file(request)
            .await
            .expect("read_text_file should succeed");

        // Verify the file was read
        assert_eq!(response.content, "test content");

        // Verify that a ToolCall SessionUpdate was emitted
        let update = update_rx
            .try_recv()
            .expect("Should have received a SessionUpdate");

        match update {
            acp::SessionUpdate::ToolCall(tool_call) => {
                assert_eq!(tool_call.status, acp::ToolCallStatus::Pending);
                assert!(
                    tool_call.title.contains("read_text_file")
                        || tool_call.title.contains("Reading")
                        || tool_call.title.contains("test.txt"),
                    "Title should indicate file read operation, got: {}",
                    tool_call.title
                );
                assert_eq!(tool_call.kind, acp::ToolKind::Execute);
            }
            other => panic!("Expected ToolCall update, got: {other:?}"),
        }

        delegate.unregister_session(&session_id);
    }

    /// Test that write_text_file emits a ToolCall SessionUpdate event.
    /// This enables TUI rendering of file write operations for agents like Gemini
    /// that use client capability methods instead of session/update notifications.
    #[tokio::test]
    async fn test_write_text_file_emits_tool_call_event() {
        use acp::Client;

        let temp_dir = tempdir().expect("Failed to create temp dir");
        let test_file = temp_dir.path().join("output.txt");

        // Create ClientDelegate with a session registered
        let (approval_tx, _approval_rx) = mpsc::channel(16);
        let delegate = ClientDelegate::new(temp_dir.path().to_path_buf(), approval_tx);

        // Register a session and capture updates
        let session_id = acp::SessionId::from("test-session-456".to_string());
        let (update_tx, mut update_rx) = mpsc::channel(32);
        delegate.register_session(session_id.clone(), update_tx);

        // Call write_text_file
        let content = "Hello, world!";
        let request = acp::WriteTextFileRequest::new(
            session_id.clone(),
            test_file.clone(),
            content.to_string(),
        );
        let response = delegate
            .write_text_file(request)
            .await
            .expect("write_text_file should succeed");

        // Verify the response is valid
        assert_eq!(response, acp::WriteTextFileResponse::new());

        // Verify the file was written
        let written_content = std::fs::read_to_string(&test_file).expect("File should exist");
        assert_eq!(written_content, content);

        // Verify that a ToolCall SessionUpdate was emitted
        let update = update_rx
            .try_recv()
            .expect("Should have received a SessionUpdate");

        match update {
            acp::SessionUpdate::ToolCall(tool_call) => {
                assert_eq!(tool_call.status, acp::ToolCallStatus::Pending);
                assert!(
                    tool_call.title.contains("write_text_file")
                        || tool_call.title.contains("Writing")
                        || tool_call.title.contains("output.txt"),
                    "Title should indicate file write operation, got: {}",
                    tool_call.title
                );
                assert_eq!(tool_call.kind, acp::ToolKind::Execute);
            }
            other => panic!("Expected ToolCall update, got: {other:?}"),
        }

        delegate.unregister_session(&session_id);
    }
}
