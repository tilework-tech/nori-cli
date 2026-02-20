use super::*;

/// Spawns the connection on the current LocalSet.
pub(super) async fn spawn_connection_internal(
    config: &AcpAgentConfig,
    cwd: &Path,
    approval_tx: mpsc::Sender<ApprovalRequest>,
    persistent_tx: mpsc::Sender<acp::SessionUpdate>,
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
    let delegate = ClientDelegate::new(cwd.to_path_buf(), approval_tx);
    delegate.set_persistent_listener(persistent_tx);
    let client_delegate = Rc::new(delegate);

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
pub(super) async fn run_command_loop(
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
        debug!("Failed to kill process group: {e}");
    }

    // Then kill the direct child (this is a no-op if process group kill succeeded).
    if let Err(e) = inner.child.start_kill() {
        debug!("Failed to kill ACP agent child process: {e}");
    }

    // Wait for actual termination with a short timeout.
    // If grandchildren kept pipes open, this prevents hanging indefinitely.
    match tokio::time::timeout(Duration::from_millis(500), inner.child.wait()).await {
        Ok(Ok(status)) => {
            debug!("ACP agent exited with status: {status:?}");
        }
        Ok(Err(e)) => {
            debug!("Error waiting for ACP agent exit: {e}");
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
