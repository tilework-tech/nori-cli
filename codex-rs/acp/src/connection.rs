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
use std::sync::RwLock;
use std::thread;

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

/// A thread-safe wrapper around an ACP agent subprocess.
///
/// This spawns a dedicated single-threaded tokio runtime on a background thread
/// to handle the ACP protocol (which requires `!Send` futures), and communicates
/// with the main runtime via channels.
pub struct AcpConnection {
    command_tx: mpsc::Sender<AcpCommand>,
    agent_capabilities: acp::AgentCapabilities,
    /// Channel to receive approval requests from the agent.
    /// The UI layer should listen on this channel and respond via the oneshot sender.
    approval_rx: mpsc::Receiver<ApprovalRequest>,
    /// Thread-safe model state shared between the main thread and worker thread.
    /// Updated when sessions are created or models are switched.
    model_state: Arc<RwLock<AcpModelState>>,
    _worker_thread: thread::JoinHandle<()>,
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
                                run_command_loop(inner, command_rx, model_state_for_worker).await;
                            }
                            Err(e) => {
                                let _ = init_tx.send(Err(e));
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
            _worker_thread: worker_thread,
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

/// Internal connection state that lives on the worker thread.
struct AcpConnectionInner {
    connection: acp::ClientSideConnection,
    #[allow(dead_code)]
    client_delegate: Rc<ClientDelegate>,
    child: Child,
    #[allow(dead_code)]
    io_task: tokio::task::JoinHandle<acp::Result<()>>,
    #[allow(dead_code)]
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

    let mut child = Command::new(&config.command)
        .args(&config.args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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
async fn run_command_loop(
    mut inner: AcpConnectionInner,
    mut command_rx: mpsc::Receiver<AcpCommand>,
    model_state: Arc<RwLock<AcpModelState>>,
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
    // This happens when the AcpConnection is dropped (e.g., during session switch)
    debug!("ACP command loop exiting, terminating child process");
    if let Err(e) = inner.child.kill().await {
        // Log but don't fail - process may have already exited
        debug!("Failed to kill ACP agent child process: {}", e);
    }
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
            // Non-blocking send - if channel is full or closed, we drop the update
            let _ = tx.try_send(notification.update);
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
}
