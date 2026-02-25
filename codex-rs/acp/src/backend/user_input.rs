use super::*;

impl AcpBackend {
    /// Handle user input by sending a prompt to the ACP agent.
    pub(super) async fn handle_user_input(&self, items: Vec<UserInput>, id: &str) -> Result<()> {
        // Separate text items (needed for hooks, summary, transcript) from
        // image items (converted to ACP ContentBlock::Image).
        let mut prompt_text = String::new();
        let mut image_items = Vec::new();
        for item in items {
            match item {
                UserInput::Text { text } => {
                    if !prompt_text.is_empty() {
                        prompt_text.push('\n');
                    }
                    prompt_text.push_str(&text);
                }
                UserInput::Image { .. } | UserInput::LocalImage { .. } => {
                    image_items.push(item);
                }
                _ => {
                    warn!("Unknown UserInput variant in ACP mode");
                }
            }
        }

        // Convert image items to ACP content blocks
        let image_blocks = translator::user_inputs_to_content_blocks(image_items)?;

        if prompt_text.is_empty() && image_blocks.is_empty() {
            return Ok(());
        }

        // For image-only prompts, use a placeholder for downstream consumers
        // (hooks, transcript, summary, snapshot labels) that expect non-empty text.
        let display_text = if prompt_text.is_empty() && !image_blocks.is_empty() {
            "[image]".to_string()
        } else {
            prompt_text.clone()
        };

        // Execute pre_user_prompt hooks before sending the prompt
        if !self.pre_user_prompt_hooks.is_empty() {
            let env_vars = HashMap::from([
                ("NORI_HOOK_EVENT".to_string(), "pre_user_prompt".to_string()),
                ("NORI_HOOK_PROMPT_TEXT".to_string(), display_text.clone()),
            ]);
            let results = crate::hooks::execute_hooks_with_env(
                &self.pre_user_prompt_hooks,
                self.script_timeout,
                &env_vars,
            )
            .await;
            route_hook_results(
                &results,
                &self.event_tx,
                id,
                Some(&self.pending_hook_context),
            )
            .await;
        }

        // Fire-and-forget async pre_user_prompt hooks
        if !self.async_pre_user_prompt_hooks.is_empty() {
            let env_vars = HashMap::from([
                ("NORI_HOOK_EVENT".to_string(), "pre_user_prompt".to_string()),
                ("NORI_HOOK_PROMPT_TEXT".to_string(), display_text.clone()),
            ]);
            let _ = crate::hooks::execute_hooks_fire_and_forget(
                self.async_pre_user_prompt_hooks.clone(),
                self.script_timeout,
                env_vars,
            );
        }

        // On first prompt, spawn a fire-and-forget summarization task.
        // Skip for mock models (debug-only test agents) since they don't
        // produce meaningful summaries.
        {
            let mut is_first = self.is_first_prompt.lock().await;
            if *is_first {
                *is_first = false;
                let skip_summary = cfg!(debug_assertions) && self.agent_name.starts_with("mock-");
                if !skip_summary {
                    let event_tx = self.event_tx.clone();
                    let agent_name = self.agent_name.clone();
                    let cwd = self.cwd.clone();
                    let prompt_for_summary = display_text.clone();
                    let auto_worktree = self.auto_worktree;
                    let auto_worktree_repo_root = self.auto_worktree_repo_root.clone();
                    tokio::spawn(async move {
                        if let Err(e) = run_prompt_summary(
                            &event_tx,
                            &agent_name,
                            &cwd,
                            &prompt_for_summary,
                            auto_worktree,
                            auto_worktree_repo_root.as_deref(),
                        )
                        .await
                        {
                            debug!("Prompt summary failed (non-fatal): {e}");
                        }
                    });
                }
            }
        }

        // Create ghost snapshot before sending prompt to agent.
        // This captures the working tree state so /undo can restore it.
        let snapshot_cwd = self.cwd.clone();
        let ghost_snapshots = Arc::clone(&self.ghost_snapshots);
        let label_for_snapshot = display_text.clone();
        match tokio::task::spawn_blocking(move || {
            let options = codex_git::CreateGhostCommitOptions::new(&snapshot_cwd);
            codex_git::create_ghost_commit(&options)
        })
        .await
        {
            Ok(Ok(snapshot)) => {
                ghost_snapshots.push(snapshot, label_for_snapshot).await;
            }
            Ok(Err(codex_git::GitToolingError::NotAGitRepository { .. })) => {
                debug!("Skipping ghost snapshot: not a git repository");
            }
            Ok(Err(err)) => {
                warn!("Failed to create ghost snapshot: {err}");
            }
            Err(err) => {
                warn!("Ghost snapshot task panicked: {err}");
            }
        }

        // Record user message to transcript
        if let Some(ref recorder) = self.transcript_recorder
            && let Err(e) = recorder
                .record_user_message(id, &display_text, vec![])
                .await
        {
            warn!("Failed to record user message to transcript: {e}");
        }

        // Save prompt text for post_user_prompt hooks (before it gets moved)
        let prompt_text_for_hooks = display_text;

        // Prepend any accumulated hook context (from ::context:: lines)
        // This must happen before the compact summary prefix so that the
        // SUMMARY_PREFIX framing instruction always comes first.
        let prompt_with_context = if let Some(ctx) = self.pending_hook_context.lock().await.take() {
            format!("{ctx}\n{prompt_text}")
        } else {
            prompt_text
        };

        // Check if we have a pending compact summary to prepend
        let pending_summary = self.pending_compact_summary.lock().await.take();
        let final_prompt_text = if let Some(summary) = pending_summary {
            use codex_core::compact::SUMMARY_PREFIX;
            format!("{SUMMARY_PREFIX}\n{summary}\n\n{prompt_with_context}")
        } else {
            prompt_with_context
        };

        let mut prompt = Vec::new();
        if !final_prompt_text.is_empty() {
            prompt.push(translator::text_to_content_block(&final_prompt_text));
        }
        prompt.extend(image_blocks);

        // Create channel for receiving session updates
        let (update_tx, mut update_rx) = mpsc::channel(32);

        // Clone what we need for the background task
        let event_tx = self.event_tx.clone();
        let session_id = self.session_id.read().await.clone();
        let connection = Arc::clone(&self.connection);
        let id_clone = id.to_string();
        let user_notifier = Arc::clone(&self.user_notifier);
        let idle_timer_abort = Arc::clone(&self.idle_timer_abort);
        let transcript_recorder = self.transcript_recorder.clone();
        let notify_after_idle = self.notify_after_idle;
        let post_user_prompt_hooks = self.post_user_prompt_hooks.clone();
        let pre_tool_call_hooks = self.pre_tool_call_hooks.clone();
        let post_tool_call_hooks = self.post_tool_call_hooks.clone();
        let pre_agent_response_hooks = self.pre_agent_response_hooks.clone();
        let post_agent_response_hooks = self.post_agent_response_hooks.clone();
        let async_post_user_prompt_hooks = self.async_post_user_prompt_hooks.clone();
        let async_pre_tool_call_hooks = self.async_pre_tool_call_hooks.clone();
        let async_post_tool_call_hooks = self.async_post_tool_call_hooks.clone();
        let async_pre_agent_response_hooks = self.async_pre_agent_response_hooks.clone();
        let async_post_agent_response_hooks = self.async_post_agent_response_hooks.clone();
        let hook_timeout = self.script_timeout;
        let pending_hook_context = Arc::clone(&self.pending_hook_context);

        // Spawn task to handle the prompt and translate events
        tokio::spawn(async move {
            // Cancel any existing idle timer when a new turn starts processing.
            // This handles the case where a new prompt arrives while a previous
            // task's idle timer is pending but before submit() could cancel it.
            if let Some(abort_handle) = idle_timer_abort.lock().await.take() {
                abort_handle.abort();
            }

            // Send TaskStarted event
            let _ = event_tx
                .send(Event {
                    id: id_clone.clone(),
                    msg: EventMsg::TaskStarted(codex_protocol::protocol::TaskStartedEvent {
                        model_context_window: None,
                    }),
                })
                .await;

            // Spawn update consumer task that returns accumulated text for transcript
            let event_tx_clone = event_tx.clone();
            let id_for_updates = id_clone.clone();
            let transcript_recorder_for_updates = transcript_recorder.clone();
            let pre_tool_call_hooks_for_updates = pre_tool_call_hooks.clone();
            let post_tool_call_hooks_for_updates = post_tool_call_hooks.clone();
            let pre_agent_response_hooks_for_updates = pre_agent_response_hooks.clone();
            let async_pre_tool_call_hooks_for_updates = async_pre_tool_call_hooks.clone();
            let async_post_tool_call_hooks_for_updates = async_post_tool_call_hooks.clone();
            let async_pre_agent_response_hooks_for_updates = async_pre_agent_response_hooks.clone();
            let update_handler = tokio::spawn(async move {
                let mut event_sequence: u64 = 0;
                // Accumulate assistant text for transcript recording
                let mut accumulated_text = String::new();
                // Track whether pre_agent_response hook has fired
                let mut has_fired_pre_agent_response = false;
                let mut has_agent_text = false;
                let mut needs_agent_separator = false;
                // Track call_ids that have already been recorded to the transcript.
                let mut recorded_tool_call_ids: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                // Track pending patch operations: store FileChange data from ToolCall events
                // so we can emit PatchApplyBegin on ToolCallUpdate (after approval).
                let mut pending_patch_changes: std::collections::HashMap<
                    String,
                    std::collections::HashMap<PathBuf, codex_protocol::protocol::FileChange>,
                > = std::collections::HashMap::new();
                while let Some(update) = update_rx.recv().await {
                    if has_agent_text
                        && matches!(
                            update,
                            acp::SessionUpdate::ToolCall(_)
                                | acp::SessionUpdate::ToolCallUpdate(_)
                                | acp::SessionUpdate::Plan(_)
                                | acp::SessionUpdate::UserMessageChunk(_)
                                | acp::SessionUpdate::CurrentModeUpdate(_)
                                | acp::SessionUpdate::AvailableCommandsUpdate(_)
                        )
                    {
                        needs_agent_separator = true;
                    }
                    // Record tool calls and results to transcript
                    if let Some(ref recorder) = transcript_recorder_for_updates {
                        record_tool_events_to_transcript(
                            &update,
                            recorder,
                            &mut recorded_tool_call_ids,
                        )
                        .await;
                    }

                    // Execute pre_agent_response hooks on first agent message chunk
                    if let acp::SessionUpdate::AgentMessageChunk(chunk) = &update
                        && !has_fired_pre_agent_response
                        && let acp::ContentBlock::Text(text) = &chunk.content
                        && !text.text.is_empty()
                    {
                        has_fired_pre_agent_response = true;
                        if !pre_agent_response_hooks_for_updates.is_empty() {
                            let env_vars = HashMap::from([(
                                "NORI_HOOK_EVENT".to_string(),
                                "pre_agent_response".to_string(),
                            )]);
                            let results = crate::hooks::execute_hooks_with_env(
                                &pre_agent_response_hooks_for_updates,
                                hook_timeout,
                                &env_vars,
                            )
                            .await;
                            route_hook_results(&results, &event_tx_clone, &id_for_updates, None)
                                .await;
                        }
                        if !async_pre_agent_response_hooks_for_updates.is_empty() {
                            let env = HashMap::from([(
                                "NORI_HOOK_EVENT".to_string(),
                                "pre_agent_response".to_string(),
                            )]);
                            let _ = crate::hooks::execute_hooks_fire_and_forget(
                                async_pre_agent_response_hooks_for_updates.clone(),
                                hook_timeout,
                                env,
                            );
                        }
                    }

                    // Execute pre_tool_call hooks when a tool call begins
                    if let acp::SessionUpdate::ToolCall(tool_call) = &update {
                        let env_vars = HashMap::from([
                            ("NORI_HOOK_EVENT".to_string(), "pre_tool_call".to_string()),
                            ("NORI_HOOK_TOOL_NAME".to_string(), tool_call.title.clone()),
                            (
                                "NORI_HOOK_TOOL_ARGS".to_string(),
                                tool_call
                                    .raw_input
                                    .as_ref()
                                    .map_or_else(String::new, std::string::ToString::to_string),
                            ),
                        ]);
                        if !pre_tool_call_hooks_for_updates.is_empty() {
                            let results = crate::hooks::execute_hooks_with_env(
                                &pre_tool_call_hooks_for_updates,
                                hook_timeout,
                                &env_vars,
                            )
                            .await;
                            route_hook_results(&results, &event_tx_clone, &id_for_updates, None)
                                .await;
                        }
                        if !async_pre_tool_call_hooks_for_updates.is_empty() {
                            let _ = crate::hooks::execute_hooks_fire_and_forget(
                                async_pre_tool_call_hooks_for_updates.clone(),
                                hook_timeout,
                                env_vars.clone(),
                            );
                        }
                    }

                    // Execute post_tool_call hooks when a tool call completes
                    if let acp::SessionUpdate::ToolCallUpdate(tcu) = &update
                        && tcu.fields.status == Some(acp::ToolCallStatus::Completed)
                    {
                        let tool_output = extract_tool_output(&tcu.fields);
                        let env_vars = HashMap::from([
                            ("NORI_HOOK_EVENT".to_string(), "post_tool_call".to_string()),
                            (
                                "NORI_HOOK_TOOL_NAME".to_string(),
                                tcu.fields.title.clone().unwrap_or_default(),
                            ),
                            ("NORI_HOOK_TOOL_OUTPUT".to_string(), tool_output),
                        ]);
                        if !post_tool_call_hooks_for_updates.is_empty() {
                            let results = crate::hooks::execute_hooks_with_env(
                                &post_tool_call_hooks_for_updates,
                                hook_timeout,
                                &env_vars,
                            )
                            .await;
                            route_hook_results(&results, &event_tx_clone, &id_for_updates, None)
                                .await;
                        }
                        if !async_post_tool_call_hooks_for_updates.is_empty() {
                            let _ = crate::hooks::execute_hooks_fire_and_forget(
                                async_post_tool_call_hooks_for_updates.clone(),
                                hook_timeout,
                                env_vars.clone(),
                            );
                        }
                    }

                    let events =
                        translate_session_update_to_events(&update, &mut pending_patch_changes);
                    for mut event_msg in events {
                        // Accumulate text for transcript
                        if let EventMsg::AgentMessageDelta(ref mut delta) = event_msg {
                            if needs_agent_separator && has_agent_text {
                                if !delta.delta.starts_with('\n') {
                                    delta.delta =
                                        format!("\n{delta_text}", delta_text = delta.delta);
                                }
                                needs_agent_separator = false;
                            }
                            if !delta.delta.is_empty() {
                                has_agent_text = true;
                            }
                            accumulated_text.push_str(&delta.delta);
                        }
                        event_sequence += 1;
                        debug!(
                            target: "acp_event_flow",
                            seq = event_sequence,
                            event_type = get_event_msg_type(&event_msg),
                            "ACP dispatch: sending event to TUI"
                        );
                        let _ = event_tx_clone
                            .send(Event {
                                id: id_for_updates.clone(),
                                msg: event_msg,
                            })
                            .await;
                    }
                }
                debug!(
                    target: "acp_event_flow",
                    total_events = event_sequence,
                    "ACP dispatch: update stream completed"
                );
                accumulated_text
            });

            // Send the prompt (clone session_id before moving it since we need it for idle timer)
            let session_id_for_timer = session_id.to_string();
            let result = connection.prompt(session_id, prompt, update_tx).await;

            // Wait for all updates to be processed and get accumulated text
            let accumulated_text = update_handler.await.unwrap_or_default();

            // Record assistant message to transcript if there's accumulated text
            if !accumulated_text.is_empty()
                && let Some(ref recorder) = transcript_recorder
            {
                let content = vec![ContentBlock::Text {
                    text: accumulated_text.clone(),
                }];
                if let Err(e) = recorder
                    .record_assistant_message(&id_clone, content, None)
                    .await
                {
                    warn!("Failed to record assistant message to transcript: {e}");
                }
            }

            // Execute post_agent_response hooks after the agent has finished responding
            if !accumulated_text.is_empty() && !post_agent_response_hooks.is_empty() {
                let env_vars = HashMap::from([
                    (
                        "NORI_HOOK_EVENT".to_string(),
                        "post_agent_response".to_string(),
                    ),
                    (
                        "NORI_HOOK_RESPONSE_TEXT".to_string(),
                        accumulated_text.clone(),
                    ),
                ]);
                let results = crate::hooks::execute_hooks_with_env(
                    &post_agent_response_hooks,
                    hook_timeout,
                    &env_vars,
                )
                .await;
                route_hook_results(&results, &event_tx, &id_clone, None).await;
            }

            if !accumulated_text.is_empty() && !async_post_agent_response_hooks.is_empty() {
                let env_vars = HashMap::from([
                    (
                        "NORI_HOOK_EVENT".to_string(),
                        "post_agent_response".to_string(),
                    ),
                    (
                        "NORI_HOOK_RESPONSE_TEXT".to_string(),
                        accumulated_text.clone(),
                    ),
                ]);
                let _ = crate::hooks::execute_hooks_fire_and_forget(
                    async_post_agent_response_hooks,
                    hook_timeout,
                    env_vars,
                );
            }

            // Execute post_user_prompt hooks after the turn completes
            if !post_user_prompt_hooks.is_empty() {
                let env_vars = HashMap::from([
                    (
                        "NORI_HOOK_EVENT".to_string(),
                        "post_user_prompt".to_string(),
                    ),
                    (
                        "NORI_HOOK_PROMPT_TEXT".to_string(),
                        prompt_text_for_hooks.clone(),
                    ),
                ]);
                let results = crate::hooks::execute_hooks_with_env(
                    &post_user_prompt_hooks,
                    hook_timeout,
                    &env_vars,
                )
                .await;
                route_hook_results(&results, &event_tx, &id_clone, Some(&pending_hook_context))
                    .await;
            }

            if !async_post_user_prompt_hooks.is_empty() {
                let env_vars = HashMap::from([
                    (
                        "NORI_HOOK_EVENT".to_string(),
                        "post_user_prompt".to_string(),
                    ),
                    (
                        "NORI_HOOK_PROMPT_TEXT".to_string(),
                        prompt_text_for_hooks.clone(),
                    ),
                ]);
                let _ = crate::hooks::execute_hooks_fire_and_forget(
                    async_post_user_prompt_hooks,
                    hook_timeout,
                    env_vars,
                );
            }

            // If prompt failed, send an error event to the TUI BEFORE TaskComplete
            // This ensures the user sees why their request failed instead of a silent failure
            if let Err(ref e) = result {
                let error_string = format!("{e:?}");
                let category = categorize_acp_error(&error_string);
                let display_error = format!("{e:#}");

                // Generate user-friendly message based on error category
                let user_message = match category {
                    AcpErrorCategory::Authentication => {
                        format!(
                            "Authentication error: {display_error}. Please check your credentials or re-authenticate."
                        )
                    }
                    AcpErrorCategory::QuotaExceeded => {
                        format!("Rate limit or quota exceeded: {display_error}")
                    }
                    AcpErrorCategory::ExecutableNotFound => {
                        format!("Agent executable not found: {display_error}")
                    }
                    AcpErrorCategory::Initialization => {
                        format!("Agent initialization failed: {display_error}")
                    }
                    AcpErrorCategory::PromptTooLong => {
                        "Prompt is too long. Try using /compact to reduce context size, or start a new session."
                            .to_string()
                    }
                    AcpErrorCategory::ApiServerError => {
                        "The API returned a server error. This is usually temporary — please try again."
                            .to_string()
                    }
                    AcpErrorCategory::Unknown => {
                        format!("ACP prompt failed: {display_error}")
                    }
                };

                warn!("ACP prompt failed: {}", e);
                debug!(
                    target: "acp_event_flow",
                    user_message = %user_message,
                    "ACP prompt failure: sending ErrorEvent to TUI"
                );

                // Send error event to TUI so user sees the error
                let _ = event_tx
                    .send(Event {
                        id: id_clone.clone(),
                        msg: EventMsg::Error(ErrorEvent {
                            message: user_message.clone(),
                            codex_error_info: None,
                        }),
                    })
                    .await;

                debug!(
                    target: "acp_event_flow",
                    "ACP prompt failure: ErrorEvent sent to TUI"
                );
            }

            // Send TaskComplete event (always, to end the turn)
            let _ = event_tx
                .send(Event {
                    id: id_clone,
                    msg: EventMsg::TaskComplete(codex_protocol::protocol::TaskCompleteEvent {
                        last_agent_message: None,
                    }),
                })
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
                // Store the abort handle so the timer can be cancelled on new activity
                *idle_timer_abort.lock().await = Some(idle_task.abort_handle());
            }
        });

        Ok(())
    }

    /// Handle an exec approval decision by finding and resolving the pending approval.
    pub(super) async fn handle_exec_approval(&self, call_id: &str, decision: ReviewDecision) {
        let mut pending = self.pending_approvals.lock().await;
        if let Some(pos) = pending.iter().position(|r| r.event.call_id() == call_id) {
            let request = pending.remove(pos);
            let _ = request.response_tx.send(decision);
        } else {
            warn!("No pending approval found for call_id: {}", call_id);
        }
    }
}
