use super::*;

/// Build the session's MCP server list (CLI-configured servers plus the
/// nori-client goal server when the agent advertises HTTP MCP) and commit
/// the registered server. Shared by every branch of `resume_session`.
async fn session_mcp_servers(
    config: &AcpBackendConfig,
    connection: &AcpConnection,
    thread_goal_state: &Arc<Mutex<thread_goal::ThreadGoalState>>,
    backend_event_tx: &mpsc::Sender<BackendEvent>,
    goal_mcp_connected: &Arc<std::sync::atomic::AtomicBool>,
    goal_mcp_http_server: &Arc<Mutex<Option<nori_client_mcp::NoriClientServer>>>,
) -> Result<Vec<acp::McpServer>> {
    let mut mcp_servers = crate::connection::mcp::to_acp_mcp_servers(
        &config.mcp_servers,
        config.mcp_oauth_credentials_store_mode,
    );
    let nori_client_server = nori_client_mcp::register_for_session(
        connection,
        &mut mcp_servers,
        Arc::clone(thread_goal_state),
        backend_event_tx.clone(),
        Arc::clone(goal_mcp_connected),
    )
    .await?;
    if let Some(server) = nori_client_server {
        server.commit(goal_mcp_http_server).await;
    }
    Ok(mcp_servers)
}

fn retarget_replay_event(
    mut event: nori_protocol::SessionEvent,
    session_id: &acp::SessionId,
) -> nori_protocol::SessionEvent {
    if let nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
        acp::AgentNotification::SessionNotification(notification),
    )) = &mut event
    {
        notification.session_id = session_id.clone();
    }
    event
}

struct ReplayBatch {
    source: nori_protocol::ReplaySource,
    events: Vec<nori_protocol::SessionEvent>,
}

impl ReplayBatch {
    fn new(
        source: nori_protocol::ReplaySource,
        events: Vec<nori_protocol::SessionEvent>,
    ) -> Option<Self> {
        if events.is_empty() {
            None
        } else {
            Some(Self { source, events })
        }
    }
}

fn drain_setup_session_events(
    event_rx: &mut mpsc::Receiver<crate::connection::ConnectionEvent>,
) -> Result<Vec<nori_protocol::SessionEvent>> {
    let mut events = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        match event {
            crate::connection::ConnectionEvent::Acp(event) => {
                events.push(nori_protocol::SessionEvent::Acp(*event));
            }
            // The exact notification is the preceding ACP event. The private
            // reducer projection starts after setup completes.
            crate::connection::ConnectionEvent::SessionUpdate(_) => {}
            crate::connection::ConnectionEvent::DelegatedRequest(request) => {
                let _ =
                    request
                        .response_tx
                        .send(Ok(acp::ClientResponse::RequestPermissionResponse(
                            acp::RequestPermissionResponse::new(
                                acp::RequestPermissionOutcome::Cancelled,
                            ),
                        )));
            }
            crate::connection::ConnectionEvent::SessionClosed => {
                anyhow::bail!("ACP session closed during setup");
            }
            crate::connection::ConnectionEvent::ChildExited {
                status,
                stderr_tail,
            } => {
                anyhow::bail!("ACP agent exited during setup (status: {status:?}): {stderr_tail}");
            }
        }
    }
    Ok(events)
}

async fn forward_setup_session_events(
    backend_event_tx: &mpsc::Sender<BackendEvent>,
    events: impl IntoIterator<Item = nori_protocol::SessionEvent>,
) -> Result<()> {
    for event in events {
        backend_event_tx
            .send(BackendEvent::Public(event))
            .await
            .map_err(|_| anyhow::anyhow!("session event receiver closed during setup"))?;
    }
    Ok(())
}

