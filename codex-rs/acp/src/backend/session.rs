use super::*;

impl AcpBackend {
    /// Resume a previous ACP session.
    ///
    /// If the agent supports `session/load` (via capabilities) and an
    /// `acp_session_id` is provided, the existing server-side resume path is
    /// used. Otherwise a client-side replay fallback is used: a fresh session
    /// is created via `session/new`, the transcript is converted into
    /// `initial_messages` for TUI display, and a summary is stored in
    /// `pending_compact_summary` so it gets prepended to the first prompt.
    pub async fn resume_session(
        config: &AcpBackendConfig,
        acp_session_id: Option<&str>,
        transcript: Option<&crate::transcript::Transcript>,
        event_tx: mpsc::Sender<Event>,
    ) -> Result<Self> {
        let agent_config = get_agent_config(&config.agent)?;
        let cwd = config.cwd.clone();

        debug!(
            "Resuming ACP session (acp_session_id={:?}) for agent: {}",
            acp_session_id, config.agent
        );

        let mut connection = AcpConnection::spawn(&agent_config, &cwd)
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
            initial_messages,
            pending_summary,
            is_first_prompt_val,
            used_fallback,
            deferred_replay_events,
        ) = if let Some(sid) = acp_session_id.filter(|_| supports_load_session) {
            debug!("Agent supports session/load — using server-side resume");

            let (update_tx, mut update_rx) = mpsc::channel::<acp::SessionUpdate>(256);

            // Collect replay events into a buffer instead of sending them
            // directly to event_tx. The event_tx consumer only starts after
            // resume_session returns, so sending directly would deadlock
            // when the number of events exceeds the channel capacity.
            let collect_handle = tokio::spawn(async move {
                let mut pending_patch_changes = std::collections::HashMap::new();
                let mut pending_tool_calls = std::collections::HashMap::new();
                let mut buffered_events = Vec::new();
                while let Some(update) = update_rx.recv().await {
                    let event_msgs = translate_session_update_to_events(
                        &update,
                        &mut pending_patch_changes,
                        &mut pending_tool_calls,
                    );
                    for msg in event_msgs {
                        buffered_events.push(Event {
                            id: String::new(),
                            msg,
                        });
                    }
                }
                buffered_events
            });

            match connection.load_session(sid, &cwd, update_tx).await {
                Ok(session_id) => {
                    // Wait for all updates to be collected. This is safe
                    // because the collect task buffers into a Vec (no
                    // backpressure) and update_rx closes when load_session
                    // completes (the worker thread drops update_tx).
                    let buffered_events = collect_handle.await.unwrap_or_default();
                    if !buffered_events.is_empty() {
                        debug!(
                            "ACP session/load produced {} replay events (deferred until after setup)",
                            buffered_events.len()
                        );
                    }
                    debug!("ACP session resumed via session/load: {sid}");
                    (session_id, None, None, false, None, buffered_events)
                }
                Err(e) => {
                    warn!(
                        "Server-side session/load failed, falling back to client-side replay: {e}"
                    );
                    collect_handle.abort();

                    let session_id = connection.create_session(&cwd).await.map_err(|e| {
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
                        let events = transcript_to_replay_events(t);
                        let summary_text = transcript_to_summary(t);
                        let summary_opt = if summary_text.is_empty() {
                            None
                        } else {
                            Some(summary_text)
                        };
                        (Some(events), summary_opt)
                    } else {
                        (None, None)
                    };

                    (
                        session_id,
                        replay_events,
                        summary,
                        true,
                        Some(e.to_string()),
                        Vec::new(),
                    )
                }
            }
        } else {
            debug!("Agent does not support session/load — using client-side replay");

            let session_id = connection.create_session(&cwd).await.map_err(|e| {
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
                let events = transcript_to_replay_events(t);
                let summary_text = transcript_to_summary(t);
                let summary_opt = if summary_text.is_empty() {
                    None
                } else {
                    Some(summary_text)
                };
                (Some(events), summary_opt)
            } else {
                (None, None)
            };

            (session_id, replay_events, summary, true, None, Vec::new())
        };

        let approval_rx = connection.take_approval_receiver();
        let persistent_rx = connection.take_persistent_receiver();
        let connection = Arc::new(connection);
        let pending_approvals = Arc::new(Mutex::new(Vec::new()));
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
            initial_messages,
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
            event_tx.clone(),
            Arc::clone(&pending_approvals),
            Arc::clone(&user_notifier),
            cwd.clone(),
            approval_policy_rx,
        ));

        // Spawn persistent listener relay for inter-turn notifications
        tokio::spawn(Self::run_persistent_relay(persistent_rx, event_tx.clone()));

        // Spawn the replay relay *after* all setup events (SessionConfigured,
        // Warning, etc.) have been sent.  Spawning it earlier causes a
        // deadlock: the relay fills the bounded event_tx channel, blocking
        // resume_session from sending its own events while nobody is
        // consuming from event_rx yet.
        if !deferred_replay_events.is_empty() {
            tokio::spawn(async move {
                for event in deferred_replay_events {
                    let _ = event_tx.send(event).await;
                }
            });
        }

        Ok(backend)
    }
}
