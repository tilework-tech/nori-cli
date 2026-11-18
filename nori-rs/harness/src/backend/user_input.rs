use super::*;
use crate::follow_up::PromptOrigin;

impl AcpBackend {
    /// Submit schema-native ACP content through Nori's prompt lifecycle.
    pub(super) async fn handle_prompt(
        &self,
        content: Vec<acp::ContentBlock>,
        id: &str,
        prompt_meta: Option<acp::Meta>,
        admission: PromptAdmission,
    ) -> Result<()> {
        let origin = if admission == PromptAdmission::Queue && prompt_meta.is_none() {
            PromptOrigin::LocalComposer
        } else {
            PromptOrigin::External
        };
        let prompt = self
            .prepare_user_input(content, id, prompt_meta, origin)
            .await;
        let event = match admission {
            PromptAdmission::Queue => session_reducer::InboundEvent::PromptSubmit(prompt),
            PromptAdmission::RejectIfBusy => {
                session_reducer::InboundEvent::PromptSubmitIfIdle(prompt)
            }
        };
        self.session_event_tx
            .send(session_runtime_driver::SessionRuntimeInput::Reducer(event))
            .await
            .map_err(|_| anyhow::anyhow!("ACP session runtime closed during prompt submission"))?;

        Ok(())
    }

