use super::*;

impl AcpBackend {
    pub(crate) async fn cancel(&self) -> Result<()> {
        self.session_event_tx
            .send(session_runtime_driver::SessionRuntimeInput::Reducer(
                session_reducer::InboundEvent::CancelSubmit,
            ))
            .await
            .map_err(|_| anyhow::anyhow!("session runtime closed"))
    }

    pub(crate) async fn shutdown(&self) -> Result<()> {
        self.teardown(true).await;
        let _ = self
            .backend_event_tx
            .send(BackendEvent::Public(SessionEvent::Nori(
                nori_protocol::NoriEvent::SessionEnded(nori_protocol::SessionEnded {
                    reason: nori_protocol::SessionEndReason::Shutdown,
                    message: None,
                }),
            )))
            .await;
        Ok(())
    }

    pub(super) async fn teardown(&self, cancel_session: bool) {
        self.is_shutting_down
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(abort) = self.cancel_timeout_abort.lock().await.take() {
            abort.abort();
        }
        if let Some(abort) = self.prompt_task_abort.lock().await.take() {
            abort.abort();
        }
        if let Some(abort) = self.runtime_task_abort.lock().await.take() {
            abort.abort();
        }
        if cancel_session {
            let _ = self.connection.cancel(&*self.session_id.read().await).await;
        }

        if !self.session_end_hooks.is_empty() {
            let results =
                crate::hooks::execute_hooks(&self.session_end_hooks, self.script_timeout).await;
            route_hook_results(&results, &self.backend_event_tx, None).await;
        }
        if let Some(handle) = crate::hooks::execute_hooks_fire_and_forget(
            self.async_session_end_hooks.clone(),
            self.script_timeout,
            HashMap::new(),
        ) && let Err(error) = handle.await
        {
            warn!("Async session_end hook task panicked: {error}");
        }
        self.connection.shutdown().await;
        if cancel_session && let Some(abort) = self.relay_task_abort.lock().await.take() {
            abort.abort();
        }
    }

    pub(crate) async fn add_history(&self, text: String) -> Result<()> {
        crate::message_history::append_entry(
            &text,
            &self.conversation_id,
            &self.nori_home,
            self.history_persistence,
        )
        .await
        .map_err(Into::into)
    }

    pub(crate) async fn history_entry(
        &self,
        log_id: i64,
        offset: i64,
    ) -> Result<Option<crate::HistoryEntry>> {
        let log_id =
            u64::try_from(log_id).map_err(|_| anyhow::anyhow!("invalid history log id"))?;
        let offset =
            usize::try_from(offset).map_err(|_| anyhow::anyhow!("invalid history offset"))?;
        let nori_home = self.nori_home.clone();
        tokio::task::spawn_blocking(move || {
            crate::message_history::lookup(log_id, offset, &nori_home)
        })
        .await
        .map_err(Into::into)
    }

    pub(crate) async fn search_history(
        &self,
        max_results: i64,
    ) -> Result<Vec<crate::HistoryEntry>> {
        let max_results = usize::try_from(max_results)
            .map_err(|_| anyhow::anyhow!("invalid history result limit"))?;
        let nori_home = self.nori_home.clone();
        tokio::task::spawn_blocking(move || {
            crate::message_history::search_entries(&nori_home, max_results)
        })
        .await
        .map_err(Into::into)
    }

    pub(crate) async fn custom_prompts(&self) -> Vec<crate::CustomPrompt> {
        crate::custom_prompts::discover_prompts_in(&commands_dir(&self.nori_home)).await
    }

    pub(crate) async fn compact(&self) -> Result<()> {
        self.handle_compact(&generate_id()).await
    }

    pub(crate) async fn undo(&self) -> Result<()> {
        self.connection
            .cancel(&*self.session_id.read().await)
            .await
            .ok();
        crate::undo::handle_undo(
            &self.backend_event_tx,
            &generate_id(),
            &self.cwd,
            &self.ghost_snapshots,
        )
        .await;
        Ok(())
    }

    pub(crate) async fn undo_snapshots(&self) -> Vec<crate::UndoSnapshot> {
        self.ghost_snapshots.list().await
    }

