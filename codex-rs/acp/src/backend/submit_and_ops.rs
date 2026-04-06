use std::sync::atomic::Ordering;

use super::*;

impl AcpBackend {
    /// Submit an operation to the ACP backend.
    ///
    /// Translates Codex `Op` variants to appropriate ACP actions:
    /// - `Op::UserInput` → ACP prompt
    /// - `Op::Interrupt` → ACP cancel
    /// - `Op::ExecApproval` → Resolve pending approval
    /// - Other ops → Send error event (not supported)
    pub async fn submit(&self, op: Op) -> Result<String> {
        let id = generate_id();

        // Cancel any running idle timer on new user activity
        if let Some(abort_handle) = self.idle_timer_abort.lock().await.take() {
            abort_handle.abort();
        }

        match op {
            Op::UserInput { items } => {
                self.handle_user_input(items, &id).await?;
            }
            Op::Interrupt => {
                self.turn_id.fetch_add(1, Ordering::SeqCst);
                self.connection
                    .cancel(&*self.session_id.read().await)
                    .await?;
                emit_client_event(
                    &self.backend_event_tx,
                    self.transcript_recorder.as_ref(),
                    nori_protocol::ClientEvent::TurnLifecycle(
                        nori_protocol::TurnLifecycle::Aborted {
                            reason: nori_protocol::TurnAbortReason::Interrupted,
                        },
                    ),
                )
                .await;
            }
            Op::ExecApproval {
                id: call_id,
                decision,
            } => {
                self.handle_exec_approval(&call_id, decision).await;
            }
            Op::PatchApproval {
                id: call_id,
                decision,
            } => {
                self.handle_exec_approval(&call_id, decision).await;
            }
            Op::Shutdown => {
                // Cancel any in-progress session and send ShutdownComplete
                // to allow the TUI to exit properly
                debug!("Processing Op::Shutdown in ACP mode");
                let _ = self.connection.cancel(&*self.session_id.read().await).await;

                // Execute session_end hooks and route output before teardown
                if !self.session_end_hooks.is_empty() {
                    let results =
                        crate::hooks::execute_hooks(&self.session_end_hooks, self.script_timeout)
                            .await;
                    // Context lines are irrelevant during shutdown, so pass None.
                    route_hook_results(&results, &self.event_tx, &id, None).await;
                }

                // Async session end hooks: await completion before shutdown
                // so the runtime doesn't kill them when the process exits.
                if let Some(handle) = crate::hooks::execute_hooks_fire_and_forget(
                    self.async_session_end_hooks.clone(),
                    self.script_timeout,
                    HashMap::new(),
                ) && let Err(e) = handle.await
                {
                    warn!("Async session_end hook task panicked: {e}");
                }

                // Shutdown transcript recorder
                if let Some(ref recorder) = self.transcript_recorder
                    && let Err(e) = recorder.shutdown().await
                {
                    warn!("Failed to shutdown transcript recorder: {e}");
                }

                let _ = self
                    .event_tx
                    .send(Event {
                        id: id.clone(),
                        msg: EventMsg::ShutdownComplete,
                    })
                    .await;
            }
            Op::AddToHistory { text } => {
                // Append to history file in the background
                let nori_home = self.nori_home.clone();
                let conversation_id = self.conversation_id;
                let persistence = self.history_persistence;
                tokio::spawn(async move {
                    if let Err(e) = crate::message_history::append_entry(
                        &text,
                        &conversation_id,
                        &nori_home,
                        persistence,
                    )
                    .await
                    {
                        warn!("failed to append to message history: {e}");
                    }
                });
            }
            Op::GetHistoryEntryRequest { offset, log_id } => {
                // Look up history entry in the background
                let nori_home = self.nori_home.clone();
                let event_tx = self.event_tx.clone();
                let id_clone = id.clone();
                tokio::spawn(async move {
                    // Run lookup in blocking thread because it does file IO + locking.
                    let entry_opt = tokio::task::spawn_blocking(move || {
                        crate::message_history::lookup(log_id, offset, &nori_home)
                    })
                    .await
                    .unwrap_or(None);

                    let event = Event {
                        id: id_clone,
                        msg: EventMsg::GetHistoryEntryResponse(
                            codex_protocol::protocol::GetHistoryEntryResponseEvent {
                                offset,
                                log_id,
                                entry: entry_opt.map(|e| {
                                    codex_protocol::message_history::HistoryEntry {
                                        conversation_id: e.session_id,
                                        ts: e.ts,
                                        text: e.text,
                                    }
                                }),
                            },
                        ),
                    };

                    let _ = event_tx.send(event).await;
                });
            }
            Op::SearchHistoryRequest { max_results } => {
                let nori_home = self.nori_home.clone();
                let event_tx = self.event_tx.clone();
                let id_clone = id.clone();
                tokio::spawn(async move {
                    let entries = tokio::task::spawn_blocking(move || {
                        crate::message_history::search_entries(&nori_home, max_results)
                    })
                    .await
                    .unwrap_or_default();

                    let event = Event {
                        id: id_clone,
                        msg: EventMsg::SearchHistoryResponse(
                            codex_protocol::protocol::SearchHistoryResponseEvent {
                                entries: entries
                                    .into_iter()
                                    .map(|e| codex_protocol::message_history::HistoryEntry {
                                        conversation_id: e.session_id,
                                        ts: e.ts,
                                        text: e.text,
                                    })
                                    .collect(),
                            },
                        ),
                    };

                    let _ = event_tx.send(event).await;
                });
            }
            Op::Compact => {
                self.handle_compact(&id).await?;
            }
            Op::ListCustomPrompts => {
                let dir = commands_dir(&self.nori_home);
                let event_tx = self.event_tx.clone();
                let id_clone = id.clone();
                tokio::spawn(async move {
                    let custom_prompts =
                        codex_core::custom_prompts::discover_prompts_in(&dir).await;
                    let _ = event_tx
                        .send(Event {
                            id: id_clone,
                            msg: EventMsg::ListCustomPromptsResponse(
                                codex_protocol::protocol::ListCustomPromptsResponseEvent {
                                    custom_prompts,
                                },
                            ),
                        })
                        .await;
                });
            }
            Op::Undo => {
                // Best-effort cancel any in-progress agent turn before restoring.
                self.connection
                    .cancel(&*self.session_id.read().await)
                    .await
                    .ok();
                crate::undo::handle_undo(&self.event_tx, &id, &self.cwd, &self.ghost_snapshots)
                    .await;
            }
            Op::UndoList => {
                crate::undo::handle_list_snapshots(&self.event_tx, &id, &self.ghost_snapshots)
                    .await;
            }
            Op::UndoTo { index } => {
                self.connection
                    .cancel(&*self.session_id.read().await)
                    .await
                    .ok();
                crate::undo::handle_undo_to(
                    &self.event_tx,
                    &id,
                    &self.cwd,
                    &self.ghost_snapshots,
                    index,
                )
                .await;
            }
            // Unsupported operations - only show error in debug builds
            Op::RunUserShellCommand { .. } => {
                let op_name = get_op_name(&op);
                warn!("Unsupported Op in ACP mode: {op_name}");
                #[cfg(debug_assertions)]
                self.send_error(&format!(
                    "Operation '{op_name}' is not supported in ACP mode"
                ))
                .await;
            }
            Op::OverrideTurnContext {
                approval_policy, ..
            } => {
                // Update approval policy if provided
                if let Some(policy) = approval_policy {
                    debug!("Updating approval policy to {policy:?} in ACP mode");
                    // Send the new policy to the approval handler via watch channel
                    let _ = self.approval_policy_tx.send(policy);
                }
            }
            // These ops are internal/context-related, silently ignore
            Op::UserTurn { .. } | Op::ResolveElicitation { .. } => {
                debug!("Ignoring internal Op in ACP mode: {}", get_op_name(&op));
            }
            // Catch any new Op variants we haven't handled - only show error in debug builds
            _ => {
                let op_name = get_op_name(&op);
                warn!("Unknown Op in ACP mode: {op_name}");
                #[cfg(debug_assertions)]
                self.send_error(&format!(
                    "Operation '{op_name}' is not supported in ACP mode"
                ))
                .await;
            }
        }

        Ok(id)
    }

