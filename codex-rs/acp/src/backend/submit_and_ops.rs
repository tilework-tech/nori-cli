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
                let _ = self
                    .session_event_tx
                    .send(session_runtime_driver::SessionRuntimeInput::Reducer(
                        session_reducer::InboundEvent::CancelSubmit,
                    ))
                    .await;
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

                self.connection.shutdown().await;

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

        let _ = self
            .session_event_tx
            .send(session_runtime_driver::SessionRuntimeInput::Reducer(
                session_reducer::InboundEvent::PromptSubmit(
                    nori_protocol::session_runtime::QueuedPrompt {
                        event_id: id.to_string(),
                        kind: nori_protocol::session_runtime::QueuedPromptKind::Compact,
                        text: SUMMARIZATION_PROMPT.to_string(),
                        display_text: None,
                        images: Vec::new(),
                        queue_drain: nori_protocol::session_runtime::QueueDrainOutcome::LeaveQueued,
                    },
                ),
            ))
            .await;

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