fn response_request_id(events: &[nori_protocol::SessionEvent]) -> Option<acp::RequestId> {
    events.iter().find_map(|event| match event {
        nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
            request_id, ..
        }) => Some(request_id.clone()),
        nori_protocol::SessionEvent::Acp(_) | nori_protocol::SessionEvent::Nori(_) => None,
    })
}

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
        let cwd = config.cwd.clone();

        debug!(
            "Resuming ACP session (acp_session_id={:?}) for agent: {}",
            acp_session_id, config.agent
        );

        let agent_config = get_agent_config(&config.agent)?;
        let mut connection = spawn_and_relay::spawn_connection_with_public_initialize(
            &agent_config,
            &cwd,
            config.acp_proxy.clone(),
            &backend_event_tx,
        )
        .await?;
        let mut event_rx = connection.take_event_receiver();

        let supports_load_session = connection.capabilities().load_session;
        let supports_session_resume = connection
            .capabilities()
            .session_capabilities
            .resume
            .is_some();
        let initial_goal_replay_events = transcript
            .map(transcript_to_replay_client_events)
            .unwrap_or_default();
        let thread_goal_state = Arc::new(Mutex::new(
            thread_goal::ThreadGoalState::from_replay_events(&initial_goal_replay_events),
        ));
        let goal_mcp_connected = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let goal_mcp_http_server = Arc::new(Mutex::new(None));

        // Either load the session server-side or create a fresh session for
        // client-side replay.
        //
        // If server-side load_session fails at runtime and a local transcript
        // is available, fall back to client-side replay. Without a transcript,
        // propagate the failure: creating an empty replacement session would
        // abandon the session the user asked to reattach to.
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
            deferred_setup_session_events,
            deferred_replay_client_events,
            deferred_replay_batches,
            event_rx,
            session_driver_state,
        ) = if let Some(sid) = acp_session_id.filter(|_| supports_load_session) {
            debug!("Agent supports session/load — using server-side resume");

            // Collect replay events into a buffer. The collector runs until
            // load_session() finishes and signals completion via the oneshot.
            let (load_done_tx, load_done_rx) = tokio::sync::oneshot::channel::<()>();
            let (load_started_tx, load_started_rx) = tokio::sync::oneshot::channel();
            let collect_handle = tokio::spawn(async move {
                let mut event_rx = event_rx;
                let mut session_driver = session_runtime_driver::SessionDriver::new();
                let mut buffered_session_events = Vec::new();
                let load_request_id = load_started_rx.await.map_err(|_| {
                    anyhow::anyhow!("session/load did not expose its wire request ID")
                })?;
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
                                Some(crate::connection::ConnectionEvent::Acp(event)) => {
                                    buffered_session_events.push(
                                        nori_protocol::SessionEvent::Acp(*event),
                                    );
                                }
                                Some(crate::connection::ConnectionEvent::SessionUpdate(update)) => {
                                    buffered_events.extend(client_events_to_replay_client_events(
                                        session_driver
                                            .apply(session_reducer::InboundEvent::Notification(Box::new(update)))
                                            .events,
                                    ));
                                }
                                Some(crate::connection::ConnectionEvent::SessionClosed) => break,
                                Some(crate::connection::ConnectionEvent::DelegatedRequest(request)) => {
                                    let _ = request.response_tx.send(Ok(
                                        acp::ClientResponse::RequestPermissionResponse(
                                            acp::RequestPermissionResponse::new(
                                                acp::RequestPermissionOutcome::Cancelled,
                                            ),
                                        ),
                                    ));
                                }
                                Some(crate::connection::ConnectionEvent::ChildExited { status, stderr_tail }) => {
                                    warn!(?status, %stderr_tail, "ACP agent exited during session/load");
                                    break;
                                }
                                None => break,
                            }
                        }
                        _ = &mut done => {
                            // Drain any remaining buffered updates after load completes
                            while let Ok(event) = event_rx.try_recv() {
                                match event {
                                    crate::connection::ConnectionEvent::Acp(event) => {
                                        buffered_session_events.push(
                                            nori_protocol::SessionEvent::Acp(*event),
                                        );
                                    }
                                    crate::connection::ConnectionEvent::SessionUpdate(update) => {
                                        buffered_events.extend(client_events_to_replay_client_events(
                                            session_driver
                                                .apply(session_reducer::InboundEvent::Notification(Box::new(update)))
                                                .events,
                                        ));
                                    }
                                    crate::connection::ConnectionEvent::SessionClosed => {}
                                    crate::connection::ConnectionEvent::DelegatedRequest(request) => {
                                        let _ = request.response_tx.send(Ok(
                                            acp::ClientResponse::RequestPermissionResponse(
                                                acp::RequestPermissionResponse::new(
                                                    acp::RequestPermissionOutcome::Cancelled,
                                                ),
                                            ),
                                        ));
                                    }
                                    crate::connection::ConnectionEvent::ChildExited { status, stderr_tail } => {
                                        warn!(?status, %stderr_tail, "ACP agent exited during session/load drain");
                                    }
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
                Ok::<_, anyhow::Error>((
                    session_driver,
                    event_rx,
                    buffered_events,
                    buffered_session_events,
                ))
            });

            let mcp_servers = session_mcp_servers(
                config,
                &connection,
                &thread_goal_state,
                &backend_event_tx,
                &goal_mcp_connected,
                &goal_mcp_http_server,
            )
            .await?;

            match connection
                .load_session(sid, &cwd, mcp_servers, Some(load_started_tx))
                .await
            {
                Ok(session_id) => {
                    // Signal the collector that load is done, then collect results.
                    let _ = load_done_tx.send(());
                    let (
                        session_driver,
                        recovered_rx,
                        buffered_client_events,
                        buffered_session_events,
                    ) = collect_handle.await.map_err(|err| {
                        anyhow::anyhow!("load session collector task panicked: {err}")
                    })??;
                    if !buffered_client_events.is_empty() {
                        debug!(
                            "ACP session/load produced {} replay client events (deferred until after setup)",
                            buffered_client_events.len()
                        );
                    }
                    let load_request_id = response_request_id(&buffered_session_events)
                        .ok_or_else(|| anyhow::anyhow!("session/load produced no raw response"))?;
                    let mut setup_session_events = vec![nori_protocol::SessionEvent::Nori(
                        nori_protocol::NoriEvent::SessionPhaseChanged(
                            nori_protocol::SessionPhase::Loading {
                                request_id: load_request_id,
                            },
                        ),
                    )];
                    let (replay_session_events, current_session_events): (Vec<_>, Vec<_>) =
                        buffered_session_events.into_iter().partition(|event| {
                            matches!(
                                event,
                                nori_protocol::SessionEvent::Acp(
                                    nori_protocol::AcpEvent::Notification(_)
                                )
                            )
                        });
                    setup_session_events.extend(current_session_events);
                    setup_session_events.push(nori_protocol::SessionEvent::Nori(
                        nori_protocol::NoriEvent::SessionPhaseChanged(
                            nori_protocol::SessionPhase::Idle,
                        ),
                    ));
                    debug!("ACP session resumed via session/load: {sid}");
                    (
                        session_id,
                        None,
                        false,
                        None,
                        setup_session_events,
                        buffered_client_events,
                        ReplayBatch::new(nori_protocol::ReplaySource::Agent, replay_session_events)
                            .into_iter()
                            .collect(),
                        recovered_rx,
                        session_driver,
                    )
                }
                Err(e) => {
                    warn!("Server-side session/load failed: {e}");
                    let _ = load_done_tx.send(());
                    let (_failed_driver, mut recovered_rx, _, buffered_session_events) =
                        collect_handle.await.map_err(|err| {
                            anyhow::anyhow!("load session collector task panicked: {err}")
                        })??;
                    let load_request_id = response_request_id(&buffered_session_events)
                        .ok_or_else(|| {
                            anyhow::anyhow!("failed session/load produced no raw response")
                        })?;
                    let mut setup_session_events = vec![nori_protocol::SessionEvent::Nori(
                        nori_protocol::NoriEvent::SessionPhaseChanged(
                            nori_protocol::SessionPhase::Loading {
                                request_id: load_request_id,
                            },
                        ),
                    )];
                    let (agent_replay_events, current_session_events): (Vec<_>, Vec<_>) =
                        buffered_session_events.into_iter().partition(|event| {
                            matches!(
                                event,
                                nori_protocol::SessionEvent::Acp(
                                    nori_protocol::AcpEvent::Notification(_)
                                )
                            )
                        });
                    setup_session_events.extend(current_session_events);
                    setup_session_events.push(nori_protocol::SessionEvent::Nori(
                        nori_protocol::NoriEvent::SessionPhaseChanged(
                            nori_protocol::SessionPhase::Idle,
                        ),
                    ));

                    let Some(transcript) = transcript else {
                        setup_session_events.extend(drain_setup_session_events(&mut recovered_rx)?);
                        forward_setup_session_events(&backend_event_tx, setup_session_events)
                            .await?;
                        return Err(enhance_agent_error(e, &agent_config));
                    };
                    warn!("Falling back to client-side transcript replay");

                    let mcp_servers = session_mcp_servers(
                        config,
                        &connection,
                        &thread_goal_state,
                        &backend_event_tx,
                        &goal_mcp_connected,
                        &goal_mcp_http_server,
                    )
                    .await?;
                    let session_id = match connection.create_session(&cwd, mcp_servers).await {
                        Ok(session_id) => session_id,
                        Err(error) => {
                            setup_session_events
                                .extend(drain_setup_session_events(&mut recovered_rx)?);
                            forward_setup_session_events(&backend_event_tx, setup_session_events)
                                .await?;
                            return Err(enhance_agent_error(error, &agent_config));
                        }
                    };

                    if let Some(ref default_model) = config.default_model {
                        session_defaults::apply_default_model(
                            &connection,
                            &session_id,
                            default_model,
                        )
                        .await;
                    }
                    setup_session_events.extend(drain_setup_session_events(&mut recovered_rx)?);

                    let replay_events = transcript_to_replay_client_events(transcript);
                    let replay_session_events = transcript_to_replay_session_events(transcript);
                    let summary_text = transcript_to_summary(transcript);
                    let summary = if summary_text.is_empty() {
                        None
                    } else {
                        Some(summary_text)
                    };
                    let mut replay_batches =
                        ReplayBatch::new(nori_protocol::ReplaySource::Agent, agent_replay_events)
                            .into_iter()
                            .collect::<Vec<_>>();
                    if let Some(transcript_batch) = ReplayBatch::new(
                        nori_protocol::ReplaySource::Transcript,
                        replay_session_events,
                    ) {
                        replay_batches.push(transcript_batch);
                    }

                    (
                        session_id,
                        summary,
                        true,
                        Some(e.to_string()),
                        setup_session_events,
                        replay_events,
                        replay_batches,
                        recovered_rx,
                        session_runtime_driver::SessionDriver::new(),
                    )
                }
            }
        } else if let Some(sid) = acp_session_id.filter(|_| supports_session_resume) {
            debug!("Agent supports session/resume — using live reattach");

            // Live reattach with no history replay (`session/resume`).
            // Failures propagate instead of falling back to `session/new`,
            // which would abandon the session the user selected.
            let mcp_servers = session_mcp_servers(
                config,
                &connection,
                &thread_goal_state,
                &backend_event_tx,
                &goal_mcp_connected,
                &goal_mcp_http_server,
            )
            .await?;

            let session_id = match connection.resume_session(sid, &cwd, mcp_servers).await {
                Ok(session_id) => session_id,
                Err(error) => {
                    let setup_events = drain_setup_session_events(&mut event_rx)?;
                    forward_setup_session_events(&backend_event_tx, setup_events).await?;
                    return Err(enhance_agent_error(error, &agent_config));
                }
            };
            debug!("ACP session resumed via session/resume: {sid}");
            let setup_session_events = drain_setup_session_events(&mut event_rx)?;

            (
                session_id,
                None,
                false,
                None,
                setup_session_events,
                Vec::new(),
                Vec::new(),
                event_rx,
                session_runtime_driver::SessionDriver::new(),
            )
        } else {
            debug!(
                "Agent supports neither session/load nor session/resume — using client-side replay"
            );

            let mcp_servers = session_mcp_servers(
                config,
                &connection,
                &thread_goal_state,
                &backend_event_tx,
                &goal_mcp_connected,
                &goal_mcp_http_server,
            )
            .await?;
            let session_id = match connection.create_session(&cwd, mcp_servers).await {
                Ok(session_id) => session_id,
                Err(error) => {
                    let setup_events = drain_setup_session_events(&mut event_rx)?;
                    forward_setup_session_events(&backend_event_tx, setup_events).await?;
                    return Err(enhance_agent_error(error, &agent_config));
                }
            };

            if let Some(ref default_model) = config.default_model {
                session_defaults::apply_default_model(&connection, &session_id, default_model)
                    .await;
            }
            let setup_session_events = drain_setup_session_events(&mut event_rx)?;

            let (replay_events, replay_session_events, summary) = if let Some(t) = transcript {
                let client_events = transcript_to_replay_client_events(t);
                let session_events = transcript_to_replay_session_events(t);
                let summary_text = transcript_to_summary(t);
                let summary_opt = if summary_text.is_empty() {
                    None
                } else {
                    Some(summary_text)
                };
                (client_events, session_events, summary_opt)
            } else {
                (Vec::new(), Vec::new(), None)
            };

            (
                session_id,
                summary,
                true,
                None,
                setup_session_events,
                replay_events,
                ReplayBatch::new(
                    nori_protocol::ReplaySource::Transcript,
                    replay_session_events,
                )
                .into_iter()
                .collect(),
                event_rx,
                session_runtime_driver::SessionDriver::new(),
            )
        };

        if !deferred_replay_client_events.is_empty() {
            let mut replay_events_for_goal_state = initial_goal_replay_events;
            replay_events_for_goal_state.extend(deferred_replay_client_events.iter().cloned());
            *thread_goal_state.lock().await =
                thread_goal::ThreadGoalState::from_replay_events(&replay_events_for_goal_state);
        }

        forward_setup_session_events(&backend_event_tx, deferred_setup_session_events).await?;

        let capabilities_update =
            nori_client_mcp::capabilities_update_for_session(&connection, &goal_mcp_connected);
        let connection = Arc::new(connection);
        let pending_approvals = Arc::new(Mutex::new(Vec::new()));
        let session_driver = Arc::new(Mutex::new(session_driver_state));
        let (session_event_tx, mut session_event_rx) = mpsc::channel(128);
        let (prompt_result_tx, prompt_result_rx) = mpsc::channel(128);
        let use_native_notifications =
            config.os_notifications == crate::config::OsNotifications::Enabled;
        let user_notifier = Arc::new(crate::UserNotifier::new(
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
        let transcript_id = transcript_recorder
            .as_ref()
            .map(|recorder| recorder.session_id().to_string());
        let transcript_path = transcript_recorder
            .as_ref()
            .map(|recorder| recorder.transcript_path().to_path_buf());
        let conversation_id = transcript_recorder
            .as_ref()
            .and_then(|recorder| ConversationId::from_string(recorder.session_id()).ok())
            .unwrap_or_default();
        let pending_hook_context = session_context_for_connection(config, &connection);
        let backend = Self {
            connection,
            session_id: Arc::new(RwLock::new(session_id)),
            backend_event_tx: backend_event_tx.clone(),
            cwd: cwd.clone(),
            pending_approvals: Arc::clone(&pending_approvals),
            pending_prompt_submissions: Arc::new(Mutex::new(HashMap::new())),
            user_notifier: Arc::clone(&user_notifier),
            idle_timer_abort: Arc::clone(&idle_timer_abort),
            nori_home: config.nori_home.clone(),
            history_persistence: config.history_persistence,
            acp_proxy: config.acp_proxy.clone(),
            conversation_id: Arc::new(RwLock::new(conversation_id)),
            approval_policy_tx,
            pending_compact_summary: Arc::new(Mutex::new(pending_summary)),
            thread_goal_state,
            goal_mcp_connected,
            goal_mcp_http_server,
            goal_ext_driving: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            pending_hook_context: Arc::new(Mutex::new(pending_hook_context)),
            transcript_recorder: Arc::new(RwLock::new(transcript_recorder)),
            session_event_tx: session_event_tx.clone(),
            prompt_result_tx: prompt_result_tx.clone(),
            prompt_phase_gate: Arc::new(Mutex::new(None)),
            notify_after_idle: config.notify_after_idle,
            ghost_snapshots: Arc::new(GhostSnapshotStack::new()),
            is_first_prompt: Arc::new(Mutex::new(is_first_prompt_val)),
            prompt_summary_enabled: config.prompt_summary_enabled,
            agent_name: config.agent.clone(),
            cli_version: config.cli_version.clone(),
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
            mcp_oauth_credentials_store_mode: config.mcp_oauth_credentials_store_mode,
            is_shutting_down: Arc::new(AtomicBool::new(false)),
            prompt_task_abort: Arc::new(Mutex::new(None)),
            cancel_timeout_abort: Arc::new(Mutex::new(None)),
            runtime_task_abort: Arc::new(Mutex::new(None)),
            relay_task_abort: Arc::new(Mutex::new(None)),
        };

        let runtime_backend = backend.clone();
        let runtime_task = tokio::spawn(async move {
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
        *backend.runtime_task_abort.lock().await = Some(runtime_task.abort_handle());

        // Execute session_start hooks
        run_session_start_hooks(
            &config.session_start_hooks,
            config.script_timeout,
            &backend_event_tx,
            Some(&backend.pending_hook_context),
        )
        .await;

        // Fire-and-forget async session start hooks
        let _ = crate::hooks::execute_hooks_fire_and_forget(
            config.async_session_start_hooks.clone(),
            config.script_timeout,
            HashMap::new(),
        );

        backend_event_tx
            .send(BackendEvent::Public(SessionEvent::Nori(
                nori_protocol::NoriEvent::SessionStarted(nori_protocol::SessionStarted {
                    transcript_id,
                    acp_session_id: backend.session_id().await,
                    cwd: cwd.clone(),
                    transcript_path,
                    history_log_id: i64::try_from(history_log_id).unwrap_or(i64::MAX),
                    history_entry_count: i64::try_from(history_entry_count).unwrap_or(i64::MAX),
                }),
            )))
            .await
            .ok();

        backend_event_tx
            .send(BackendEvent::Public(SessionEvent::Nori(
                nori_protocol::NoriEvent::CapabilitiesChanged(public_nori_capabilities(
                    capabilities_update,
                )),
            )))
            .await
            .ok();

        if let Some(ref fallback_error) = used_fallback {
            backend_event_tx
                .send(BackendEvent::Public(SessionEvent::Nori(
                    nori_protocol::NoriEvent::Notice(nori_protocol::Notice {
                        message: format!(
                            "Server-side session restore failed ({fallback_error}). \
                             Falling back to transcript replay. The restored session \
                             will not have tool call information in the context."
                        ),
                    }),
                )))
                .await
                .ok();
        }

        let goal_automation_available = backend.goal_mcp_http_server.lock().await.is_some()
            || backend.goal_ext_capability().is_some();
        let resume_goal_notice = {
            let goals = backend.thread_goal_state.lock().await;
            let now = thread_goal::now_seconds();
            goals.resume_notice_for(now, goal_automation_available)
        };

        let replay_session_id = backend.session_id().await;
        let relay_backend = backend.clone();
        let relay_task = tokio::spawn(async move {
            for ReplayBatch { source, events } in deferred_replay_batches {
                let _ = relay_backend
                    .backend_event_tx
                    .send(BackendEvent::Public(SessionEvent::Nori(
                        nori_protocol::NoriEvent::ReplayStarted(nori_protocol::ReplayStarted {
                            source,
                        }),
                    )))
                    .await;
                for event in events {
                    let _ = relay_backend
                        .backend_event_tx
                        .send(BackendEvent::Public(retarget_replay_event(
                            event,
                            &replay_session_id,
                        )))
                        .await;
                }
                let _ = relay_backend
                    .backend_event_tx
                    .send(BackendEvent::Public(SessionEvent::Nori(
                        nori_protocol::NoriEvent::ReplayFinished,
                    )))
                    .await;
            }
            if let Some(update) = resume_goal_notice {
                let _ = relay_backend
                    .backend_event_tx
                    .send(BackendEvent::Public(SessionEvent::Nori(
                        nori_protocol::NoriEvent::Notice(nori_protocol::Notice {
                            message: match update.hint {
                                Some(hint) => format!("{} {hint}", update.message),
                                None => update.message,
                            },
                        }),
                    )))
                    .await;
            }
            Self::run_connection_event_relay(
                relay_backend,
                event_rx,
                prompt_result_rx,
                approval_policy_rx,
            )
            .await;
        });
        *backend.relay_task_abort.lock().await = Some(relay_task.abort_handle());

        Ok(backend)
    }

    /// Close (release) the active session via ACP `session/close`.
    ///
    /// Used by the explicit close action; callers gate on the agent
    /// advertising `sessionCapabilities.close`. Failures carry the same
    /// enhanced structured-code-aware message as spawn/resume errors, so the
    /// agent's error code and `data.detail` reach the user.
    pub async fn close_active_session(&self) -> Result<()> {
        let session_id = self.session_id.read().await.clone();
        self.connection
            .close_session(&session_id.to_string())
            .await
            .map_err(|e| match get_agent_config(&self.agent_name) {
                Ok(agent_config) => enhance_agent_error(e, &agent_config),
                Err(_) => e,
            })?;
        self.teardown(false, None).await;
        Ok(())
    }
}