    pub(super) async fn prepare_user_input(
        &self,
        content: Vec<acp::ContentBlock>,
        id: &str,
        mut prompt_meta: Option<acp::Meta>,
        origin: PromptOrigin,
    ) -> crate::normalized::session_runtime::QueuedPrompt {
        let prompt_text = content
            .iter()
            .filter_map(|block| match block {
                acp::ContentBlock::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .fold(String::new(), |mut prompt_text, text| {
                if !prompt_text.is_empty() {
                    prompt_text.push('\n');
                }
                prompt_text.push_str(text);
                prompt_text
            });

        // For image-only prompts, use a placeholder for downstream consumers
        // (hooks, transcript, summary, snapshot labels) that expect non-empty text.
        let display_text = if prompt_text.is_empty() && !content.is_empty() {
            "[attachment]".to_string()
        } else {
            prompt_text.clone()
        };

        // Only text the local user typed may go back to their composer; a
        // remote controller's or a delegated child's prompt may not. Its
        // inherited hook context is captured before this prompt's own hooks
        // add theirs, so a reclaim can return just what it consumed.
        let local_user_prompt = origin == PromptOrigin::LocalComposer
            && nori_protocol::delegation_result_summary(&display_text).is_none();
        let inherited_hook_context = self.pending_hook_context.lock().await.clone();

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
                &self.backend_event_tx,
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
            // A child's result is not the user's own first prompt.
            if *is_first && nori_protocol::delegation_result_summary(&display_text).is_none() {
                *is_first = false;
                let skip_summary = cfg!(debug_assertions) && self.agent_name.starts_with("mock-");
                if !skip_summary && (self.prompt_summary_enabled || self.auto_worktree.is_enabled())
                {
                    let event_tx = self.backend_event_tx.clone();
                    let agent_name = self.agent_name.clone();
                    let cwd = self.cwd.clone();
                    let prompt_for_summary = display_text.clone();
                    let auto_worktree = self.auto_worktree;
                    let auto_worktree_repo_root = self.auto_worktree_repo_root.clone();
                    let acp_proxy = self.acp_proxy.clone();
                    tokio::spawn(async move {
                        if let Err(e) = run_prompt_summary(
                            &event_tx,
                            &agent_name,
                            &cwd,
                            &prompt_for_summary,
                            auto_worktree,
                            auto_worktree_repo_root.as_deref(),
                            acp_proxy,
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

        // Save prompt text for post_user_prompt hooks (before it gets moved)
        let prompt_text_for_hooks = display_text;

        // Prepend any accumulated hook context (from ::context:: lines)
        // This must happen before the compact summary prefix so that the
        // SUMMARY_PREFIX framing instruction always comes first.
        let prompt_with_context = if let Some(ctx) = self.pending_hook_context.lock().await.take() {
            format!("{ctx}\n{prompt_text}")
        } else {
            prompt_text.clone()
        };
        let prompt_with_goal_context = self
            .prepend_goal_context_to_prompt(prompt_with_context)
            .await;

        // Check if we have a pending compact summary to prepend
        let pending_summary = self.pending_compact_summary.lock().await.take();
        let final_prompt_text = if let Some(summary) = &pending_summary {
            use crate::compact::SUMMARY_PREFIX;
            format!("{SUMMARY_PREFIX}\n{summary}\n\n{prompt_with_goal_context}")
        } else {
            prompt_with_goal_context
        };

        // Internal context is an additional leading block. The caller's ACP
        // content remains an ordered subsequence of the wire prompt.
        let echo_id = prompt_meta
            .as_ref()
            .and_then(|meta| meta.get(nori_protocol::PROMPT_ECHO_ID_META_KEY))
            .and_then(serde_json::Value::as_str)
            .unwrap_or(id)
            .to_string();
        prompt_meta.get_or_insert_default().insert(
            nori_protocol::PROMPT_ECHO_ID_META_KEY.to_string(),
            serde_json::Value::String(echo_id),
        );
        let original_content = content.clone();
        let mut final_content = content;
        if final_prompt_text != prompt_text {
            let injected_prefix = final_prompt_text
                .strip_suffix(&prompt_text)
                .unwrap_or(&final_prompt_text);
            if !injected_prefix.is_empty() {
                final_content.insert(
                    0,
                    acp::ContentBlock::Text(acp::TextContent::new(injected_prefix)),
                );
            }
        }

        let (phase_before_submit, active_request_id_before_submit, queue_len_before_submit) = {
            let driver = self.session_driver.lock().await;
            (
                driver.phase_label(),
                driver.active_request_id(),
                driver.queue_len(),
            )
        };
        debug!(
            target: "acp_event_flow",
            event_id = id,
            phase_before_submit,
            active_request_id_before_submit = active_request_id_before_submit
                .as_deref()
                .unwrap_or("<none>"),
            queue_len_before_submit,
            prompt_text_len = final_prompt_text.len(),
            content_blocks = final_content.len(),
            "Accepted user prompt into ACP backend"
        );

        crate::normalized::session_runtime::QueuedPrompt {
            target_session: None,
            event_id: id.to_string(),
            kind: crate::normalized::session_runtime::QueuedPromptKind::User,
            text: final_prompt_text,
            prompt_meta,
            original_content,
            content: final_content,
            display_text: Some(prompt_text_for_hooks),
            reclaim: local_user_prompt.then_some(Box::new(
                crate::normalized::session_runtime::ReclaimRestore {
                    hook_context: inherited_hook_context,
                    compact_summary: pending_summary,
                },
            )),
        }
    }

    pub(super) async fn publish_user_input(
        &self,
        prompt_event_id: &str,
        display_text: Option<&str>,
        original_content: Vec<acp::ContentBlock>,
        prompt_meta: Option<&acp::Meta>,
    ) {
        if let Some(recorder) = self.transcript_recorder.read().await.clone()
            && let Some(display_text) = display_text
            && let Err(error) = recorder
                .record_user_message(prompt_event_id, display_text, vec![])
                .await
        {
            warn!("Failed to record user message to transcript: {error}");
        }

        let session_id = self.session_id.read().await.clone();
        let message_id = acp::MessageId::new(prompt_event_id.to_owned());
        let echo_meta = prompt_meta
            .map(|meta| {
                meta.iter()
                    .filter(|(key, _)| {
                        matches!(
                            key.as_str(),
                            nori_protocol::PROMPT_ECHO_ID_META_KEY | crate::follow_up::SENDER_META
                        )
                    })
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect::<acp::Meta>()
            })
            .filter(|meta| !meta.is_empty());
        for content in original_content {
            let notification = acp::SessionNotification::new(
                session_id.clone(),
                acp::SessionUpdate::UserMessageChunk(
                    acp::ContentChunk::new(content)
                        .message_id(message_id.clone())
                        .meta(echo_meta.clone()),
                ),
            );
            let _ = self
                .backend_event_tx
                .send(BackendEvent::Public(SessionEvent::Acp(
                    nori_protocol::AcpEvent::Notification(
                        acp::AgentNotification::SessionNotification(notification),
                    ),
                )))
                .await;
        }
    }
}
