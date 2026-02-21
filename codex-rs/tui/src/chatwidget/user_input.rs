use super::*;

impl ChatWidget {
    pub(super) fn flush_active_cell(&mut self) {
        if let Some(active) = self.active_cell.take() {
            // Check if this is an incomplete ExecCell that should be saved to pending
            // instead of being flushed to history. This prevents duplicate entries when
            // the ExecCommandEnd event arrives later.
            if let Some(exec_cell) = active.as_any().downcast_ref::<ExecCell>()
                && exec_cell.is_active()
            {
                // Get the pending call_ids before we consume the cell
                let pending_ids = exec_cell.pending_call_ids();
                if !pending_ids.is_empty() {
                    // Save to pending map with ALL pending call_ids
                    // This allows the cell to be retrieved when any of them completes
                    self.pending_exec_cells.save_pending(pending_ids, active);
                    return;
                }
            }
            // Normal flush path - cell is complete or not an ExecCell
            self.needs_final_message_separator = true;
            self.app_event_tx.send(AppEvent::InsertHistoryCell(active));
        }
    }

    pub(super) fn add_to_history(&mut self, cell: impl HistoryCell + 'static) {
        self.add_boxed_history(Box::new(cell));
    }

    pub(crate) fn add_boxed_history(&mut self, cell: Box<dyn HistoryCell>) {
        if !cell.display_lines(u16::MAX).is_empty() {
            // Only break exec grouping if the cell renders visible lines.
            // EXCEPT: Don't flush incomplete ExecCells - they should remain visible
            // in active_cell while streaming content is added to history.
            let should_flush = self
                .active_cell
                .as_ref()
                .map(|c| {
                    c.as_any()
                        .downcast_ref::<ExecCell>()
                        .map(|exec| !exec.is_active())
                        .unwrap_or(true)
                })
                .unwrap_or(true);
            if should_flush {
                self.flush_active_cell();
            }
            self.needs_final_message_separator = true;
        }
        self.app_event_tx.send(AppEvent::InsertHistoryCell(cell));
    }

    pub(super) fn queue_user_message(&mut self, user_message: UserMessage) {
        if self.bottom_pane.is_task_running() {
            self.queued_user_messages.push_back(user_message);
            self.refresh_queued_user_messages();
        } else {
            self.submit_user_message(user_message);
        }
    }

