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

        let _ = self
            .session_event_tx
            .send(session_runtime_driver::SessionRuntimeInput::Reducer(
                session_reducer::InboundEvent::PromptSubmit(
                    nori_protocol::session_runtime::QueuedPrompt {
                        event_id: id.to_string(),
                        kind: nori_protocol::session_runtime::QueuedPromptKind::User,
                        text: final_prompt_text,
                        display_text: Some(prompt_text_for_hooks),
                        images: image_blocks,
                        queue_drain:
                            nori_protocol::session_runtime::QueueDrainOutcome::SendNextPrompt,
                    },
                ),
            ))
            .await;

        Ok(())
    }

    /// Handle an exec approval decision by finding and resolving the pending approval.
    pub(super) async fn handle_exec_approval(&self, call_id: &str, decision: ReviewDecision) {
        let mut pending = self.pending_approvals.lock().await;
        if let Some(pos) = pending
            .iter()
            .position(|pending_request| pending_request.request.event.call_id() == call_id)
        {
            let request = pending.remove(pos);
            let _ = request.request.response_tx.send(decision);
        } else {
            warn!("No pending approval found for call_id: {}", call_id);
        }
    }
}
