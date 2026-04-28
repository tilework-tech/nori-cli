use super::*;

impl AcpBackend {
    /// Resume a previous ACP session.
    ///
    /// If the agent supports `session/load` (via capabilities) and an
    /// `acp_session_id` is provided, the existing server-side resume path is
    /// used. Otherwise a client-side replay fallback is used: a fresh session
    /// is created via `session/new`, normalized replay entries are derived from
    /// the transcript, and a summary is stored in `pending_compact_summary` so
    /// it gets prepended to the first prompt.
    pub async fn resume_session(
        config: &AcpBackendConfig,
        acp_session_id: Option<&str>,
        transcript: Option<&crate::transcript::Transcript>,
        backend_event_tx: mpsc::Sender<BackendEvent>,
    ) -> Result<Self> {
        let (event_tx, event_rx) = mpsc::channel(32);
        tokio::spawn(forward_control_events(event_rx, backend_event_tx.clone()));
        let agent_config = get_agent_config(&config.agent)?;
        let cwd = config.cwd.clone();

        debug!(
            "Resuming ACP session (acp_session_id={:?}) for agent: {}",
            acp_session_id, config.agent
        );

        let mut connection = SacpConnection::spawn(&agent_config, &cwd)
            .await
            .map_err(|e| {
                let error_string = format!("{e:?}");
                let category = categorize_acp_error(&error_string);
                let display_error = format!("{e}");
                anyhow::anyhow!(enhanced_error_message(
                    category,
                    &display_error,
                    &agent_config.provider_info.name,
                    &agent_config.auth_hint,
                    &agent_config.display_name,
                    &agent_config.install_hint,
                ))
            })?;

        let supports_load_session = connection.capabilities().load_session;

        // Either load the session server-side or create a fresh session for
        // client-side replay.
        //
        // If server-side load_session fails at runtime, we fall back to
        // client-side replay rather than propagating the error. This ensures
        // /resume works even when the agent's load_session is broken.
        // The sixth tuple element carries buffered replay events from
        // server-side session/load.  We must NOT spawn a relay task for
        // these events until *after* resume_session has finished sending
        // its own events (SessionConfigured, Warning, etc.) to event_tx,
        // because the relay can fill the bounded channel and block
        // resume_session from sending.
        let (
            session_id,
            pending_summary,
            is_first_prompt_val,
            used_fallback,
            deferred_replay_client_events,
            event_rx,
            session_driver_state,
        ) = if let Some(sid) = acp_session_id.filter(|_| supports_load_session) {
            debug!("Agent supports session/load — using server-side resume");

            // Take the notification receiver so we can collect replay events
            // during session/load. With the unified channel, load replay
            // events flow through the same notification_tx as all other updates.
            let event_rx = connection.take_event_receiver();

            // Collect replay events into a buffer. The collector runs until
            // load_session() finishes and signals completion via the oneshot.
            let (load_done_tx, load_done_rx) = tokio::sync::oneshot::channel::<()>();
            let load_request_id = uuid::Uuid::new_v4().to_string();
            let collect_handle = tokio::spawn(async move {
                let mut event_rx = event_rx;
                let mut session_driver = session_runtime_driver::SessionDriver::new();
                let mut buffered_events = client_events_to_replay_client_events(
                    session_driver
                        .apply(session_reducer::InboundEvent::LoadSubmit {
                            request_id: load_request_id,
                        })
                        .events,
                );
                let mut done = std::pin::pin!(load_done_rx);
                loop {
                    tokio::select! {
                        biased;
                        maybe_event = event_rx.recv() => {
                            match maybe_event {
                                Some(crate::connection::ConnectionEvent::SessionUpdate(update)) => {
                                    buffered_events.extend(client_events_to_replay_client_events(
                                        session_driver
                                            .apply(session_reducer::InboundEvent::Notification(Box::new(update)))
                                            .events,
                                    ));
                                }
                                Some(crate::connection::ConnectionEvent::ApprovalRequest(_)) => {}
                                None => break,
                            }
                        }
                        _ = &mut done => {
                            // Drain any remaining buffered updates after load completes
                            while let Ok(event) = event_rx.try_recv() {
                                if let crate::connection::ConnectionEvent::SessionUpdate(update) = event {
                                    buffered_events.extend(client_events_to_replay_client_events(
                                        session_driver
                                            .apply(session_reducer::InboundEvent::Notification(Box::new(update)))
                                            .events,
                                    ));
                                }
                            }
                            buffered_events.extend(client_events_to_replay_client_events(
                                session_driver
                                    .apply(session_reducer::InboundEvent::LoadResponse)
                                    .events,
                            ));
                            break;
                        }
                    }
                }
                (session_driver, event_rx, buffered_events)
            });

            match connection.load_session(sid, &cwd).await {
                Ok(session_id) => {
                    // Signal the collector that load is done, then collect results.
                    let _ = load_done_tx.send(());
                    let (session_driver, recovered_rx, buffered_client_events) =
                        collect_handle.await.map_err(|err| {
                            anyhow::anyhow!("load session collector task panicked: {err}")
                        })?;
                    if !buffered_client_events.is_empty() {
                        debug!(
                            "ACP session/load produced {} replay client events (deferred until after setup)",
                            buffered_client_events.len()
                        );
                    }
                    debug!("ACP session resumed via session/load: {sid}");
                    (
                        session_id,
                        None,
                        false,
                        None,
                        buffered_client_events,
                        recovered_rx,
                        session_driver,
                    )
                }
                Err(e) => {
                    warn!(
                        "Server-side session/load failed, falling back to client-side replay: {e}"
                    );
                    let _ = load_done_tx.send(());
                    let (_, recovered_rx, _) = collect_handle.await.map_err(|err| {
                        anyhow::anyhow!("load session collector task panicked: {err}")
                    })?;

                    let mcp_servers =
                        crate::connection::mcp::to_sacp_mcp_servers(&config.mcp_servers);
                    let session_id =
                        connection
                            .create_session(&cwd, mcp_servers)
                            .await
                            .map_err(|e| {
                                let error_string = format!("{e:?}");
                                let category = categorize_acp_error(&error_string);
                                let display_error = format!("{e}");
                                anyhow::anyhow!(enhanced_error_message(
                                    category,
                                    &display_error,
                                    &agent_config.provider_info.name,
                                    &agent_config.auth_hint,
                                    &agent_config.display_name,
                                    &agent_config.install_hint,
                                ))
                            })?;

                    let (replay_events, summary) = if let Some(t) = transcript {
                        let client_events = transcript_to_replay_client_events(t);
                        let summary_text = transcript_to_summary(t);
                        let summary_opt = if summary_text.is_empty() {
                            None
                        } else {
                            Some(summary_text)
                        };
                        (client_events, summary_opt)
                    } else {
                        (Vec::new(), None)
                    };

                    (
                        session_id,
                        summary,
                        true,
                        Some(e.to_string()),
                        replay_events,
                        recovered_rx,
                        session_runtime_driver::SessionDriver::new(),
                    )
                }
            }
        } else {
            debug!("Agent does not support session/load — using client-side replay");

            let mcp_servers = crate::connection::mcp::to_sacp_mcp_servers(&config.mcp_servers);
            let session_id = connection
                .create_session(&cwd, mcp_servers)
                .await
                .map_err(|e| {
                    let error_string = format!("{e:?}");
                    let category = categorize_acp_error(&error_string);
                    let display_error = format!("{e}");
                    anyhow::anyhow!(enhanced_error_message(
                        category,
                        &display_error,
                        &agent_config.provider_info.name,
                        &agent_config.auth_hint,
                        &agent_config.display_name,
                        &agent_config.install_hint,
                    ))
                })?;

            let (replay_events, summary) = if let Some(t) = transcript {
                let client_events = transcript_to_replay_client_events(t);
                let summary_text = transcript_to_summary(t);
                let summary_opt = if summary_text.is_empty() {
                    None
                } else {
                    Some(summary_text)
                };
                (client_events, summary_opt)
            } else {
                (Vec::new(), None)
            };

            let event_rx = connection.take_event_receiver();
            (
                session_id,
                summary,
                true,
                None,
                replay_events,
                event_rx,
                session_runtime_driver::SessionDriver::new(),
            )
        };

        let connection = Arc::new(connection);
        let pending_approvals = Arc::new(Mutex::new(Vec::new()));
        let session_driver = Arc::new(Mutex::new(session_driver_state));
        let (session_event_tx, mut session_event_rx) = mpsc::channel(128);
        let (prompt_result_tx, prompt_result_rx) = mpsc::channel(128);
        let use_native_notifications =
            config.os_notifications == crate::config::OsNotifications::Enabled;
        let user_notifier = Arc::new(codex_core::UserNotifier::new(
            config.notify.clone(),
            use_native_notifications,
        ));
        let idle_timer_abort = Arc::new(Mutex::new(None));
        let (approval_policy_tx, approval_policy_rx) = watch::channel(config.approval_policy);
        let (history_log_id, history_entry_count) =
            crate::message_history::history_metadata(&config.nori_home).await;

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
        let conversation_id = transcript_recorder
            .as_ref()
            .and_then(|recorder| ConversationId::from_string(recorder.session_id()).ok())
            .unwrap_or_default();

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
            pending_compact_summary: Arc::new(Mutex::new(pending_summary)),
            pending_hook_context: Arc::new(Mutex::new(config.session_context.clone())),
            transcript_recorder,
            session_event_tx: session_event_tx.clone(),
            prompt_result_tx: prompt_result_tx.clone(),
            notify_after_idle: config.notify_after_idle,
            ghost_snapshots: Arc::new(GhostSnapshotStack::new()),
            is_first_prompt: Arc::new(Mutex::new(is_first_prompt_val)),
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

        if let Some(ref fallback_error) = used_fallback {
            event_tx
                .send(Event {
                    id: String::new(),
                    msg: EventMsg::Warning(WarningEvent {
                        message: format!(
                            "Server-side session restore failed ({fallback_error}). \
                             Falling back to transcript replay. The restored session \
                             will not have tool call information in the context."
                        ),
                    }),
                })
                .await
                .ok();
        }

        tokio::spawn(Self::run_connection_event_relay(
            backend.clone(),
            event_rx,
            prompt_result_rx,
            approval_policy_rx,
        ));

        if !deferred_replay_client_events.is_empty() {
            let backend_event_tx = backend.backend_event_tx.clone();
            tokio::spawn(async move {
                for client_event in deferred_replay_client_events {
                    let _ = backend_event_tx
                        .send(BackendEvent::Client(client_event))
                        .await;
                }
            });
        }

        Ok(backend)
    }
}
