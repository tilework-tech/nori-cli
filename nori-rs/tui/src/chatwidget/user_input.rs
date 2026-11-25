use super::*;

fn local_image_content(
    path: &std::path::Path,
) -> Result<nori_protocol::acp::v1::ContentBlock, String> {
    use base64::Engine as _;

    let bytes = std::fs::read(path)
        .map_err(|error| format!("Failed to read image file {}: {error}", path.display()))?;
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    let mime_type = match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "image/png",
    };
    Ok(nori_protocol::acp::v1::ContentBlock::Image(
        nori_protocol::acp::v1::ImageContent::new(data, mime_type),
    ))
}

/// A joined host with no session on screen has nothing to prompt.
const NO_HOST_SESSION: &str = "No session is open on this host. Pick one with /sessions.";

impl ChatWidget {
    pub(super) fn flush_active_cell(&mut self) {
        self.active_user_message_id = None;
        if let Some(active) = self.active_cell.take() {
            let is_user_message = active.as_any().is::<history_cell::UserHistoryCell>();
            if let Some(client_cell) = active.as_any().downcast_ref::<ClientToolCell>() {
                if client_cell.is_active() {
                    self.completed_client_tool_calls
                        .insert(client_cell.call_id().to_owned());
                }
                // Track all exploring group call_ids so completions arriving
                // after flush don't get re-merged into a later exploring cell.
                for id in client_cell.exploring_call_ids() {
                    self.completed_client_tool_calls.insert(id);
                }
            }
            self.needs_final_message_separator = !is_user_message;
            self.app_event_tx.send(AppEvent::InsertHistoryCell(active));
        }
    }

    pub(super) fn add_to_history(&mut self, cell: impl HistoryCell + 'static) {
        self.add_boxed_history(Box::new(cell));
    }

    pub(crate) fn add_boxed_history(&mut self, cell: Box<dyn HistoryCell>) {
        // A pending thought block precedes whatever ends it.
        if !self.full_reasoning_buffer.is_empty() {
            self.flush_thought_block();
        }
        if !cell.display_lines(u16::MAX).is_empty() {
            // Always flush active cell before inserting new history to preserve
            // chronological ordering.
            self.flush_active_cell();
            if !cell.as_any().is::<history_cell::AgentMessageCell>() {
                self.needs_final_message_separator = true;
            }
        }
        self.app_event_tx.send(AppEvent::InsertHistoryCell(cell));
    }

    pub(crate) fn submit_user_message_text(&mut self, text: String) {
        self.submit_user_message(UserMessage {
            text,
            image_paths: Vec::new(),
        });
    }

    pub(super) fn submit_user_message(&mut self, user_message: UserMessage) {
        self.submit_composed_message(user_message, crate::follow_up::Delivery::Default);
    }

