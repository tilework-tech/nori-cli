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
        // The app is exiting: teardown has started, so a prompt submitted by
        // a fast typist must not start another turn.
        if self.exiting {
            return;
        }
        if user_message.image_paths.is_empty()
            && let Some(request) =
                crate::remote_control::parse_remote_control_request(&user_message.text)
        {
            match request {
                Ok(request) => self
                    .app_event_tx
                    .send(AppEvent::RemoteControlRequested(request)),
                Err(message) => self.add_error_message(message),
            }
            return;
        }
        let UserMessage { text, image_paths } = user_message;
        if text.is_empty() && image_paths.is_empty() {
            return;
        }

        if image_paths.is_empty() && self.handle_goal_user_message(&text) {
            self.persist_prompt_history(&text);
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

        // Followers may use local commands, but must never submit a prompt to
        // the Slack-owned ACP session, including through programmatic callers.
        if self.follow_only_attachment {
            return;
        }

        if self.harness_handle.is_none() {
            if text.starts_with('/') {
                self.add_error_message(
                    "No active session — pick one with /resume or start one with /new.".to_string(),
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

        self.submit_prompt(content);

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

    pub(crate) fn add_status_output(&mut self) {
        // Get optional status card fields from bottom_pane
        let prompt_summary = self.bottom_pane.prompt_summary();
        let token_breakdown = self.bottom_pane.transcript_token_breakdown();
        let status_info = self.bottom_pane.status_card_info();

        // Calculate approval mode label from config
        let approval_mode_label =
            approval_mode_label(self.config.approval_policy, &self.config.sandbox_policy);

        self.add_to_history(crate::nori::session_header::new_nori_status_output(
            &self.config.active_agent,
            self.config.cwd.clone(),
            prompt_summary,
            approval_mode_label,
            token_breakdown,
            self.cloud_session_identity(),
            self.conversation_id(),
            self.forked_from,
            status_info,
        ));
    }
}