    pub(super) fn submit_user_message(&mut self, user_message: UserMessage) {
        let UserMessage { text, image_paths } = user_message;
        if text.is_empty() && image_paths.is_empty() {
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

        if self.first_prompt_text.is_none() {
            self.first_prompt_text = Some(text.clone());

            // Initialize loop mode on the very first prompt.
            // Use the ephemeral per-session override if set, otherwise fall
            // back to the persisted NoriConfig value.
            #[cfg(feature = "nori-config")]
            {
                let effective_loop_count = match self.loop_count_override {
                    Some(overridden) => overridden,
                    None => {
                        codex_acp::config::NoriConfig::load()
                            .unwrap_or_default()
                            .loop_count
                    }
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

        // Track user message for session statistics
        self.session_stats.record_user_message();

        // Refresh system info (including git branch) on user message submission.
        // This catches branch changes that happened between interactions
        // (e.g., user switched branches in another terminal).
        self.app_event_tx
            .send(AppEvent::RefreshSystemInfoForDirectory {
                dir: self.config.cwd.clone(),
                agent: Some(self.config.model.clone()),
            });

        // Check if there's a pending agent switch - if so, send the message through
        // the App to trigger the switch first
        if let Some(pending) = self.pending_agent.take() {
            self.app_event_tx.send(AppEvent::SubmitWithAgentSwitch {
                agent_name: pending.agent_name,
                display_name: pending.display_name,
                message_text: text,
                image_paths,
            });
            return;
        }

        let mut items: Vec<UserInput> = Vec::new();

        // Special-case: "!cmd" executes a local shell command instead of sending to the model.
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
            self.submit_op(Op::RunUserShellCommand {
                command: cmd.to_string(),
            });
            return;
        }

        if !text.is_empty() {
            items.push(UserInput::Text { text: text.clone() });
        }

        for path in image_paths {
            items.push(UserInput::LocalImage { path });
        }

        self.codex_op_tx
            .send(Op::UserInput { items })
            .unwrap_or_else(|e| {
                tracing::error!("failed to send message: {e}");
            });

        // Persist the text to cross-session message history.
        if !text.is_empty() {
            self.codex_op_tx
                .send(Op::AddToHistory { text: text.clone() })
                .unwrap_or_else(|e| {
                    tracing::error!("failed to send AddHistory op: {e}");
                });
        }

        // Only show the text portion in conversation history.
        if !text.is_empty() {
            self.add_to_history(history_cell::new_user_prompt(text));
        }
        self.needs_final_message_separator = false;
    }

    /// Replay a subset of initial events into the UI to seed the transcript when
    /// resuming an existing session. This approximates the live event flow and
    /// is intentionally conservative: only safe-to-replay items are rendered to
    /// avoid triggering side effects. Event ids are passed as `None` to
    /// distinguish replayed events from live ones.
    pub(super) fn replay_initial_messages(&mut self, events: Vec<EventMsg>) {
        for msg in events {
            if matches!(msg, EventMsg::SessionConfigured(_)) {
                continue;
            }
            // `id: None` indicates a synthetic/fake id coming from replay.
            self.dispatch_event_msg(None, msg, true);
        }
    }

    pub(crate) fn handle_codex_event(&mut self, event: Event) {
        let Event { id, msg } = event;

        // When expected_agent is set (during agent switching), we need to filter events
        // to prevent events from the OLD agent from affecting the NEW widget.
        if let Some(ref expected) = self.expected_agent {
            tracing::debug!(
                "Event filtering active: expected_agent={}, session_configured_received={}",
                expected,
                self.session_configured_received
            );
            if !self.session_configured_received {
                // Only process SessionConfigured events, and only if the model matches
                match &msg {
                    EventMsg::SessionConfigured(e) => {
                        if e.model.to_lowercase() != expected.to_lowercase() {
                            tracing::debug!(
                                "Ignoring SessionConfigured from wrong model: expected={}, got={}",
                                expected,
                                e.model
                            );
                            return;
                        }
                        tracing::debug!(
                            "SessionConfigured received with matching model: {}",
                            e.model
                        );
                        // Model matches, proceed with processing
                    }
                    // Ignore all other events until SessionConfigured arrives
                    _ => {
                        tracing::debug!(
                            "Ignoring event before SessionConfigured: {:?} (waiting for model={})",
                            std::mem::discriminant(&msg),
                            expected
                        );
                        return;
                    }
                }
            }
        }

        self.dispatch_event_msg(Some(id), msg, false);
    }

    /// Dispatch a protocol `EventMsg` to the appropriate handler.
    ///
    /// `id` is `Some` for live events and `None` for replayed events from
    /// `replay_initial_messages()`. Callers should treat `None` as a "fake" id
    /// that must not be used to correlate follow-up actions.
    pub(super) fn dispatch_event_msg(
        &mut self,
        id: Option<String>,
        msg: EventMsg,
        from_replay: bool,
    ) {
        match msg {
            EventMsg::AgentMessageDelta(_)
            | EventMsg::AgentReasoningDelta(_)
            | EventMsg::ExecCommandOutputDelta(_) => {}
            _ => {
                tracing::trace!("handle_codex_event: {:?}", msg);
            }
        }

        match msg {
            EventMsg::SessionConfigured(e) => self.on_session_configured(e),
            EventMsg::AgentMessage(AgentMessageEvent { message }) => self.on_agent_message(message),
            EventMsg::AgentMessageDelta(AgentMessageDeltaEvent { delta }) => {
                self.on_agent_message_delta(delta)
            }
            EventMsg::AgentReasoningDelta(AgentReasoningDeltaEvent { delta })
            | EventMsg::AgentReasoningRawContentDelta(AgentReasoningRawContentDeltaEvent {
                delta,
            }) => self.on_agent_reasoning_delta(delta),
            EventMsg::AgentReasoning(AgentReasoningEvent { .. }) => self.on_agent_reasoning_final(),
            EventMsg::AgentReasoningRawContent(AgentReasoningRawContentEvent { text }) => {
                self.on_agent_reasoning_delta(text);
                self.on_agent_reasoning_final()
            }
            EventMsg::AgentReasoningSectionBreak(_) => self.on_reasoning_section_break(),
            EventMsg::TaskStarted(_) => self.on_task_started(),
            EventMsg::TaskComplete(TaskCompleteEvent { last_agent_message }) => {
                self.on_task_complete(last_agent_message)
            }
            EventMsg::TokenCount(ev) => {
                self.set_token_info(ev.info);
                self.on_rate_limit_snapshot(ev.rate_limits);
            }
            EventMsg::Warning(WarningEvent { message }) => self.on_warning(message),
            EventMsg::Error(ErrorEvent { message, .. }) => self.on_error(message),
            EventMsg::McpStartupUpdate(ev) => self.on_mcp_startup_update(ev),
            EventMsg::McpStartupComplete(ev) => self.on_mcp_startup_complete(ev),
            EventMsg::TurnAborted(ev) => match ev.reason {
                TurnAbortReason::Interrupted => {
                    self.on_interrupted_turn(ev.reason);
                }
                TurnAbortReason::Replaced => {
                    self.on_error("Turn aborted: replaced by a new task".to_owned())
                }
            },
            EventMsg::PlanUpdate(update) => self.on_plan_update(update),
            EventMsg::ExecApprovalRequest(ev) => {
                // For replayed events, synthesize an empty id (these should not occur).
                self.on_exec_approval_request(id.unwrap_or_default(), ev)
            }
            EventMsg::ApplyPatchApprovalRequest(ev) => {
                self.on_apply_patch_approval_request(id.unwrap_or_default(), ev)
            }
            EventMsg::ElicitationRequest(ev) => {
                self.on_elicitation_request(ev);
            }
            EventMsg::ExecCommandBegin(ev) => self.on_exec_command_begin(ev),
            EventMsg::ExecCommandOutputDelta(delta) => self.on_exec_command_output_delta(delta),
            EventMsg::PatchApplyBegin(ev) => self.on_patch_apply_begin(ev),
            EventMsg::PatchApplyEnd(ev) => self.on_patch_apply_end(ev),
            EventMsg::ExecCommandEnd(ev) => self.on_exec_command_end(ev),
            EventMsg::ViewImageToolCall(ev) => self.on_view_image_tool_call(ev),
            EventMsg::McpToolCallBegin(ev) => self.on_mcp_tool_call_begin(ev),
            EventMsg::McpToolCallEnd(ev) => self.on_mcp_tool_call_end(ev),
            EventMsg::WebSearchBegin(ev) => self.on_web_search_begin(ev),
            EventMsg::WebSearchEnd(ev) => self.on_web_search_end(ev),
            EventMsg::GetHistoryEntryResponse(ev) => self.on_get_history_entry_response(ev),
            EventMsg::McpListToolsResponse(ev) => self.on_list_mcp_tools(ev),
            EventMsg::ListCustomPromptsResponse(ev) => self.on_list_custom_prompts(ev),
            EventMsg::ShutdownComplete => self.on_shutdown_complete(),
            EventMsg::TurnDiff(TurnDiffEvent { unified_diff }) => self.on_turn_diff(unified_diff),
            EventMsg::DeprecationNotice(ev) => self.on_deprecation_notice(ev),
            EventMsg::BackgroundEvent(BackgroundEventEvent { message }) => {
                self.on_background_event(message)
            }
            EventMsg::UndoStarted(ev) => self.on_undo_started(ev),
            EventMsg::UndoCompleted(ev) => self.on_undo_completed(ev),
            EventMsg::UndoListResult(ev) => self.on_undo_list_result(ev),
            EventMsg::StreamError(StreamErrorEvent { message, .. }) => {
                self.on_stream_error(message)
            }
            EventMsg::UserMessage(ev) => {
                if from_replay {
                    self.on_user_message_event(ev);
                }
            }
            EventMsg::ContextCompacted(event) => self.on_context_compacted(event),
            EventMsg::RawResponseItem(_)
            | EventMsg::ItemStarted(_)
            | EventMsg::ItemCompleted(_)
            | EventMsg::AgentMessageContentDelta(_)
            | EventMsg::ReasoningContentDelta(_)
            | EventMsg::ReasoningRawContentDelta(_) => {}
            EventMsg::PromptSummary(ev) => self.on_prompt_summary(ev.summary),
            EventMsg::HookOutput(HookOutputEvent { message, level }) => match level {
                HookOutputLevel::Info => {
                    self.add_plain_history_lines(vec![Line::from(message)]);
                }
                HookOutputLevel::Warn => {
                    self.on_warning(message);
                }
                HookOutputLevel::Error => {
                    self.add_error_message(message);
                }
            },
            EventMsg::SearchHistoryResponse(ev) => {
                self.bottom_pane.on_search_history_response(ev.entries);
                self.request_redraw();
            }
        }
    }

    pub(super) fn on_user_message_event(&mut self, event: UserMessageEvent) {
        let message = event.message.trim();
        if !message.is_empty() {
            self.add_to_history(history_cell::new_user_prompt(message.to_string()));
        }
    }

    pub(super) fn request_exit(&mut self) {
        // Clear the ctrl-c quit hint to make room for the exit message
        self.bottom_pane.clear_ctrl_c_quit_hint();
        self.request_redraw();

        // Send exit request - app.rs will handle adding the exit message cell before exiting
        self.app_event_tx.send(AppEvent::ExitRequest);
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
        if !self.config.tui_notifications {
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
            if let Some(exec) = cell.as_any_mut().downcast_mut::<ExecCell>() {
                exec.mark_failed();
            } else if let Some(tool) = cell.as_any_mut().downcast_mut::<McpToolCallCell>() {
                tool.mark_failed();
            }
            self.add_boxed_history(cell);
        }
    }

    // If idle and there are queued inputs, submit exactly one to start the next turn.
    pub(super) fn maybe_send_next_queued_input(&mut self) {
        if self.bottom_pane.is_task_running() {
            return;
        }
        if let Some(user_message) = self.queued_user_messages.pop_front() {
            self.submit_user_message(user_message);
        }
        // Update the list to reflect the remaining queued messages (if any).
        self.refresh_queued_user_messages();
    }

    /// Rebuild and update the queued user messages from the current queue.
    pub(super) fn refresh_queued_user_messages(&mut self) {
        let messages: Vec<String> = self
            .queued_user_messages
            .iter()
            .map(|m| m.text.clone())
            .collect();
        self.bottom_pane.set_queued_user_messages(messages);
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
        let context_window_percent = self.bottom_pane.context_window_percent();

        // Calculate approval mode label from config
        let approval_mode_label =
            approval_mode_label(self.config.approval_policy, &self.config.sandbox_policy);

        self.add_to_history(crate::nori::session_header::new_nori_status_output(
            &self.config.model,
            self.config.cwd.clone(),
            prompt_summary,
            approval_mode_label,
            token_breakdown,
            context_window_percent,
        ));
    }

    pub(super) fn stop_rate_limit_poller(&mut self) {
        if let Some(handle) = self.rate_limit_poller.take() {
            handle.abort();
        }
    }

    pub(super) fn prefetch_rate_limits(&mut self) {
        // Rate limit prefetching is not used in Nori (no backend-client)
    }
}