    /// Handle the /compact operation by sending a summarization prompt to the agent,
    /// capturing the summary, and storing it for the next user prompt.
    ///
    /// This implements Option 3 (Prompt-Based Approach) from the implementation plan:
    /// 1. Send the summarization prompt to the agent
    /// 2. Capture the agent's summary response
    /// 3. Store it in pending_compact_summary
    /// 4. Emit ContextCompacted and Warning events
    pub(super) async fn handle_compact(&self, id: &str) -> Result<()> {
        use codex_core::compact::SUMMARIZATION_PROMPT;

        // Build the summarization prompt
        let prompt = vec![translator::text_to_content_block(SUMMARIZATION_PROMPT)];

        // Create channel for receiving session updates
        let (update_tx, mut update_rx) = mpsc::channel(32);
        let (done_tx, mut done_rx) = tokio::sync::oneshot::channel::<()>();

        // Clone what we need for capturing the response
        let event_tx = self.event_tx.clone();
        let session_id = self.session_id.read().await.clone();
        let session_id_lock = Arc::clone(&self.session_id);
        let connection = Arc::clone(&self.connection);
        let cwd = self.cwd.clone();
        let mcp_servers = crate::connection::mcp::to_sacp_mcp_servers(&self.mcp_servers);
        let id_clone = id.to_string();
        let pending_compact_summary = Arc::clone(&self.pending_compact_summary);
        let user_notifier = Arc::clone(&self.user_notifier);
        let idle_timer_abort = Arc::clone(&self.idle_timer_abort);
        let notify_after_idle = self.notify_after_idle;
        let client_event_normalizer = Arc::clone(&self.client_event_normalizer);
        let backend_event_tx = self.backend_event_tx.clone();
        let transcript_recorder = self.transcript_recorder.clone();
        let turn_id = Arc::clone(&self.turn_id);
        let my_turn_id = turn_id.fetch_add(1, Ordering::SeqCst) + 1;

        // Spawn task to handle the prompt and capture the summary
        tokio::spawn(async move {
            // Cancel any existing idle timer when a new turn starts processing
            if let Some(abort_handle) = idle_timer_abort.lock().await.take() {
                abort_handle.abort();
            }

            // Send TaskStarted event (inside spawned task for consistency)
            emit_client_event(
                &backend_event_tx,
                transcript_recorder.as_ref(),
                nori_protocol::ClientEvent::TurnLifecycle(nori_protocol::TurnLifecycle::Started),
            )
            .await;

            // Spawn update consumer task to capture the agent's response
            let pending_summary_for_capture = Arc::clone(&pending_compact_summary);
            let client_event_normalizer = Arc::clone(&client_event_normalizer);
            let backend_event_tx_for_updates = backend_event_tx.clone();

            let update_handler = tokio::spawn(async move {
                let mut summary_text = String::new();
                let mut done = false;

                loop {
                    let update = if done {
                        match tokio::time::timeout(
                            super::POST_PROMPT_DRAIN_TIMEOUT,
                            update_rx.recv(),
                        )
                        .await
                        {
                            Ok(Some(u)) => u,
                            _ => break,
                        }
                    } else {
                        tokio::select! {
                            msg = update_rx.recv() => match msg {
                                Some(u) => u,
                                None => break,
                            },
                            _ = &mut done_rx => {
                                done = true;
                                continue;
                            }
                        }
                    };
                    let client_events =
                        normalize_session_update(&client_event_normalizer, &update).await;
                    forward_client_events(&backend_event_tx_for_updates, &client_events).await;

                    // Capture text from agent message chunks
                    if let acp::SessionUpdate::AgentMessageChunk(chunk) = &update
                        && let acp::ContentBlock::Text(text) = &chunk.content
                    {
                        summary_text.push_str(&text.text);
                    }
                }

                // Store the captured summary for use in the next prompt
                if !summary_text.is_empty() {
                    *pending_summary_for_capture.lock().await = Some(summary_text);
                }
            });

            // Send the summarization prompt
            let session_id_for_timer = session_id.to_string();
            let result = connection.prompt(session_id, prompt, update_tx).await;

            // Signal the update_handler to drain remaining events and stop.
            let _ = done_tx.send(());

            // Wait for all updates to be processed
            let _ = update_handler.await;

            // Only emit tail events if this is still the active turn. When
            // the turn_id has advanced, this task is stale and all its late
            // events (errors, warnings, Completed) must be suppressed.
            if turn_id.load(Ordering::SeqCst) == my_turn_id {
                if let Err(ref e) = result {
                    warn!("Compact prompt failed: {e}");
                    *pending_compact_summary.lock().await = None;
                    let _ = event_tx
                        .send(Event {
                            id: id_clone.clone(),
                            msg: EventMsg::Error(ErrorEvent {
                                message: format!("Compact failed: {e}"),
                                codex_error_info: None,
                            }),
                        })
                        .await;
                } else {
                    match connection.create_session(&cwd, mcp_servers).await {
                        Ok(new_session_id) => {
                            debug!("Created new session after compact: {:?}", new_session_id);
                            *session_id_lock.write().await = new_session_id;
                        }
                        Err(e) => {
                            warn!("Failed to create new session after compact: {e}");
                        }
                    }

                    let compact_summary = pending_compact_summary.lock().await.clone();
                    emit_client_event(
                        &backend_event_tx,
                        transcript_recorder.as_ref(),
                        nori_protocol::ClientEvent::TurnLifecycle(
                            nori_protocol::TurnLifecycle::ContextCompacted {
                                summary: compact_summary.clone(),
                            },
                        ),
                    )
                    .await;

                    let _ = event_tx
                        .send(Event {
                            id: id_clone.clone(),
                            msg: EventMsg::Warning(WarningEvent {
                                message: "Heads up: Long conversations and multiple compactions can cause the model to be less accurate. Start a new conversation when possible to keep conversations small and targeted.".to_string(),
                            }),
                        })
                        .await;
                }

                emit_client_event(
                    &backend_event_tx,
                    transcript_recorder.as_ref(),
                    nori_protocol::ClientEvent::TurnLifecycle(
                        nori_protocol::TurnLifecycle::Completed {
                            last_agent_message: None,
                        },
                    ),
                )
                .await;
                // Start idle timer if configured
                if let Some(duration) = notify_after_idle.as_duration() {
                    let idle_secs = duration.as_secs();
                    let user_notifier_for_timer = Arc::clone(&user_notifier);
                    let idle_task = tokio::spawn(async move {
                        tokio::time::sleep(duration).await;
                        user_notifier_for_timer.notify(&codex_core::UserNotification::Idle {
                            session_id: session_id_for_timer,
                            idle_duration_secs: idle_secs,
                        });
                    });
                    *idle_timer_abort.lock().await = Some(idle_task.abort_handle());
                }
            } else if let Err(ref e) = result {
                warn!("Compact prompt failed (stale turn, suppressed): {e}");
                *pending_compact_summary.lock().await = None;
            }
        });

        Ok(())
    }

