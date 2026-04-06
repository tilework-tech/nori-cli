use sacp::schema as acp;

use super::*;

/// Hook configuration for the reducer loop.
///
/// Groups all lifecycle hook paths and the timeout so they can be passed
/// as a single parameter to `run_reducer_loop`.
#[derive(Debug, Clone, Default)]
pub struct ReducerHookConfig {
    pub pre_agent_response_hooks: Vec<PathBuf>,
    pub async_pre_agent_response_hooks: Vec<PathBuf>,
    pub pre_tool_call_hooks: Vec<PathBuf>,
    pub async_pre_tool_call_hooks: Vec<PathBuf>,
    pub post_tool_call_hooks: Vec<PathBuf>,
    pub async_post_tool_call_hooks: Vec<PathBuf>,
    pub post_agent_response_hooks: Vec<PathBuf>,
    pub async_post_agent_response_hooks: Vec<PathBuf>,
    pub script_timeout: std::time::Duration,
}

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
        let pending_tool_calls = Arc::new(Mutex::new(HashMap::new()));
        let client_event_normalizer = Arc::new(Mutex::new(ClientEventNormalizer::default()));
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

        // Create the reducer channel (before struct so reducer_tx can be stored).
        let (reducer_tx, reducer_rx) = mpsc::channel::<session_reducer::InboundEvent>(256);
        let session_runtime = Arc::new(Mutex::new(
            nori_protocol::session_runtime::SessionRuntime::new(),
        ));

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
            async_session_end_hooks: config.async_session_end_hooks.clone(),
            async_pre_user_prompt_hooks: config.async_pre_user_prompt_hooks.clone(),
            async_post_user_prompt_hooks: config.async_post_user_prompt_hooks.clone(),
            script_timeout: config.script_timeout,
            mcp_servers: config.mcp_servers.clone(),
            turn_interrupted: Arc::new(AtomicBool::new(false)),
            reducer_tx: reducer_tx.clone(),
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
            backend_event_tx.clone(),
            Arc::clone(&pending_approvals),
            Arc::clone(&user_notifier),
            cwd.clone(),
            approval_policy_rx,
            Arc::clone(&pending_tool_calls),
            Arc::clone(&client_event_normalizer),
            backend.transcript_recorder.clone(),
        ));

        // Bridge: read notifications from the ACP connection and wrap them
        // as InboundEvent::Notification for the reducer.
        let bridge_tx = reducer_tx.clone();
        tokio::spawn(async move {
            let mut notification_rx = notification_rx;
            while let Some(update) = notification_rx.recv().await {
                let _ = bridge_tx
                    .send(session_reducer::InboundEvent::Notification(Box::new(
                        update,
                    )))
                    .await;
            }
        });

        // Spawn the reducer loop.
        let reducer_hook_config = ReducerHookConfig {
            pre_agent_response_hooks: config.pre_agent_response_hooks.clone(),
            async_pre_agent_response_hooks: config.async_pre_agent_response_hooks.clone(),
            pre_tool_call_hooks: config.pre_tool_call_hooks.clone(),
            async_pre_tool_call_hooks: config.async_pre_tool_call_hooks.clone(),
            post_tool_call_hooks: config.post_tool_call_hooks.clone(),
            async_post_tool_call_hooks: config.async_post_tool_call_hooks.clone(),
            post_agent_response_hooks: config.post_agent_response_hooks.clone(),
            async_post_agent_response_hooks: config.async_post_agent_response_hooks.clone(),
            script_timeout: config.script_timeout,
        };
        tokio::spawn(Self::run_reducer_loop(
            reducer_rx,
            Arc::clone(&client_event_normalizer),
            backend_event_tx,
            backend.transcript_recorder.clone(),
            Arc::clone(&session_runtime),
            Some(Arc::clone(&backend.connection)),
            reducer_tx.clone(),
            event_tx.clone(),
            reducer_hook_config,
        ));

        Ok(backend)
    }

    /// Background task to handle approval requests from the ACP connection.
    ///
    /// When `approval_policy` is `AskForApproval::Never` (yolo mode), requests
    /// are auto-approved without prompting the user.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn run_approval_handler(
        mut approval_rx: mpsc::Receiver<ApprovalRequest>,
        backend_event_tx: mpsc::Sender<BackendEvent>,
        pending_approvals: Arc<Mutex<Vec<ApprovalRequest>>>,
        user_notifier: Arc<codex_core::UserNotifier>,
        cwd: PathBuf,
        approval_policy_rx: watch::Receiver<AskForApproval>,
        pending_tool_calls: Arc<Mutex<HashMap<String, AccumulatedToolCall>>>,
        client_event_normalizer: Arc<Mutex<ClientEventNormalizer>>,
        transcript_recorder: Option<Arc<TranscriptRecorder>>,
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
                };
                let mut map = pending_tool_calls.lock().await;
                let entry = map.entry(call_id).or_insert_with(|| AccumulatedToolCall {
                    title: None,
                    kind: None,
                    raw_input: None,
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

            let client_events =
                normalize_permission_request(&client_event_normalizer, &request).await;
            forward_client_events(&backend_event_tx, &client_events).await;
            if let Some(ref recorder) = transcript_recorder {
                for client_event in &client_events {
                    if let Err(e) = recorder.record_client_event(client_event).await {
                        warn!("Failed to record normalized approval event to transcript: {e}");
                    }
                }
            }

            // Send the appropriate approval request event to TUI based on operation type.
            // Use the call_id as the event wrapper ID so that the TUI can
            // correctly route the user's decision back to this pending request.
            let (id, command_for_notification) = match &request.event {
                ApprovalEventType::Exec(exec_event) => {
                    (exec_event.call_id.clone(), exec_event.command.join(" "))
                }
                ApprovalEventType::Patch(patch_event) => (
                    patch_event.call_id.clone(),
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

    /// Background task that processes ALL inbound events through the
    /// serialized reducer and forwards produced `ClientEvent`s to the TUI.
    ///
    /// This replaces the old `run_notification_relay`. It receives
    /// `InboundEvent`s (notifications bridged from the ACP connection,
    /// plus PromptSubmit / PromptResponse / CancelSubmit from user-facing
    /// code paths) and drives the `SessionRuntime` state machine.
    ///
    /// Lifecycle hooks (pre_agent_response, pre_tool_call, post_tool_call,
    /// post_agent_response) are executed in the loop based on the inbound
    /// event type, BEFORE the event is passed to the reducer.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn run_reducer_loop(
        mut reducer_rx: mpsc::Receiver<session_reducer::InboundEvent>,
        client_event_normalizer: Arc<Mutex<ClientEventNormalizer>>,
        backend_event_tx: mpsc::Sender<BackendEvent>,
        transcript_recorder: Option<Arc<TranscriptRecorder>>,
        session_runtime: Arc<Mutex<nori_protocol::session_runtime::SessionRuntime>>,
        _connection: Option<Arc<SacpConnection>>,
        _reducer_tx: mpsc::Sender<session_reducer::InboundEvent>,
        event_tx: mpsc::Sender<Event>,
        hook_config: ReducerHookConfig,
    ) {
        // Track whether pre_agent_response has fired for the current prompt.
        // Reset on PromptSubmit.
        let mut has_fired_pre_agent_response = false;

        while let Some(event) = reducer_rx.recv().await {
            // Fire hooks BEFORE the reducer processes the event.
            Self::fire_hooks_for_event(
                &event,
                &event_tx,
                &hook_config,
                &mut has_fired_pre_agent_response,
            )
            .await;

            let output = {
                let mut runtime = session_runtime.lock().await;
                let mut normalizer = client_event_normalizer.lock().await;
                session_reducer::reduce(&mut runtime, event, &mut normalizer)
            };

            // Forward produced ClientEvents to the TUI.
            forward_client_events(&backend_event_tx, &output.events).await;

            // Fire post_agent_response hooks after a Completed event.
            // The reducer has already assembled the last_agent_message.
            Self::fire_post_agent_response_if_completed(&output.events, &event_tx, &hook_config)
                .await;

            // Record to transcript in background to avoid blocking the loop.
            if let Some(ref recorder) = transcript_recorder {
                let recorder = Arc::clone(recorder);
                let events = output.events.clone();
                tokio::spawn(async move {
                    for ev in &events {
                        if let Err(e) = recorder.record_client_event(ev).await {
                            warn!("Failed to record client event to transcript: {e}");
                        }
                    }
                });
            }

            // Side effects are currently handled externally:
            // - SendPrompt: user_input.rs drives the prompt lifecycle
            // - SendCancel: submit_and_ops.rs handles Op::Interrupt
            // - SendLoad: session.rs drives the resume path
            // - ResolvePermissionCancelled: approval handler resolves permissions
            //
            // As the reducer takes over more of the lifecycle, side effect
            // execution will move here.
            for side_effect in &output.side_effects {
                debug!(
                    target: "acp_reducer",
                    "Reducer side effect (not yet executed here): {side_effect:?}"
                );
            }
        }
    }

    /// Fire lifecycle hooks based on the inbound event type.
    ///
    /// This runs BEFORE the reducer processes the event so that hooks
    /// execute at the correct point in the lifecycle.
    async fn fire_hooks_for_event(
        event: &session_reducer::InboundEvent,
        event_tx: &mpsc::Sender<Event>,
        hook_config: &ReducerHookConfig,
        has_fired_pre_agent_response: &mut bool,
    ) {
        match event {
            session_reducer::InboundEvent::PromptSubmit(..) => {
                // Reset the per-prompt flag so pre_agent_response can fire
                // again for the new turn.
                *has_fired_pre_agent_response = false;
            }
            session_reducer::InboundEvent::Notification(update) => {
                Self::fire_notification_hooks(
                    update,
                    event_tx,
                    hook_config,
                    has_fired_pre_agent_response,
                )
                .await;
            }
            session_reducer::InboundEvent::PromptResponse { .. } => {
                // post_agent_response hooks are fired AFTER reduce, via
                // fire_post_agent_response_if_completed, because the reducer
                // needs to finalize the agent message first.
            }
            _ => {}
        }
    }

    /// Fire hooks for a specific ACP session update notification.
    async fn fire_notification_hooks(
        update: &acp::SessionUpdate,
        event_tx: &mpsc::Sender<Event>,
        hook_config: &ReducerHookConfig,
        has_fired_pre_agent_response: &mut bool,
    ) {
        match update {
            acp::SessionUpdate::AgentMessageChunk(chunk) => {
                // Fire pre_agent_response on the first chunk with non-empty text.
                if !*has_fired_pre_agent_response {
                    let has_text = matches!(
                        &chunk.content,
                        acp::ContentBlock::Text(t) if !t.text.is_empty()
                    );
                    if has_text {
                        *has_fired_pre_agent_response = true;

                        if !hook_config.pre_agent_response_hooks.is_empty() {
                            let env_vars = HashMap::from([(
                                "NORI_HOOK_EVENT".to_string(),
                                "pre_agent_response".to_string(),
                            )]);
                            let results = crate::hooks::execute_hooks_with_env(
                                &hook_config.pre_agent_response_hooks,
                                hook_config.script_timeout,
                                &env_vars,
                            )
                            .await;
                            route_hook_results(&results, event_tx, "", None).await;
                        }

                        if !hook_config.async_pre_agent_response_hooks.is_empty() {
                            let env_vars = HashMap::from([(
                                "NORI_HOOK_EVENT".to_string(),
                                "pre_agent_response".to_string(),
                            )]);
                            let _ = crate::hooks::execute_hooks_fire_and_forget(
                                hook_config.async_pre_agent_response_hooks.clone(),
                                hook_config.script_timeout,
                                env_vars,
                            );
                        }
                    }
                }
            }
            acp::SessionUpdate::ToolCall(tool_call) => {
                if !hook_config.pre_tool_call_hooks.is_empty() {
                    let title = tool_call.title.clone();
                    let raw_input = tool_call
                        .raw_input
                        .as_ref()
                        .map(std::string::ToString::to_string)
                        .unwrap_or_default();
                    let env_vars = HashMap::from([
                        ("NORI_HOOK_EVENT".to_string(), "pre_tool_call".to_string()),
                        ("NORI_HOOK_TOOL_NAME".to_string(), title.clone()),
                        ("NORI_HOOK_TOOL_ARGS".to_string(), raw_input.clone()),
                    ]);
                    let results = crate::hooks::execute_hooks_with_env(
                        &hook_config.pre_tool_call_hooks,
                        hook_config.script_timeout,
                        &env_vars,
                    )
                    .await;
                    route_hook_results(&results, event_tx, "", None).await;
                }

                if !hook_config.async_pre_tool_call_hooks.is_empty() {
                    let title = tool_call.title.clone();
                    let raw_input = tool_call
                        .raw_input
                        .as_ref()
                        .map(std::string::ToString::to_string)
                        .unwrap_or_default();
                    let env_vars = HashMap::from([
                        ("NORI_HOOK_EVENT".to_string(), "pre_tool_call".to_string()),
                        ("NORI_HOOK_TOOL_NAME".to_string(), title),
                        ("NORI_HOOK_TOOL_ARGS".to_string(), raw_input),
                    ]);
                    let _ = crate::hooks::execute_hooks_fire_and_forget(
                        hook_config.async_pre_tool_call_hooks.clone(),
                        hook_config.script_timeout,
                        env_vars,
                    );
                }
            }
            acp::SessionUpdate::ToolCallUpdate(tool_update) => {
                let is_completed = tool_update
                    .fields
                    .status
                    .as_ref()
                    .map(|s| *s == acp::ToolCallStatus::Completed)
                    .unwrap_or(false);

                if is_completed {
                    let title = tool_update
                        .fields
                        .title
                        .clone()
                        .unwrap_or_else(|| "Tool".to_string());
                    let output =
                        crate::backend::tool_display::extract_tool_output(&tool_update.fields);

                    if !hook_config.post_tool_call_hooks.is_empty() {
                        let env_vars = HashMap::from([
                            ("NORI_HOOK_EVENT".to_string(), "post_tool_call".to_string()),
                            ("NORI_HOOK_TOOL_NAME".to_string(), title.clone()),
                            ("NORI_HOOK_TOOL_OUTPUT".to_string(), output.clone()),
                        ]);
                        let results = crate::hooks::execute_hooks_with_env(
                            &hook_config.post_tool_call_hooks,
                            hook_config.script_timeout,
                            &env_vars,
                        )
                        .await;
                        route_hook_results(&results, event_tx, "", None).await;
                    }

                    if !hook_config.async_post_tool_call_hooks.is_empty() {
                        let env_vars = HashMap::from([
                            ("NORI_HOOK_EVENT".to_string(), "post_tool_call".to_string()),
                            ("NORI_HOOK_TOOL_NAME".to_string(), title),
                            ("NORI_HOOK_TOOL_OUTPUT".to_string(), output),
                        ]);
                        let _ = crate::hooks::execute_hooks_fire_and_forget(
                            hook_config.async_post_tool_call_hooks.clone(),
                            hook_config.script_timeout,
                            env_vars,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    /// Fire post_agent_response hooks if the output contains a
    /// `TurnLifecycle::Completed` event with a non-empty agent message.
    ///
    /// Called AFTER reduce so the agent message text is available.
    async fn fire_post_agent_response_if_completed(
        events: &[nori_protocol::ClientEvent],
        event_tx: &mpsc::Sender<Event>,
        hook_config: &ReducerHookConfig,
    ) {
        if hook_config.post_agent_response_hooks.is_empty()
            && hook_config.async_post_agent_response_hooks.is_empty()
        {
            return;
        }

        for ev in events {
            if let nori_protocol::ClientEvent::TurnLifecycle(
                nori_protocol::TurnLifecycle::Completed {
                    last_agent_message, ..
                },
            ) = ev
            {
                let agent_text = last_agent_message.as_deref().unwrap_or("").to_string();
                if agent_text.is_empty() {
                    return;
                }

                if !hook_config.post_agent_response_hooks.is_empty() {
                    let env_vars = HashMap::from([
                        (
                            "NORI_HOOK_EVENT".to_string(),
                            "post_agent_response".to_string(),
                        ),
                        ("NORI_HOOK_AGENT_RESPONSE".to_string(), agent_text.clone()),
                    ]);
                    let results = crate::hooks::execute_hooks_with_env(
                        &hook_config.post_agent_response_hooks,
                        hook_config.script_timeout,
                        &env_vars,
                    )
                    .await;
                    route_hook_results(&results, event_tx, "", None).await;
                }

                if !hook_config.async_post_agent_response_hooks.is_empty() {
                    let env_vars = HashMap::from([
                        (
                            "NORI_HOOK_EVENT".to_string(),
                            "post_agent_response".to_string(),
                        ),
                        ("NORI_HOOK_AGENT_RESPONSE".to_string(), agent_text),
                    ]);
                    let _ = crate::hooks::execute_hooks_fire_and_forget(
                        hook_config.async_post_agent_response_hooks.clone(),
                        hook_config.script_timeout,
                        env_vars,
                    );
                }

                return;
            }
        }
    }
}
