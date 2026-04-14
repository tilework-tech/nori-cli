use super::*;

impl AcpBackend {
    /// Spawn an ACP backend for the given configuration.
    ///
    /// This will:
    /// 1. Look up the agent config from the registry
    /// 2. Spawn the ACP connection
    /// 3. Create a session
    /// 4. Send a synthetic `SessionConfigured` event
    /// 5. Start background tasks for control-plane forwarding, approvals, and normalized session updates
    ///
    /// # Arguments
    /// * `config` - The ACP backend configuration
    /// * `backend_event_tx` - Channel to send ACP backend events to the TUI
    ///
    /// # Returns
    /// A connected `AcpBackend` ready to receive operations.
    pub async fn spawn(
        config: &AcpBackendConfig,
        backend_event_tx: mpsc::Sender<BackendEvent>,
    ) -> Result<Self> {
        let agent_config = get_agent_config(&config.agent)?;
        let cwd = config.cwd.clone();

        let (event_tx, event_rx) = mpsc::channel(32);
        tokio::spawn(forward_control_events(event_rx, backend_event_tx.clone()));

        debug!("Spawning ACP backend for agent: {}", config.agent);

        // Spawn the ACP connection with enhanced error handling
        let connection_result = SacpConnection::spawn(&agent_config, &cwd).await;

        let mut connection = match connection_result {
            Ok(conn) => conn,
            Err(e) => {
                // Get the full error chain to check for nested auth errors
                let error_string = format!("{e:?}");
                let category = categorize_acp_error(&error_string);

                // Use the display format for the user-facing message
                let display_error = format!("{e}");
                let enhanced_message = enhanced_error_message(
                    category,
                    &display_error,
                    &agent_config.provider_info.name,
                    &agent_config.auth_hint,
                    &agent_config.display_name,
                    &agent_config.install_hint,
                );

                return Err(anyhow::anyhow!(enhanced_message));
            }
        };

        // Create a session with enhanced error handling, forwarding CLI MCP servers.
        let mcp_servers = crate::connection::mcp::to_sacp_mcp_servers(&config.mcp_servers);
        let session_result = connection.create_session(&cwd, mcp_servers).await;
        let session_id = match session_result {
            Ok(id) => id,
            Err(e) => {
                // Get the full error chain to check for nested auth errors
                let error_string = format!("{e:?}");
                let category = categorize_acp_error(&error_string);

                // Use the display format for the user-facing message
                let display_error = format!("{e}");
                let enhanced_message = enhanced_error_message(
                    category,
                    &display_error,
                    &agent_config.provider_info.name,
                    &agent_config.auth_hint,
                    &agent_config.display_name,
                    &agent_config.install_hint,
                );

                return Err(anyhow::anyhow!(enhanced_message));
            }
        };

        debug!("ACP session created: {:?}", session_id);

        // Apply default model from config if one is set for this agent
        #[cfg(feature = "unstable")]
        if let Some(ref default_model) = config.default_model {
            let model_state = connection.model_state();
            let model_available = model_state
                .available_models
                .iter()
                .any(|m| m.model_id.to_string() == *default_model);
            if model_available {
                let model_id = acp::ModelId::from(default_model.clone());
                match connection.set_model(&session_id, &model_id).await {
                    Ok(()) => {
                        debug!("Applied default model from config: {default_model}");
                    }
                    Err(e) => {
                        warn!("Failed to apply default model '{default_model}': {e}");
                    }
                }
            } else {
                debug!("Default model '{default_model}' not in available models, skipping");
            }
        }

        // Take the approval receiver for handling permission requests
        let approval_rx = connection.take_approval_receiver();
        let notification_rx = connection.take_notification_receiver();

        let connection = Arc::new(connection);
        let pending_approvals = Arc::new(Mutex::new(Vec::new()));
        let session_driver = Arc::new(Mutex::new(session_runtime_driver::SessionDriver::new()));
        let (session_event_tx, mut session_event_rx) = mpsc::channel(128);
        let (prompt_result_tx, prompt_result_rx) = mpsc::channel(128);
        let use_native_notifications =
            config.os_notifications == crate::config::OsNotifications::Enabled;
        let user_notifier = Arc::new(codex_core::UserNotifier::new(
            config.notify.clone(),
            use_native_notifications,
        ));

        let idle_timer_abort = Arc::new(Mutex::new(None));

        // Create watch channel for dynamic approval policy updates
        let (approval_policy_tx, approval_policy_rx) = watch::channel(config.approval_policy);

        // Create conversation ID for this session
        let conversation_id = ConversationId::new();

        // Get history metadata
        let (history_log_id, history_entry_count) =
            crate::message_history::history_metadata(&config.nori_home).await;

        // Initialize transcript recorder (non-fatal if it fails)
        let transcript_recorder = match TranscriptRecorder::new(
            &config.nori_home,
            &cwd,
            Some(config.agent.clone()),
            &config.cli_version,
            Some(session_id.to_string()),
        )
        .await
        {
            Ok(recorder) => Some(Arc::new(recorder)),
            Err(e) => {
                warn!("Failed to initialize transcript recorder: {e}");
                None
            }
        };

        let backend = Self {
            connection,
            session_id: Arc::new(RwLock::new(session_id)),
            event_tx: event_tx.clone(),
            backend_event_tx: backend_event_tx.clone(),
            cwd: cwd.clone(),
            pending_approvals: Arc::clone(&pending_approvals),
            user_notifier: Arc::clone(&user_notifier),
            idle_timer_abort: Arc::clone(&idle_timer_abort),
            nori_home: config.nori_home.clone(),
            history_persistence: config.history_persistence,
            conversation_id,
            approval_policy_tx,
            pending_compact_summary: Arc::new(Mutex::new(config.initial_context.clone())),
            pending_hook_context: Arc::new(Mutex::new(config.session_context.clone())),
            transcript_recorder,
            session_event_tx: session_event_tx.clone(),
            prompt_result_tx: prompt_result_tx.clone(),
            notify_after_idle: config.notify_after_idle,
            ghost_snapshots: Arc::new(GhostSnapshotStack::new()),
            is_first_prompt: Arc::new(Mutex::new(true)),
            agent_name: config.agent.clone(),
            auto_worktree: config.auto_worktree,
            auto_worktree_repo_root: config.auto_worktree_repo_root.clone(),
            session_end_hooks: config.session_end_hooks.clone(),
            pre_user_prompt_hooks: config.pre_user_prompt_hooks.clone(),
            post_user_prompt_hooks: config.post_user_prompt_hooks.clone(),
            post_agent_response_hooks: config.post_agent_response_hooks.clone(),
            async_session_end_hooks: config.async_session_end_hooks.clone(),
            async_pre_user_prompt_hooks: config.async_pre_user_prompt_hooks.clone(),
            async_post_user_prompt_hooks: config.async_post_user_prompt_hooks.clone(),
            async_post_agent_response_hooks: config.async_post_agent_response_hooks.clone(),
            script_timeout: config.script_timeout,
            session_driver: Arc::clone(&session_driver),
            mcp_servers: config.mcp_servers.clone(),
        };

        let runtime_backend = backend.clone();
        tokio::spawn(async move {
            while let Some(input) = session_event_rx.recv().await {
                match input {
                    session_runtime_driver::SessionRuntimeInput::Reducer(event) => {
                        runtime_backend.apply_session_event(event).await;
                    }
                    session_runtime_driver::SessionRuntimeInput::PermissionRequest {
                        pending_request,
                        current_policy,
                    } => {
                        runtime_backend
                            .handle_permission_request(pending_request, current_policy)
                            .await;
                    }
                }
            }
        });

        // Execute session_start hooks
        run_session_start_hooks(
            &config.session_start_hooks,
            config.script_timeout,
            &event_tx,
            Some(&backend.pending_hook_context),
        )
        .await;

        // Fire-and-forget async session start hooks
        let _ = crate::hooks::execute_hooks_fire_and_forget(
            config.async_session_start_hooks.clone(),
            config.script_timeout,
            HashMap::new(),
        );

        // Send synthetic SessionConfigured event
        let session_configured = SessionConfiguredEvent {
            session_id: conversation_id,
            model: config.agent.clone(),
            model_provider_id: "acp".to_string(),
            approval_policy: config.approval_policy,
            sandbox_policy: config.sandbox_policy.clone(),
            cwd: cwd.clone(),
            reasoning_effort: None,
            history_log_id,
            history_entry_count,
            initial_messages: None,
            rollout_path: cwd.join(".codex-rollout.jsonl"),
        };

        event_tx
            .send(Event {
                id: String::new(),
                msg: EventMsg::SessionConfigured(session_configured),
            })
            .await
            .ok();

        // Spawn approval handler task
        tokio::spawn(Self::run_approval_handler(
            backend.clone(),
            approval_rx,
            Arc::clone(&pending_approvals),
            Arc::clone(&user_notifier),
            approval_policy_rx,
        ));

        // Spawn reducer loop: processes ALL session notifications through the
        // serialized reducer, replacing the old per-prompt update handler and
        // persistent relay.
        tokio::spawn(Self::run_notification_relay(
            backend.clone(),
            notification_rx,
            prompt_result_rx,
        ));

        Ok(backend)
    }