    pub(crate) async fn undo_to(&self, index: i64) -> Result<()> {
        self.connection
            .cancel(&*self.session_id.read().await)
            .await
            .ok();
        crate::undo::handle_undo_to(
            &self.backend_event_tx,
            &generate_id(),
            &self.cwd,
            &self.ghost_snapshots,
            index,
        )
        .await;
        Ok(())
    }

    pub(crate) async fn run_user_shell(&self, command: String) -> Result<()> {
        let backend_event_tx = self.backend_event_tx.clone();
        let cwd = self.cwd.clone();
        let operation_id = generate_id();
        tokio::spawn(async move {
            user_shell::run_user_shell_command(&backend_event_tx, &operation_id, &cwd, command)
                .await;
        });
        Ok(())
    }

    pub(crate) fn set_approval_policy(&self, policy: nori_config::AskForApproval) {
        let _ = self.approval_policy_tx.send(policy);
    }

    pub(crate) async fn respond_to_agent(
        &self,
        request_id: acp::RequestId,
        response: Result<acp::ClientResponse, acp::Error>,
    ) -> Result<()> {
        let mut pending = self.pending_approvals.lock().await;
        let Some(index) = pending
            .iter()
            .position(|pending| pending.request.request_id == request_id)
        else {
            anyhow::bail!("no delegated ACP request is pending for {request_id}");
        };
        let pending = pending.remove(index);
        pending
            .request
            .response_tx
            .send(response)
            .map_err(|_| anyhow::anyhow!("ACP request responder closed"))
    }

    pub(crate) async fn submit_prompt(
        &self,
        content: Vec<acp::ContentBlock>,
        response_tx: oneshot::Sender<Result<acp::RequestId>>,
    ) {
        let has_content = content.iter().any(|block| match block {
            acp::ContentBlock::Text(text) => !text.text.is_empty(),
            _ => true,
        });
        if !has_content {
            let _ = response_tx.send(Err(anyhow::anyhow!("prompt content is empty")));
            return;
        }

        let id = generate_id();
        self.pending_prompt_submissions
            .lock()
            .await
            .insert(id.clone(), response_tx);
        if let Err(error) = self.handle_prompt(content, &id).await
            && let Some(response_tx) = self.pending_prompt_submissions.lock().await.remove(&id)
        {
            let _ = response_tx.send(Err(error));
        }
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
        use crate::compact::SUMMARIZATION_PROMPT;

        let _ = self
            .session_event_tx
            .send(session_runtime_driver::SessionRuntimeInput::Reducer(
                session_reducer::InboundEvent::PromptSubmit(
                    crate::normalized::session_runtime::QueuedPrompt {
                        event_id: id.to_string(),
                        kind: crate::normalized::session_runtime::QueuedPromptKind::Compact,
                        text: SUMMARIZATION_PROMPT.to_string(),
                        content: vec![acp::ContentBlock::Text(acp::TextContent::new(
                            SUMMARIZATION_PROMPT,
                        ))],
                        display_text: None,
                    },
                ),
            ))
            .await;

        Ok(())
    }

    /// Get the current ACP session config snapshot from the connection.
    pub fn config_options(&self) -> Vec<acp::SessionConfigOption> {
        self.connection.config_options()
    }

    /// Get the current session ID.
    ///
    /// Note: This clones the session ID since it may be replaced during /compact.
    pub async fn session_id(&self) -> acp::SessionId {
        self.session_id.read().await.clone()
    }

    /// Get a reference to the underlying ACP connection.
    ///
    /// This provides access to low-level ACP operations like session controls.
    pub fn connection(&self) -> &Arc<AcpConnection> {
        &self.connection
    }

    /// Set an ACP session config option for the current session.
    pub async fn set_config_option(
        &self,
        config_id: impl Into<acp::SessionConfigId>,
        value: impl Into<acp::SessionConfigValueId>,
    ) -> Result<()> {
        let session_id = self.session_id.read().await;
        self.connection
            .set_config_option(&session_id, config_id, value)
            .await
    }
}