    /// Send an error event to the TUI (only used in debug builds).
    #[cfg(debug_assertions)]
    pub(super) async fn send_error(&self, message: &str) {
        let _ = self
            .event_tx
            .send(Event {
                id: String::new(),
                msg: EventMsg::Error(ErrorEvent {
                    message: message.to_string(),
                    codex_error_info: None,
                }),
            })
            .await;
    }

    /// Get the current model state from the ACP connection.
    ///
    /// Returns information about the current model and available models.
    /// This state is updated when a session is created or when the model is switched.
    pub fn model_state(&self) -> AcpModelState {
        self.connection.model_state()
    }

    /// Get the current session ID.
    ///
    /// Note: This clones the session ID since it may be replaced during /compact.
    pub async fn session_id(&self) -> acp::SessionId {
        self.session_id.read().await.clone()
    }

    /// Get a reference to the underlying ACP connection.
    ///
    /// This provides access to low-level ACP operations like model switching.
    pub fn connection(&self) -> &Arc<SacpConnection> {
        &self.connection
    }

    /// Switch to a different model for the current session.
    ///
    /// This sends a `session/set_model` request to the ACP agent and updates
    /// the internal model state. The model_id must be one of the available
    /// models returned by `model_state().available_models`.
    ///
    /// # Arguments
    /// * `model_id` - The ID of the model to switch to
    ///
    /// # Errors
    /// Returns an error if the model switch fails (e.g., invalid model ID,
    /// agent doesn't support model switching, or connection error).
    #[cfg(feature = "unstable")]
    pub async fn set_model(&self, model_id: &acp::ModelId) -> Result<()> {
        let session_id = self.session_id.read().await;
        self.connection.set_model(&session_id, model_id).await
    }
}
