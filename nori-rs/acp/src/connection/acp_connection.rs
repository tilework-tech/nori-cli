//! ACP connection layer built on the `agent-client-protocol` SDK.
//!
//! The SDK's `ConnectionTo<Agent>` is `Send + Sync`, so all operations run
//! directly on the main tokio runtime without a dedicated worker thread or
//! `LocalSet`.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;

use agent_client_protocol::Agent;
use agent_client_protocol::Client;
use agent_client_protocol::ConnectionTo;
use agent_client_protocol::Lines;
use agent_client_protocol_schema::ProtocolVersion;
use agent_client_protocol_schema::v1 as acp;
use anyhow::Context;
use anyhow::Result;
use futures::AsyncBufReadExt;
use futures::AsyncWriteExt;
use futures::StreamExt;
use futures::io::BufReader;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tokio_util::compat::TokioAsyncWriteCompatExt;
use tracing::debug;
use tracing::warn;

use super::AcpSessionConfigState;
use super::ApprovalEventType;
use super::ApprovalRequest;
use super::ConnectionEvent;
use super::child_lifecycle::ChildHandle;
use super::child_lifecycle::SHUTDOWN_GRACE;
use super::child_lifecycle::STDERR_TAIL_LINES;
use super::child_lifecycle::collect_stderr_tail;
use super::wire_log::WireDirection;
use super::wire_log::WireLogger;
use crate::config::AcpProxyConfig;
use crate::registry::AcpAgentConfig;
use crate::translator;

/// Minimum supported ACP protocol version.
const MINIMUM_SUPPORTED_VERSION: ProtocolVersion = ProtocolVersion::V1;

#[derive(Debug, Default)]
struct SessionPromptState {
    update_seq: i64,
    draining_cancel_tail: bool,
}

/// A thread-safe connection to an ACP agent subprocess.
///
/// The `ConnectionTo<Agent>` from the `agent-client-protocol` SDK is
/// `Send + Sync`, so all operations run directly on the main tokio runtime
/// without a dedicated worker thread.
///
/// Internal architecture:
/// - A background tokio task runs the ACP connection via `connect_with`.
/// - The `ConnectionTo<Agent>` is cloned out and used for all subsequent requests.
/// - Session notifications and approval requests are forwarded via channels.
/// - All session-domain traffic flows through a single ordered inbox.
pub struct AcpConnection {
    /// Connection context for sending requests to the agent.
    cx: ConnectionTo<Agent>,

    /// Agent capabilities from the initialization handshake.
    agent_capabilities: acp::AgentCapabilities,

    /// Ordered inbox of raw ACP events from the transport layer.
    event_rx: mpsc::Receiver<ConnectionEvent>,

    /// Per-session prompt boundary state used to absorb stale terminal stop
    /// responses after cancellation without widening the public phase model.
    prompt_state: std::sync::Arc<Mutex<HashMap<String, SessionPromptState>>>,

    /// Thread-safe session config state, updated from complete ACP snapshots.
    session_config_state: std::sync::Arc<std::sync::RwLock<AcpSessionConfigState>>,

    /// Handle to the background task driving the ACP connection.
    connection_task: tokio::task::JoinHandle<()>,

    /// Handles to the child process for teardown (None for remote connections).
    child: Option<ChildHandle>,

    /// Handle to the stderr logging task (None for remote connections).
    stderr_task: Option<tokio::task::JoinHandle<()>>,
}

