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
            notification_rx,
        ) = if let Some(sid) = acp_session_id.filter(|_| supports_load_session) {
            debug!("Agent supports session/load — using server-side resume");

            // Take the notification receiver so we can collect replay events
            // during session/load. With the unified channel, load replay
            // events flow through the same notification_tx as all other updates.
            let notification_rx = connection.take_notification_receiver();

            // Collect replay events into a buffer. The collector runs until
            // load_session() finishes and signals completion via the oneshot.
            let (load_done_tx, load_done_rx) = tokio::sync::oneshot::channel::<()>();
            let collect_handle = tokio::spawn(async move {
                let mut notification_rx = notification_rx;
                let mut client_event_normalizer = nori_protocol::ClientEventNormalizer::default();
                let mut buffered_events = Vec::new();
                let mut done = std::pin::pin!(load_done_rx);
                loop {
                    tokio::select! {
                        biased;
                        update = notification_rx.recv() => {
                            match update {
                                Some(update) => {
                                    buffered_events.extend(
                                        client_event_normalizer.push_session_update(&update),
                                    );
                                }
                                None => break,
                            }
                        }
                        _ = &mut done => {
                            // Drain any remaining buffered updates after load completes
                            while let Ok(update) = notification_rx.try_recv() {
                                buffered_events.extend(
                                    client_event_normalizer.push_session_update(&update),
                                );
                            }
                            break;
                        }
                    }
                }
                (
                    client_events_to_replay_client_events(buffered_events),
                    notification_rx,
                )
            });

            match connection.load_session(sid, &cwd).await {
                Ok(session_id) => {
                    // Signal the collector that load is done, then collect results.
                    let _ = load_done_tx.send(());
                    let (buffered_client_events, recovered_rx) = collect_handle
                        .await
                        .expect("load session collector task panicked");
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
                    )
                }
                Err(e) => {
                    warn!(
                        "Server-side session/load failed, falling back to client-side replay: {e}"
                    );
                    let _ = load_done_tx.send(());
                    let (_, recovered_rx) = collect_handle
                        .await
                        .expect("load session collector task panicked");

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

            let notification_rx = connection.take_notification_receiver();
            (
                session_id,
                summary,
                true,
                None,
                replay_events,
                notification_rx,
            )
        };

        let approval_rx = connection.take_approval_receiver();
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
        let (approval_policy_tx, approval_policy_rx) = watch::channel(config.approval_policy);
        let conversation_id = ConversationId::new();
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
            pending_hook_context: Arc::new(Mutex::new(None)),
            transcript_recorder,
            notify_after_idle: config.notify_after_idle,
            ghost_snapshots: Arc::new(GhostSnapshotStack::new()),
            is_first_prompt: Arc::new(Mutex::new(is_first_prompt_val)),
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
            client_event_normalizer: Arc::clone(&client_event_normalizer),
            mcp_servers: config.mcp_servers.clone(),
            turn_interrupted: Arc::new(AtomicBool::new(false)),
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

        // Spawn notification relay for inter-turn notifications
        tokio::spawn(Self::run_notification_relay(
            notification_rx,
            Arc::clone(&client_event_normalizer),
            backend_event_tx.clone(),
            backend.transcript_recorder.clone(),
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
