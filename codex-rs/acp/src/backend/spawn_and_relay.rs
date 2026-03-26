use super::*;

impl AcpBackend {
    /// Spawn an ACP backend for the given configuration.
    ///
    /// This will:
    /// 1. Look up the agent config from the registry
    /// 2. Spawn the ACP connection
    /// 3. Create a session
    /// 4. Send a synthetic `SessionConfigured` event
    /// 5. Start background tasks for event translation and approval handling
    ///
    /// # Arguments
    /// * `config` - The ACP backend configuration
    /// * `event_tx` - Channel to send translated events to the TUI
    ///
    /// # Returns
    /// A connected `AcpBackend` ready to receive operations.
    pub async fn spawn(config: &AcpBackendConfig, event_tx: mpsc::Sender<Event>) -> Result<Self> {
        let agent_config = get_agent_config(&config.agent)?;
        let cwd = config.cwd.clone();

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

        // Create a session with enhanced error handling
        let session_result = connection.create_session(&cwd).await;
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
        let persistent_rx = connection.take_persistent_receiver();

        let connection = Arc::new(connection);
        let pending_approvals = Arc::new(Mutex::new(Vec::new()));
        let pending_tool_calls = Arc::new(Mutex::new(HashMap::new()));
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
            cwd: cwd.clone(),
            pending_approvals: Arc::clone(&pending_approvals),
            user_notifier: Arc::clone(&user_notifier),
            idle_timer_abort: Arc::clone(&idle_timer_abort),
            nori_home: config.nori_home.clone(),
            history_persistence: config.history_persistence,
            conversation_id,
            approval_policy_tx,
            pending_compact_summary: Arc::new(Mutex::new(config.initial_context.clone())),
            pending_hook_context: Arc::new(Mutex::new(None)),
            transcript_recorder,
            notify_after_idle: config.notify_after_idle,
            ghost_snapshots: Arc::new(GhostSnapshotStack::new()),
            is_first_prompt: Arc::new(Mutex::new(true)),
            agent_name: config.agent.clone(),
            auto_worktree: config.auto_worktree,
            auto_worktree_repo_root: config.auto_worktree_repo_root.clone(),
            session_end_hooks: config.session_end_hooks.clone(),
            pre_user_prompt_hooks: config.pre_user_prompt_hooks.clone(),
            post_user_prompt_hooks: config.post_user_prompt_hooks.clone(),
            pre_tool_call_hooks: config.pre_tool_call_hooks.clone(),
            post_tool_call_hooks: config.post_tool_call_hooks.clone(),
            pre_agent_response_hooks: config.pre_agent_response_hooks.clone(),
            post_agent_response_hooks: config.post_agent_response_hooks.clone(),
            async_session_end_hooks: config.async_session_end_hooks.clone(),
            async_pre_user_prompt_hooks: config.async_pre_user_prompt_hooks.clone(),
            async_post_user_prompt_hooks: config.async_post_user_prompt_hooks.clone(),
            async_pre_tool_call_hooks: config.async_pre_tool_call_hooks.clone(),
            async_post_tool_call_hooks: config.async_post_tool_call_hooks.clone(),
            async_pre_agent_response_hooks: config.async_pre_agent_response_hooks.clone(),
            async_post_agent_response_hooks: config.async_post_agent_response_hooks.clone(),
            script_timeout: config.script_timeout,
            pending_tool_calls: Arc::clone(&pending_tool_calls),
            mcp_servers: config.mcp_servers.clone(),
            mcp_oauth_credentials_store_mode: config.mcp_oauth_credentials_store_mode,
        };

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
            approval_rx,
            event_tx.clone(),
            Arc::clone(&pending_approvals),
            Arc::clone(&user_notifier),
            cwd.clone(),
            approval_policy_rx,
            Arc::clone(&pending_tool_calls),
        ));

        // Spawn persistent listener relay for inter-turn notifications
        tokio::spawn(Self::run_persistent_relay(
            persistent_rx,
            event_tx.clone(),
            Arc::clone(&pending_tool_calls),
        ));

        Ok(backend)
    }

    /// Background task to handle approval requests from the ACP connection.
    ///
    /// When `approval_policy` is `AskForApproval::Never` (yolo mode), requests
    /// are auto-approved without prompting the user.
    pub(super) async fn run_approval_handler(
        mut approval_rx: mpsc::Receiver<ApprovalRequest>,
        event_tx: mpsc::Sender<Event>,
        pending_approvals: Arc<Mutex<Vec<ApprovalRequest>>>,
        user_notifier: Arc<codex_core::UserNotifier>,
        cwd: PathBuf,
        approval_policy_rx: watch::Receiver<AskForApproval>,
        pending_tool_calls: Arc<Mutex<HashMap<String, AccumulatedToolCall>>>,
    ) {
        while let Some(request) = approval_rx.recv().await {
            // Store tool call metadata from the permission request so the
            // event translator can resolve proper titles when the subsequent
            // ToolCallUpdate(completed) arrives (often with empty fields from
            // Gemini agents).
            if let Some(ref metadata) = request.tool_call_metadata {
                let call_id = request.event.call_id().to_string();
                let cleaned_title = metadata
                    .title
                    .as_ref()
                    .map(|t| extract_command_from_permission_title(t));
                let new_entry = AccumulatedToolCall {
                    title: cleaned_title,
                    kind: metadata.kind,
                    raw_input: metadata.raw_input.clone(),
                    meta_tool_name: None,
                };
                let mut map = pending_tool_calls.lock().await;
                let entry = map.entry(call_id).or_insert_with(|| AccumulatedToolCall {
                    title: None,
                    kind: None,
                    raw_input: None,
                    meta_tool_name: None,
                });
                if new_entry.title.is_some() {
                    entry.title = new_entry.title;
                }
                if new_entry.kind.is_some() {
                    entry.kind = new_entry.kind;
                }
                if new_entry.raw_input.is_some() {
                    entry.raw_input = new_entry.raw_input;
                }
            }

            // Check current approval policy (may have changed via OverrideTurnContext)
            let current_policy = *approval_policy_rx.borrow();

            // If approval_policy is Never (yolo mode), auto-approve immediately
            if current_policy == AskForApproval::Never {
                debug!(
                    target: "acp_event_flow",
                    call_id = %request.event.call_id(),
                    "Auto-approving request (approval_policy=Never)"
                );
                let _ = request.response_tx.send(ReviewDecision::Approved);
                continue;
            }

            // Send the appropriate approval request event to TUI based on operation type.
            // Use the call_id as the event wrapper ID so that the TUI can
            // correctly route the user's decision back to this pending request.
            let (id, msg, command_for_notification) = match &request.event {
                ApprovalEventType::Exec(exec_event) => (
                    exec_event.call_id.clone(),
                    EventMsg::ExecApprovalRequest(exec_event.clone()),
                    exec_event.command.join(" "),
                ),
                ApprovalEventType::Patch(patch_event) => (
                    patch_event.call_id.clone(),
                    EventMsg::ApplyPatchApprovalRequest(patch_event.clone()),
                    format!(
                        "patch: {}",
                        patch_event
                            .changes
                            .keys()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ),
            };

            // Send the approval event to the TUI first, then notify.
            // Notification must come after event delivery because
            // notif.show() can block on some platforms (e.g. macOS),
            // which would prevent the TUI from ever receiving the event.
            let _ = event_tx
                .send(Event {
                    id: id.clone(),
                    msg,
                })
                .await;

            // Store the pending approval for later resolution
            pending_approvals.lock().await.push(request);

            // Send OS notification (non-blocking, but ordered after event delivery)
            user_notifier.notify(&codex_core::UserNotification::AwaitingApproval {
                call_id: id,
                command: command_for_notification,
                cwd: cwd.display().to_string(),
            });
        }
    }

    /// Background task that relays inter-turn notifications from the persistent
    /// listener channel to the TUI event stream.
    ///
    /// The persistent listener receives `SessionUpdate`s that arrive after
    /// `unregister_session` has been called (i.e. between prompt turns). Without
    /// this relay, those updates would be silently dropped.
    pub(super) async fn run_persistent_relay(
        mut persistent_rx: mpsc::Receiver<acp::SessionUpdate>,
        event_tx: mpsc::Sender<Event>,
        pending_tool_calls: Arc<Mutex<HashMap<String, AccumulatedToolCall>>>,
    ) {
        let mut pending_patch_changes = HashMap::new();
        while let Some(update) = persistent_rx.recv().await {
            let event_msgs = {
                let mut tool_calls = pending_tool_calls.lock().await;
                translate_session_update_to_events(
                    &update,
                    &mut pending_patch_changes,
                    &mut tool_calls,
                )
            };
            for msg in event_msgs {
                let _ = event_tx
                    .send(Event {
                        id: String::new(),
                        msg,
                    })
                    .await;
            }
        }
    }
}