/// Shared ACP connection setup: registers notification/request handlers,
/// performs the initialization handshake, and returns a `AcpConnection` with
/// no process handles (`child`/`stderr_task` are `None`). `spawn()` fills in the
/// subprocess handles afterward.
///
/// The caller supplies the connection event channel so it can hold extra
/// senders (e.g. the child exit watcher reports through the same stream).
async fn establish_connection(
    transport: impl agent_client_protocol::ConnectTo<Client> + 'static,
    cwd: &Path,
    event_tx: mpsc::Sender<ConnectionEvent>,
    event_rx: mpsc::Receiver<ConnectionEvent>,
) -> Result<AcpConnection> {
    let event_tx_for_notifications = event_tx.clone();
    let event_tx_for_write = event_tx.clone();
    let event_tx_for_read = event_tx.clone();
    let prompt_state =
        std::sync::Arc::new(Mutex::new(HashMap::<String, SessionPromptState>::new()));
    let prompt_state_for_notifications = prompt_state.clone();
    let session_config_state =
        std::sync::Arc::new(std::sync::RwLock::new(AcpSessionConfigState::new()));
    let session_config_state_for_notifications = session_config_state.clone();
    let approval_cwd = cwd.to_path_buf();
    let write_cwd = cwd.to_path_buf();
    let read_cwd = cwd.to_path_buf();

    let (init_tx, init_rx) =
        oneshot::channel::<Result<(ConnectionTo<Agent>, acp::AgentCapabilities)>>();

    let connection_task = tokio::spawn(async move {
        let result = Client
            .builder()
            .on_receive_notification(
                {
                    let event_tx = event_tx_for_notifications;
                    let prompt_state = prompt_state_for_notifications;
                    let session_config_state = session_config_state_for_notifications;
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
                        if let acp::SessionUpdate::ConfigOptionUpdate(update) = &notification.update
                            && let Ok(mut state) = session_config_state.write()
                        {
                            state.config_options = update.config_options.clone();
                        }
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
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                {
                    let event_tx = event_tx.clone();
                    let cwd = approval_cwd;
                    async move |request: acp::RequestPermissionRequest,
                                responder: agent_client_protocol::Responder<
                        acp::RequestPermissionResponse,
                    >,
                                connection: ConnectionTo<Agent>| {
                        let event = if let Some(patch_event) =
                            translator::permission_request_to_patch_approval_event(&request, &cwd)
                        {
                            ApprovalEventType::Patch(patch_event)
                        } else {
                            let exec_event =
                                translator::permission_request_to_approval_event(&request, &cwd);
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

                        connection.spawn(async move {
                            let outcome = match response_rx.await {
                                Ok(decision) => translator::review_decision_to_permission_outcome(
                                    decision,
                                    &request.options,
                                ),
                                Err(_) => {
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
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let event_tx = event_tx_for_write;
                    let cwd = write_cwd;
                    async move |request: acp::WriteTextFileRequest,
                                responder: agent_client_protocol::Responder<
                        acp::WriteTextFileResponse,
                    >,
                                _connection: ConnectionTo<Agent>| {
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
                                resolved_path.starts_with(&cwd) || resolved_path.starts_with("/tmp")
                            }
                        } else {
                            false
                        };

                        if !allowed {
                            responder.respond_with_error(
                                agent_client_protocol::Error::invalid_params().data(format!(
                                    "Write restricted to working directory ({}) or /tmp. Path: {}",
                                    cwd.display(),
                                    resolved_path.display()
                                )),
                            )?;
                            return Ok(());
                        }

                        if let Some(parent) = resolved_path.parent()
                            && !parent.exists()
                            && let Err(e) = std::fs::create_dir_all(parent)
                        {
                            responder.respond_with_error(
                                agent_client_protocol::util::internal_error(e.to_string()),
                            )?;
                            return Ok(());
                        }

                        match std::fs::write(&resolved_path, &request.content) {
                            Ok(()) => {
                                responder.respond(acp::WriteTextFileResponse::new())?;
                            }
                            Err(e) => {
                                responder.respond_with_error(
                                    agent_client_protocol::util::internal_error(e.to_string()),
                                )?;
                            }
                        }
                        Ok(())
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let event_tx = event_tx_for_read;
                    let cwd = read_cwd;
                    async move |request: acp::ReadTextFileRequest,
                                responder: agent_client_protocol::Responder<
                        acp::ReadTextFileResponse,
                    >,
                                _connection: ConnectionTo<Agent>| {
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
                                responder.respond_with_error(
                                    agent_client_protocol::util::internal_error(e.to_string()),
                                )?;
                            }
                        }
                        Ok(())
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_with(transport, |connection: ConnectionTo<Agent>| async move {
                let response = connection
                    .send_request(
                        acp::InitializeRequest::new(ProtocolVersion::LATEST)
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
                            return Err(agent_client_protocol::util::internal_error(
                                "Protocol version too old",
                            ));
                        }
                        debug!("ACP connection established, agent: {:?}", resp.agent_info);
                        let _ = init_tx.send(Ok((connection.clone(), resp.agent_capabilities)));

                        futures::future::pending::<Result<(), agent_client_protocol::Error>>().await
                    }
                    Err(e) => {
                        let _ =
                            init_tx.send(Err(anyhow::anyhow!("ACP initialization failed: {e}")));
                        Err(e)
                    }
                }
            })
            .await;

        if let Err(e) = result {
            debug!("ACP connection task ended: {e}");
        }
    });

    let (cx, capabilities) = init_rx
        .await
        .context("ACP connection task died during initialization")??;

    Ok(AcpConnection {
        cx,
        agent_capabilities: capabilities,
        event_rx,
        prompt_state,
        session_config_state,
        connection_task,
        child: None,
        stderr_task: None,
    })
}

impl AcpConnection {
    /// Spawn a new ACP agent subprocess and establish a connection.
    pub async fn spawn(
        config: &AcpAgentConfig,
        cwd: &Path,
        proxy_config: AcpProxyConfig,
    ) -> Result<Self> {
        debug!(
            "Spawning ACP agent: {} {:?} in {}",
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
        let pid = child.id().context("Failed to get ACP agent pid")?;

        debug!("ACP agent spawned (pid: Some({pid}))");

        let wire_logger = if proxy_config.enabled {
            Some(WireLogger::new(&proxy_config, config, pid)?)
        } else {
            None
        };

        // Log stderr in the background, keeping a bounded tail so child
        // failures can surface the real cause (e.g. an auth hint) to the user
        // instead of only to the log file.
        let stderr_tail = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::<
            String,
        >::new()));
        let stderr_task = tokio::spawn({
            let stderr_tail = std::sync::Arc::clone(&stderr_tail);
            async move {
                let mut stderr = BufReader::new(stderr.compat());
                let mut line = String::new();
                while let Ok(n) = stderr.read_line(&mut line).await {
                    if n == 0 {
                        break;
                    }
                    let trimmed = line.trim();
                    warn!("ACP agent stderr: {trimmed}");
                    if let Ok(mut tail) = stderr_tail.lock() {
                        if tail.len() >= STDERR_TAIL_LINES {
                            tail.pop_front();
                        }
                        tail.push_back(trimmed.to_string());
                    }
                    line.clear();
                }
            }
        });

        // The exit watcher owns the `Child`: it reaps the process, publishes
        // the exit status, and reports unexpected deaths into the ordered
        // event stream. Without it, a dead child is a silent EOF that the
        // ACP layer treats as non-terminal — pending requests hang forever.
        let (event_tx, event_rx) = mpsc::channel::<ConnectionEvent>(1024);
        let (exit_tx, exit_rx) = tokio::sync::watch::channel(None);
        let kill_notify = std::sync::Arc::new(tokio::sync::Notify::new());
        tokio::spawn({
            let event_tx = event_tx.clone();
            let stderr_tail = std::sync::Arc::clone(&stderr_tail);
            let kill_notify = std::sync::Arc::clone(&kill_notify);
            async move {
                let status = tokio::select! {
                    status = child.wait() => status.ok(),
                    _ = kill_notify.notified() => {
                        let _ = child.start_kill();
                        child.wait().await.ok()
                    }
                };
                debug!("ACP agent exited: {status:?}");
                let _ = exit_tx.send(status);
                // Give the stderr reader a moment to drain the pipe before
                // snapshotting the tail.
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                let _ = event_tx
                    .send(ConnectionEvent::ChildExited {
                        status: status.and_then(|s| s.code()),
                        stderr_tail: collect_stderr_tail(&stderr_tail),
                    })
                    .await;
            }
        });

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

        // Race initialization against the child dying: an agent that exits
        // immediately (e.g. unauthenticated) must fail spawn fast, with its
        // stderr as the cause — not hang the handshake forever.
        let mut exit_watch = exit_rx.clone();
        // The watch Ref returned by wait_for is !Send; discard it inside the
        // branch future so the whole select stays Send.
        let child_died = async move {
            let _ = exit_watch.wait_for(std::option::Option::is_some).await;
        };
        let init = establish_connection(transport, cwd, event_tx, event_rx);
        let init_result = tokio::select! {
            result = init => result,
            () = child_died => Err(anyhow::anyhow!("agent exited before initialization finished")),
        };
        let mut connection = match init_result {
            Ok(connection) => connection,
            Err(error) => {
                // Whichever branch lost the race, a dead child is the real
                // cause (a handshake error like a broken pipe is just the
                // symptom). Give the watcher and stderr reader a moment to
                // observe the exit, then prefer the child's status + stderr.
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                let exited = exit_rx
                    .borrow()
                    .as_ref()
                    .map(std::process::ExitStatus::code);
                let Some(status) = exited else {
                    return Err(error);
                };
                let tail = collect_stderr_tail(&stderr_tail);
                stderr_task.abort();
                let status_desc = match status {
                    Some(code) => format!("exit code {code}"),
                    None => "killed by signal".to_string(),
                };
                let cause = if tail.is_empty() {
                    String::new()
                } else {
                    format!(": {tail}")
                };
                anyhow::bail!(
                    "ACP agent {} exited during startup ({status_desc}){cause}",
                    config.command
                );
            }
        };
        connection.child = Some(ChildHandle {
            pid,
            exit_rx,
            kill: kill_notify,
        });
        connection.stderr_task = Some(stderr_task);
        Ok(connection)
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

        if let Some(config_options) = response.config_options
            && let Ok(mut state) = self.session_config_state.write()
        {
            state.config_options = config_options;
        }

        Ok(response.session_id)
    }

    /// Load (resume) an existing session.
    ///
    /// The agent replays previous session history. Updates flow through the
    /// ordered event inbox. The returned `SessionId` is the same as
    /// the input `session_id` (the LoadSessionResponse doesn't contain one).
    pub async fn load_session(
        &self,
        session_id: &str,
        cwd: &Path,
        mcp_servers: Vec<acp::McpServer>,
    ) -> Result<acp::SessionId> {
        let response = self
            .cx
            .send_request(
                acp::LoadSessionRequest::new(session_id.to_string(), cwd).mcp_servers(mcp_servers),
            )
            .block_task()
            .await
            .context("Failed to load ACP session")?;

        if let Some(config_options) = response.config_options
            && let Ok(mut state) = self.session_config_state.write()
        {
            state.config_options = config_options;
        }

        // The session ID from the request is reused since the response
        // doesn't contain one.
        Ok(acp::SessionId::from(session_id.to_string()))
    }

    /// Fork (branch) an existing session at its current state via
    /// `session/fork` (unstable ACP extension, gated on the agent advertising
    /// the `fork` session capability).
    ///
    /// The returned ID identifies the new session; the source session stays
    /// valid on the agent side. Like `create_session`, the response's live
    /// config options replace the current snapshot before this returns.
    pub async fn fork_session(
        &self,
        session_id: &acp::SessionId,
        cwd: &Path,
        mcp_servers: Vec<acp::McpServer>,
    ) -> Result<acp::SessionId> {
        let response = self
            .cx
            .send_request(
                acp::ForkSessionRequest::new(session_id.clone(), cwd).mcp_servers(mcp_servers),
            )
            .block_task()
            .await
            .context("Failed to fork ACP session")?;

        if let Some(config_options) = response.config_options
            && let Ok(mut state) = self.session_config_state.write()
        {
            state.config_options = config_options;
        }

        Ok(response.session_id)
    }

    /// Whether the connected agent advertises the `session/fork` capability.
    pub fn supports_fork(&self) -> bool {
        self.agent_capabilities.session_capabilities.fork.is_some()
    }

    /// List the agent's known sessions via ACP `session/list`.
    ///
    /// `cwd` is forwarded as the spec's working-directory filter. Cursor-based
    /// pagination is drained internally, so the returned vector contains every
    /// page concatenated in agent order.
    pub async fn list_sessions(
        &self,
        cwd: &Path,
    ) -> Result<Vec<crate::connection::AcpSessionSummary>> {
        // Bound pagination so a misbehaving agent that keeps returning a
        // `next_cursor` cannot pin the picker in an unbounded loop.
        const MAX_SESSION_LIST_PAGES: usize = 1000;

        let mut summaries = Vec::new();
        let mut cursor: Option<String> = None;

        for _ in 0..MAX_SESSION_LIST_PAGES {
            let mut request = acp::ListSessionsRequest::new().cwd(cwd.to_path_buf());
            if let Some(cursor) = cursor.take() {
                request = request.cursor(cursor);
            }

            let response = self
                .cx
                .send_request(request)
                .block_task()
                .await
                .context("Failed to list ACP sessions")?;

            summaries.extend(response.sessions.into_iter().map(|session| {
                crate::connection::AcpSessionSummary {
                    session_id: session.session_id.to_string(),
                    cwd: session.cwd,
                    title: session.title,
                    updated_at: session.updated_at,
                }
            }));

            match response.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }

        Ok(summaries)
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

    /// Get the current ACP session config snapshot.
    pub fn config_options(&self) -> Vec<acp::SessionConfigOption> {
        #[expect(
            clippy::expect_used,
            reason = "RwLock poisoning indicates a bug elsewhere"
        )]
        self.session_config_state
            .read()
            .expect("Session config state lock poisoned")
            .config_options
            .clone()
    }

    /// Set an ACP session config option and replace state from the full response snapshot.
    pub async fn set_config_option(
        &self,
        session_id: &acp::SessionId,
        config_id: impl Into<acp::SessionConfigId>,
        value: impl Into<acp::SessionConfigValueId>,
    ) -> Result<()> {
        let value = acp::SessionConfigOptionValue::value_id(value.into());
        let response = self
            .cx
            .send_request(acp::SetSessionConfigOptionRequest::new(
                session_id.clone(),
                config_id,
                value,
            ))
            .block_task()
            .await
            .context("Failed to set ACP session config option")?;

        if let Ok(mut state) = self.session_config_state.write() {
            state.config_options = response.config_options;
        }

        Ok(())
    }

    /// Explicitly tear down the ACP subprocess and background tasks with the
    /// default grace period.
    ///
    /// Unlike `Drop`, this async path can wait for process termination so the
    /// child is reaped promptly during agent switches and shutdown.
    pub async fn shutdown(&self) {
        self.shutdown_with_grace(SHUTDOWN_GRACE).await;
    }

    /// Tear down the ACP subprocess gracefully: close its stdin, give it
    /// `grace` to exit on its own (agents like `nori-handroll cloud-acp` run
    /// their release path on stdin EOF), then kill whatever is left.
    pub async fn shutdown_with_grace(&self, grace: std::time::Duration) {
        // Aborting the connection task drops the transport and with it the
        // child's stdin writer — the EOF is the agent's shutdown signal.
        self.connection_task.abort();

        if let Some(ref child) = self.child {
            let mut exit_rx = child.exit_rx.clone();
            let exited_in_time =
                tokio::time::timeout(grace, exit_rx.wait_for(std::option::Option::is_some))
                    .await
                    .is_ok();
            if !exited_in_time {
                debug!("ACP agent did not exit within {grace:?} of stdin EOF; killing");
                child.request_kill();
                // Wait for the watcher to reap so no zombie outlives shutdown.
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    exit_rx.wait_for(std::option::Option::is_some),
                )
                .await;
            }
        }

        if let Some(ref stderr_task) = self.stderr_task {
            stderr_task.abort();
        }
    }
}

impl Drop for AcpConnection {
    fn drop(&mut self) {
        self.connection_task.abort();
        if let Some(ref stderr_task) = self.stderr_task {
            stderr_task.abort();
        }

        // Backstop for paths that never ran `shutdown()`: kill outright.
        if let Some(ref child) = self.child {
            child.request_kill();
        }
    }
}
