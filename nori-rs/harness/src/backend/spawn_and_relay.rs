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
        let cwd = config.cwd.clone();

        let agent_config = get_agent_config(&config.agent)?;
        debug!("Spawning ACP backend for agent: {}", config.agent);
        let mut connection =
            match AcpConnection::spawn(&agent_config, &cwd, config.acp_proxy.clone()).await {
                Ok(conn) => conn,
                Err(e) => return Err(enhance_agent_error(e, &agent_config)),
            };

        let thread_goal_state = Arc::new(Mutex::new(thread_goal::ThreadGoalState::default()));
        let goal_mcp_connected = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let goal_mcp_http_server = Arc::new(Mutex::new(None));

        let mut mcp_servers = crate::connection::mcp::to_acp_mcp_servers(
            &config.mcp_servers,
            config.mcp_oauth_credentials_store_mode,
        );
        let nori_client_server = nori_client_mcp::register_for_session(
            &connection,
            &mut mcp_servers,
            Arc::clone(&thread_goal_state),
            backend_event_tx.clone(),
            Arc::clone(&goal_mcp_connected),
        )
        .await?;
        if let Some(server) = nori_client_server {
            server.commit(&goal_mcp_http_server).await;
        }
        let session_result = connection.create_session(&cwd, mcp_servers).await;
        let session_id = match session_result {
            Ok(id) => id,
            Err(e) => return Err(enhance_agent_error(e, &agent_config)),
        };

        debug!("ACP session created: {:?}", session_id);

        // Apply default model from config if one is set for this agent
        if let Some(ref default_model) = config.default_model {
            session_defaults::apply_default_model(&connection, &session_id, default_model).await;
        }

        let capabilities_update =
            nori_client_mcp::capabilities_update_for_session(&connection, &goal_mcp_connected);
        let mut event_rx = connection.take_event_receiver();
        for _ in 0..2 {
            let event = event_rx
                .recv()
                .await
                .ok_or_else(|| anyhow::anyhow!("ACP bootstrap event stream closed"))?;
            let crate::connection::ConnectionEvent::Acp(event) = event else {
                anyhow::bail!("ACP bootstrap events arrived out of order");
            };
            backend_event_tx
                .send(BackendEvent::Public(nori_protocol::SessionEvent::Acp(
                    *event,
                )))
                .await
                .map_err(|_| anyhow::anyhow!("session event receiver closed during bootstrap"))?;
        }

        let connection = Arc::new(connection);
        let pending_approvals = Arc::new(Mutex::new(Vec::new()));
        let session_driver = Arc::new(Mutex::new(session_runtime_driver::SessionDriver::new()));
        let (session_event_tx, mut session_event_rx) = mpsc::channel(128);
        let (prompt_result_tx, prompt_result_rx) = mpsc::channel(128);
        let use_native_notifications =
            config.os_notifications == crate::config::OsNotifications::Enabled;
        let user_notifier = Arc::new(crate::UserNotifier::new(
            config.notify.clone(),
            use_native_notifications,
        ));

        let idle_timer_abort = Arc::new(Mutex::new(None));

        // Create watch channel for dynamic approval policy updates
        let (approval_policy_tx, approval_policy_rx) = watch::channel(config.approval_policy);

        // Get history metadata
        let (history_log_id, history_entry_count) =
            crate::message_history::history_metadata(&config.nori_home).await;

        // Initialize transcript recorder (non-fatal if it fails).
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
        let pending_hook_context = fallback_session_context_for_connection(config, &connection);
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
            conversation_id,
            approval_policy_tx,
            pending_compact_summary: Arc::new(Mutex::new(config.initial_context.clone())),
            thread_goal_state,
            goal_mcp_connected,
            goal_mcp_http_server,
            pending_hook_context: Arc::new(Mutex::new(pending_hook_context)),
            transcript_recorder,
            session_event_tx: session_event_tx.clone(),
            prompt_result_tx: prompt_result_tx.clone(),
            notify_after_idle: config.notify_after_idle,
            ghost_snapshots: Arc::new(GhostSnapshotStack::new()),
            is_first_prompt: Arc::new(Mutex::new(true)),
            prompt_summary_enabled: config.prompt_summary_enabled,
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
                nori_protocol::NoriEvent::CapabilitiesChanged(public_nori_capabilities(
                    capabilities_update,
                )),
            )))
            .await
            .ok();
        backend_event_tx
            .send(BackendEvent::Public(nori_protocol::SessionEvent::Nori(
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

        // Spawn reducer loop: processes all transport events and prompt
        // results through the serialized reducer/runtime pipeline.
        let relay_task = tokio::spawn(Self::run_connection_event_relay(
            backend.clone(),
            event_rx,
            prompt_result_rx,
            approval_policy_rx,
        ));
        *backend.relay_task_abort.lock().await = Some(relay_task.abort_handle());

        Ok(backend)
    }

    /// Background task that processes transport events and prompt results
    /// through the serialized session reducer/runtime pipeline.
    pub(super) async fn run_connection_event_relay(
        backend: AcpBackend,
        mut event_rx: mpsc::Receiver<crate::connection::ConnectionEvent>,
        mut prompt_result_rx: mpsc::Receiver<session_reducer::InboundEvent>,
        approval_policy_rx: watch::Receiver<AskForApproval>,
    ) {
        let approval_policy_rx = approval_policy_rx;
        let mut relay_seq = 0_i64;
        loop {
            tokio::select! {
                biased;
                maybe_event = event_rx.recv() => {
                    match maybe_event {
                        Some(crate::connection::ConnectionEvent::SessionUpdate(update)) => {
                            relay_seq += 1;
                            debug!(
                                target: "acp_event_flow",
                                relay_seq,
                                relay_source = "transport_event_rx",
                                update_kind = crate::connection::session_update_kind(&update),
                                "Relaying session/update into serialized session runtime"
                            );
                            let _ = backend
                                .session_event_tx
                                .send(session_runtime_driver::SessionRuntimeInput::Reducer(
                                    session_reducer::InboundEvent::Notification(Box::new(update)),
                                ))
                                .await;
                        }
                        Some(crate::connection::ConnectionEvent::Acp(event)) => {
                            let _ = backend
                                .backend_event_tx
                                .send(BackendEvent::Public(nori_protocol::SessionEvent::Acp(
                                    *event,
                                )))
                                .await;
                        }
                        Some(crate::connection::ConnectionEvent::SessionClosed) => {
                            let _ = backend
                                .backend_event_tx
                                .send(BackendEvent::Public(SessionEvent::Nori(
                                    nori_protocol::NoriEvent::SessionEnded(
                                        nori_protocol::SessionEnded {
                                            reason: nori_protocol::SessionEndReason::Closed,
                                            message: None,
                                        },
                                    ),
                                )))
                                .await;
                            break;
                        }
                        Some(crate::connection::ConnectionEvent::DelegatedRequest(request)) => {
                            relay_seq += 1;
                            let current_policy = *approval_policy_rx.borrow();
                            debug!(
                                target: "acp_event_flow",
                                relay_seq,
                                relay_source = "transport_event_rx",
                                request_id = %request.request_id,
                                "Relaying permission request into serialized session runtime"
                            );
                            let _ = backend
                                .session_event_tx
                                .send(
                                    session_runtime_driver::SessionRuntimeInput::PermissionRequest {
                                        pending_request: Box::new(PendingApprovalRequest {
                                            request,
                                        }),
                                        current_policy,
                                    },
                                )
                                .await;
                        }
                        Some(crate::connection::ConnectionEvent::ChildExited {
                            status,
                            stderr_tail,
                        }) => {
                            if !backend.is_shutting_down.load(Ordering::Relaxed) {
                                let mut message = match status {
                                    Some(code) => format!(
                                        "Agent process exited unexpectedly (exit code {code})."
                                    ),
                                    None => {
                                        "Agent process exited unexpectedly.".to_string()
                                    }
                                };
                                if !stderr_tail.is_empty() {
                                    message.push('\n');
                                    message.push_str(&stderr_tail);
                                }
                                let _ = backend
                                    .backend_event_tx
                                    .send(BackendEvent::Public(SessionEvent::Nori(
                                        nori_protocol::NoriEvent::RequestFailed(
                                            nori_protocol::RequestFailure {
                                                request_id: None,
                                                message: message.clone(),
                                                kind: nori_protocol::RequestFailureKind::Fatal,
                                            },
                                        ),
                                    )))
                                    .await;
                                let _ = backend
                                    .backend_event_tx
                                    .send(BackendEvent::Public(SessionEvent::Nori(
                                        nori_protocol::NoriEvent::SessionEnded(
                                            nori_protocol::SessionEnded {
                                                reason: nori_protocol::SessionEndReason::ConnectionLost,
                                                message: Some(message),
                                            },
                                        ),
                                    )))
                                    .await;
                                // Fail any in-flight prompt loudly instead of
                                // letting it wait on a dead transport forever.
                                if let Some(abort) =
                                    backend.prompt_task_abort.lock().await.take()
                                {
                                    abort.abort();
                                    let _ = backend
                                        .prompt_result_tx
                                        .send(session_reducer::InboundEvent::PromptFailed {
                                            failure: Some(crate::normalized::TurnFailure::Fatal),
                                    })
                                    .await;
                                }
                            }
                            break;
                        }
                        None => break,
                    }
                }
                maybe_result = prompt_result_rx.recv() => {
                    match maybe_result {
                        Some(result) => {
                            relay_seq += 1;
                            debug!(
                                target: "acp_event_flow",
                                relay_seq,
                                relay_source = "prompt_result_rx",
                                inbound_event = session_reducer::inbound_event_kind(&result),
                                "Relaying prompt result into serialized session runtime"
                            );
                            let _ = backend
                                .session_event_tx
                                .send(session_runtime_driver::SessionRuntimeInput::Reducer(result))
                                .await;
                        }
                        None => {
                            while let Some(event) = event_rx.recv().await {
                                match event {
                                    crate::connection::ConnectionEvent::Acp(event) => {
                                        let _ = backend
                                            .backend_event_tx
                                            .send(BackendEvent::Public(
                                                nori_protocol::SessionEvent::Acp(*event),
                                            ))
                                            .await;
                                    }
                                    crate::connection::ConnectionEvent::SessionUpdate(update) => {
                                        relay_seq += 1;
                                        debug!(
                                            target: "acp_event_flow",
                                            relay_seq,
                                            relay_source = "transport_event_rx_drain",
                                            update_kind = crate::connection::session_update_kind(&update),
                                            "Draining session/update after prompt result channel closed"
                                        );
                                        let _ = backend
                                            .session_event_tx
                                            .send(session_runtime_driver::SessionRuntimeInput::Reducer(
                                                session_reducer::InboundEvent::Notification(Box::new(update)),
                                            ))
                                            .await;
                                    }
                                    crate::connection::ConnectionEvent::SessionClosed => {
                                        let _ = backend
                                            .backend_event_tx
                                            .send(BackendEvent::Public(SessionEvent::Nori(
                                                nori_protocol::NoriEvent::SessionEnded(
                                                    nori_protocol::SessionEnded {
                                                        reason: nori_protocol::SessionEndReason::Closed,
                                                        message: None,
                                                    },
                                                ),
                                            )))
                                            .await;
                                        break;
                                    }
                                    // This drain only runs during teardown
                                    // (prompt channel closed); the exit was
                                    // either user-initiated or already
                                    // reported by the main relay arm.
                                    crate::connection::ConnectionEvent::ChildExited { .. } => {}
                                    crate::connection::ConnectionEvent::DelegatedRequest(request) => {
                                        relay_seq += 1;
                                        let current_policy = *approval_policy_rx.borrow();
                                        debug!(
                                            target: "acp_event_flow",
                                            relay_seq,
                                            relay_source = "transport_event_rx_drain",
                                            request_id = %request.request_id,
                                            "Draining permission request after prompt result channel closed"
                                        );
                                        let _ = backend
                                            .session_event_tx
                                            .send(
                                                session_runtime_driver::SessionRuntimeInput::PermissionRequest {
                                                    pending_request: Box::new(PendingApprovalRequest {
                                                        request,
                                                    }),
                                                    current_policy,
                                                },
                                            )
                                            .await;
                                    }
                                }
                            }
                            break;
                        }
                    }
                }
            }
        }
        if let Some(abort) = backend.runtime_task_abort.lock().await.take() {
            abort.abort();
        }
    }
}