    /// Background task to handle approval requests from the ACP connection.
    ///
    /// When `approval_policy` is `AskForApproval::Never` (yolo mode), requests
    /// are auto-approved without prompting the user.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn run_approval_handler(
        backend: AcpBackend,
        mut approval_rx: mpsc::Receiver<ApprovalRequest>,
        _pending_approvals: Arc<Mutex<Vec<PendingApprovalRequest>>>,
        _user_notifier: Arc<codex_core::UserNotifier>,
        approval_policy_rx: watch::Receiver<AskForApproval>,
    ) {
        let approval_policy_rx = approval_policy_rx;
        while let Some(request) = approval_rx.recv().await {
            let current_policy = *approval_policy_rx.borrow();
            let _ = backend
                .session_event_tx
                .send(
                    session_runtime_driver::SessionRuntimeInput::PermissionRequest {
                        pending_request: Box::new(PendingApprovalRequest {
                            request_id: request.request_id.clone(),
                            request,
                        }),
                        current_policy,
                    },
                )
                .await;
        }
    }

    /// Background task that processes ALL session notifications through the
    /// serialized session reducer instead of forwarding them directly.
    pub(super) async fn run_notification_relay(
        backend: AcpBackend,
        mut notification_rx: mpsc::Receiver<acp::SessionUpdate>,
        mut prompt_result_rx: mpsc::Receiver<session_reducer::InboundEvent>,
    ) {
        loop {
            tokio::select! {
                biased;
                maybe_update = notification_rx.recv() => {
                    match maybe_update {
                        Some(update) => {
                            let _ = backend
                                .session_event_tx
                                .send(session_runtime_driver::SessionRuntimeInput::Reducer(
                                    session_reducer::InboundEvent::Notification(Box::new(update)),
                                ))
                                .await;
                        }
                        None => break,
                    }
                }
                maybe_result = prompt_result_rx.recv() => {
                    match maybe_result {
                        Some(result) => {
                            let _ = backend
                                .session_event_tx
                                .send(session_runtime_driver::SessionRuntimeInput::Reducer(result))
                                .await;
                        }
                        None => {
                            while let Some(update) = notification_rx.recv().await {
                                let _ = backend
                                    .session_event_tx
                                    .send(session_runtime_driver::SessionRuntimeInput::Reducer(
                                        session_reducer::InboundEvent::Notification(Box::new(update)),
                                    ))
                                    .await;
                            }
                            break;
                        }
                    }
                }
            }
        }
    }
}
