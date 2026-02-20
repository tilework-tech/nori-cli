use super::*;

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

        // Create persistent listener channel for inter-turn notifications.
        // Sender goes to the worker (ClientDelegate), receiver stays here.
        let (persistent_tx, persistent_rx) = mpsc::channel::<acp::SessionUpdate>(64);

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
                        match worker::spawn_connection_internal(
                            &config,
                            &cwd,
                            approval_tx,
                            persistent_tx,
                        )
                        .await
                        {
                            Ok((inner, capabilities)) => {
                                let _ = init_tx.send(Ok(capabilities));
                                worker::run_command_loop(
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
            persistent_rx,
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

    /// Take ownership of the persistent notification receiver.
    ///
    /// Inter-turn notifications (arriving after `unregister_session` but before
    /// the next `register_session`) are forwarded through this channel. The UI
    /// layer should drain it and translate updates into codex events.
    pub fn take_persistent_receiver(&mut self) -> mpsc::Receiver<acp::SessionUpdate> {
        std::mem::replace(&mut self.persistent_rx, mpsc::channel(1).1)
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