    pub(super) fn submit_composed_message(
        &mut self,
        user_message: UserMessage,
        delivery: crate::follow_up::Delivery,
    ) {
        // The app is exiting: teardown has started, so a prompt submitted by
        // a fast typist must not start another turn.
        if self.exiting {
            return;
        }
        let UserMessage { text, image_paths } = user_message;
        if text.is_empty() && image_paths.is_empty() {
            return;
        }

        if image_paths.is_empty() && self.handle_delegate_command(&text) {
            return;
        }

        if image_paths.is_empty() && self.handle_goal_user_message(&text) {
            self.persist_prompt_history(&text);
            return;
        }

        if image_paths.is_empty() && self.handle_new_in_folder(&text) {
            return;
        }

        // Special-case: "/login <agent>" triggers login for a specific agent
        // This intercepts before the message is sent to the agent
        if let Some(agent_name) = text.strip_prefix("/login ").map(str::trim)
            && !agent_name.is_empty()
        {
            self.handle_login_command_with_agent(agent_name);
            return;
        }

        // Special-case: "/switch-skillset <name>" directly switches to the named skillset
        // without showing the picker menu
        if let Some(skillset_name) = text.strip_prefix("/switch-skillset ").map(str::trim)
            && !skillset_name.is_empty()
        {
            self.handle_switch_skillset_command_with_name(skillset_name);
            return;
        }

        // Local shell input is never a reason to claim an ACP session.
        if let Some(stripped) = text.strip_prefix('!') {
            let cmd = stripped.trim();
            if cmd.is_empty() {
                self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                    history_cell::new_info_event(
                        USER_SHELL_COMMAND_HELP_TITLE.to_string(),
                        Some(USER_SHELL_COMMAND_HELP_HINT.to_string()),
                    ),
                )));
                return;
            }
            self.submit_harness_action(crate::app_event::HarnessAction::RunUserShell(
                cmd.to_string(),
            ));
            return;
        }

        if self.delegated_child
            && self
                .managed_conversation_id
                .and_then(|id| self.session_manager.as_ref()?.get(&id))
                .is_some_and(|record| {
                    record.status.is_closed()
                        || record.status == nori_protocol::SessionStatus::Failed
                })
        {
            self.add_error_message("This child session has ended. Its history remains available; return to its parent or start /new.".into());
            return;
        }

        // Followers may use local commands, but must never submit a prompt to
        // the Slack-owned ACP session, including through programmatic callers.
        if self.follow_only_attachment {
            return;
        }

        if self.host_connection.is_some() && self.managed_conversation_id.is_none() {
            self.add_error_message(NO_HOST_SESSION.to_string());
            return;
        }

        if !self.has_harness_session() {
            if text.starts_with('/') {
                self.add_error_message(
                    "No active session - pick one with /resume or start one with /new.".to_string(),
                );
                return;
            }
            if self.initial_user_message.is_some() {
                self.add_info_message(
                    "The first prompt is already waiting for the agent to finish preparing."
                        .to_string(),
                    None,
                );
                return;
            }
            if self.first_prompt_text.is_none() && !text.is_empty() {
                self.first_prompt_text = Some(text.clone());
            }
            self.initial_user_message = Some(UserMessage { text, image_paths });
            self.app_event_tx.send(AppEvent::NewSession);
            return;
        }

        if self.first_prompt_text.is_none() {
            self.first_prompt_text = Some(text.clone());

            // Initialize loop mode on the very first prompt.
            // Use the ephemeral per-session override if set, otherwise fall
            // back to the persisted NoriConfig value.
            {
                let effective_loop_count = match self.loop_count_override {
                    Some(overridden) => overridden,
                    None => self.config.loop_count,
                };
                if let Some(count) = effective_loop_count
                    && count > 1
                {
                    self.loop_remaining = Some(count - 1);
                    self.loop_total = Some(count);
                    self.add_info_message(format!("Loop mode: will run {count} iterations."), None);
                }
            }
        }

        // Refresh system info (including git branch) on user message submission.
        // This catches branch changes that happened between interactions
        // (e.g., user switched branches in another terminal).
        self.app_event_tx
            .send(AppEvent::RefreshSystemInfoForDirectory {
                dir: self.config.cwd.clone(),
                agent: Some(self.config.active_agent.clone()),
            });

        let mut content = Vec::new();
        if !text.is_empty() {
            content.push(nori_protocol::acp::v1::ContentBlock::Text(
                nori_protocol::acp::v1::TextContent::new(text.clone()),
            ));
        }

        for path in image_paths {
            match local_image_content(&path) {
                Ok(image) => content.push(image),
                Err(error) => {
                    self.add_error_message(error);
                    return;
                }
            }
        }

        // A joined session has no harness to prompt; the host starts the turn.
        let delivery = match delivery {
            crate::follow_up::Delivery::Default if self.follows_host_session() => {
                crate::follow_up::Delivery::IfIdle
            }
            delivery => delivery,
        };
        match delivery {
            crate::follow_up::Delivery::FollowUp { turn, intent } => {
                let Some(session) = self.managed_conversation_id else {
                    self.add_error_message("No active host session".into());
                    return;
                };
                let id = uuid::Uuid::new_v4().to_string();
                let receipt =
                    self.app_event_tx
                        .host_receipt(crate::host_client::HostCall::FollowUp {
                            session_id: session.to_string(),
                            expected_turn: turn,
                            message_id: id.clone(),
                            intent,
                            prompt: content,
                        });
                let tx = self.app_event_tx.clone();
                let submitted = text.clone();
                tokio::spawn(async move {
                    match receipt.await.and_then(|value| {
                        serde_json::from_value::<nori_harness::FollowUpReceipt>(value)
                            .map_err(Into::into)
                    }) {
                        Ok(receipt) => tx.send(AppEvent::InsertHistoryCell(Box::new(
                            history_cell::new_info_event(
                                crate::follow_up::delivery_notice(receipt.status, &id, &submitted),
                                None,
                            ),
                        ))),
                        Err(error) => tx.send(AppEvent::HarnessActionFailed(format!(
                            "{error} [{id}] {submitted}"
                        ))),
                    }
                });
            }
            crate::follow_up::Delivery::IfIdle => {
                let Some(session) = self.managed_conversation_id else {
                    self.add_error_message("No active host session".into());
                    return;
                };
                let handle = self.harness_handle.clone();
                let interrupt = crate::host_client::PromptInterrupt::new(session.to_string());
                crate::host_client::PromptInterrupt::track_first(
                    &mut self.submitted_prompt,
                    &interrupt,
                );
                let admitted =
                    self.app_event_tx
                        .host_receipt(crate::host_client::HostCall::Prompt(
                            serde_json::json!({"session_id":session, "prompt":content}),
                        ));
                let tx = self.app_event_tx.clone();
                let submitted = text.clone();
                tokio::spawn(async move {
                    match admitted.await {
                        Ok(receipt) => {
                            if let Some(handle) = handle {
                                handle.record_host_prompt_started();
                            }
                            if let Some(call) = interrupt.admitted(&receipt) {
                                tx.send(AppEvent::HostRequest {
                                    call,
                                    reply: crate::app_event_sender::HostReply::ReportError(
                                        tx.clone(),
                                    ),
                                });
                            }
                        }
                        Err(error) => {
                            interrupt.retire();
                            tx.send(AppEvent::HarnessActionFailed(format!(
                                "Could not submit “{submitted}”: {error}"
                            )));
                        }
                    }
                });
            }
            crate::follow_up::Delivery::Default => self.submit_prompt(content),
        }

        // Persist the text to cross-session message history.
        self.persist_prompt_history(&text);
    }

    fn persist_prompt_history(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        self.submit_harness_action(crate::app_event::HarnessAction::AddHistory(
            text.to_string(),
        ));
    }

    /// Create an exit message cell with session statistics.
    /// Called by app.rs before exiting to display final session summary.
    pub(crate) fn create_exit_message_cell(&self) -> Box<dyn HistoryCell> {
        use crate::nori::exit_message::ExitMessageCell;

        let session_id = self
            .conversation_id()
            .map(|id| id.to_string())
            .unwrap_or_else(|| "(no session)".to_string());

        let stats = self.session_stats().clone();

        Box::new(ExitMessageCell::new(session_id, stats))
    }

    pub(super) fn request_redraw(&mut self) {
        self.frame_requester.schedule_frame();
    }

    pub(super) fn notify(&mut self, notification: Notification) {
        if self.config.terminal_notifications != nori_config::TerminalNotifications::Enabled {
            return;
        }
        self.pending_notification = Some(notification);
        self.request_redraw();
    }

    /// Take the notification this widget wants to post, as display text.
    pub(crate) fn take_pending_notification(&mut self) -> Option<String> {
        self.pending_notification
            .take()
            .map(|notification| notification.display())
    }

    pub(crate) fn maybe_post_pending_notification(&mut self, tui: &mut crate::tui::Tui) {
        if let Some(notif) = self.pending_notification.take() {
            tui.notify(notif.display());
        }
    }

    /// Mark the active cell as failed (✗) and flush it into history.
    pub(super) fn finalize_active_cell_as_failed(&mut self) {
        if let Some(mut cell) = self.active_cell.take() {
            // Insert finalized cell into history and keep grouping consistent.
            if let Some(client) = cell.as_any_mut().downcast_mut::<ClientToolCell>() {
                client.mark_failed();
            }
            self.add_boxed_history(cell);
        }
    }

    pub(crate) fn add_diff_in_progress(&mut self) {
        self.request_redraw();
    }

    pub(crate) fn on_diff_complete(&mut self) {
        self.request_redraw();
    }
}
