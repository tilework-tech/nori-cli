use super::*;
use crate::client_tool_cell::ClientToolCell;
use crate::ui_types::PlanItem;
use crate::ui_types::PlanUpdate;
use crate::ui_types::StepStatus;

impl ChatWidget {
    pub(crate) fn handle_session_event(
        &mut self,
        generation: crate::app_event::SessionGeneration,
        event: nori_protocol::SessionEvent,
    ) {
        if generation != self.session_generation {
            return;
        }
        match event {
            nori_protocol::SessionEvent::Acp(event) => self.handle_acp_event(event),
            nori_protocol::SessionEvent::Nori(event) => self.handle_nori_event(event),
        }
    }

    fn handle_acp_event(&mut self, event: nori_protocol::AcpEvent) {
        match event {
            nori_protocol::AcpEvent::Notification(
                nori_protocol::acp::v1::AgentNotification::SessionNotification(notification),
            ) => {
                // Classify live proactive activity before projecting the same update for display.
                if self.replay_source.is_none() {
                    self.handle_proactive_session_update(&notification.update);
                }
                let events = self
                    .client_event_normalizer
                    .push_session_update(&notification.update);
                for event in events {
                    if self.replay_source.is_some() {
                        self.handle_replay_client_event(event);
                    } else {
                        self.handle_client_event(event);
                    }
                }
            }
            nori_protocol::AcpEvent::Request {
                request_id,
                request: nori_protocol::acp::v1::AgentRequest::RequestPermissionRequest(request),
            } => {
                for event in self
                    .client_event_normalizer
                    .push_permission_request(&request)
                {
                    if let crate::presentation::ClientEvent::ApprovalRequest(approval) = event {
                        self.handle_client_approval_request(
                            approval,
                            request_id.clone(),
                            request.options.clone(),
                        );
                    }
                }
            }
            nori_protocol::AcpEvent::Response {
                response: Ok(nori_protocol::acp::v1::AgentResponse::InitializeResponse(response)),
                ..
            } => {
                self.session_agent_info = response.agent_info;
                self.session_info_state.reset();
                self.bottom_pane.set_session_title(None);
                let capabilities = response.agent_capabilities;
                self.session_agent_capabilities = crate::presentation::AgentCapabilitiesView {
                    http_mcp: capabilities.mcp_capabilities.http,
                    load_session: capabilities.load_session,
                    session_list: capabilities.session_capabilities.list.is_some(),
                    session_resume: capabilities.session_capabilities.resume.is_some(),
                    session_close: capabilities.session_capabilities.close.is_some(),
                    session_fork: capabilities.session_capabilities.fork.is_some(),
                };
            }
            nori_protocol::AcpEvent::Response {
                request_id,
                response: Ok(nori_protocol::acp::v1::AgentResponse::PromptResponse(response)),
            } if self.owned_prompt_request_id.as_ref() == Some(&request_id) => {
                self.owned_prompt_request_id = None;
                self.handle_client_prompt_completed(crate::presentation::PromptCompleted {
                    stop_reason: response.stop_reason,
                    last_agent_message: None,
                    failure: None,
                });
            }
            nori_protocol::AcpEvent::Response {
                request_id,
                response: Err(error),
            } => {
                if self.owned_prompt_request_id.as_ref() == Some(&request_id) {
                    if !self.unpaired_prompt_error_ids.remove(&request_id) {
                        self.unpaired_prompt_error_ids.insert(request_id);
                    }
                } else if !self.unpaired_prompt_error_ids.remove(&request_id) {
                    self.on_error(error.to_string());
                }
            }
            nori_protocol::AcpEvent::Request { .. }
            | nori_protocol::AcpEvent::Response { .. }
            | nori_protocol::AcpEvent::Notification(_) => {}
        }
    }

    fn handle_proactive_session_update(&mut self, update: &nori_protocol::acp::v1::SessionUpdate) {
        // Known Nori statuses refine presentation only when no local request owns the turn.
        if let nori_protocol::acp::v1::SessionUpdate::SessionInfoUpdate(info) = update
            && let Some(status) = crate::presentation::nori_turn_status(info)
        {
            if self.locally_owned_request_active() {
                return;
            }
            match status {
                crate::presentation::NoriTurnStatus::Working => self.start_proactive_turn(),
                crate::presentation::NoriTurnStatus::Idle => self.complete_proactive_turn(),
            }
            return;
        }

        // Any unowned turn content is valid proactive activity, even without status hints.
        if !self.locally_owned_request_active() && is_turn_content_update(update) {
            self.start_proactive_turn();
        }
    }

    fn locally_owned_request_active(&self) -> bool {
        self.owned_prompt_request_id.is_some()
            || matches!(
                self.acp_session_phase,
                Some(
                    crate::presentation::session_runtime::SessionPhaseView::Loading
                        | crate::presentation::session_runtime::SessionPhaseView::Prompt
                        | crate::presentation::session_runtime::SessionPhaseView::Cancelling
                )
            )
    }

    fn start_proactive_turn(&mut self) {
        if !self.proactive_turn_active {
            self.proactive_turn_active = true;
            self.start_task_presentation(false);
        }
    }

    fn separate_proactive_turn(&mut self) {
        self.record_proactive_assistant_message();
        self.proactive_turn_active = false;
        self.flush_answer_stream_with_separator();
    }

    fn complete_proactive_turn(&mut self) {
        if self.proactive_turn_active {
            self.record_proactive_assistant_message();
            self.proactive_turn_active = false;
            self.finish_task_presentation(None, true);
        }
    }

    fn record_proactive_assistant_message(&mut self) {
        if self.assistant_stream_seen_for_stats {
            self.session_stats.record_assistant_message();
        }
        self.assistant_stream_seen_for_stats = false;
    }

    fn handle_nori_event(&mut self, event: nori_protocol::NoriEvent) {
        match event {
            nori_protocol::NoriEvent::SessionStarted(started) => {
                self.on_session_started(started);
            }
            nori_protocol::NoriEvent::SessionPhaseChanged(phase) => {
                // A local request boundary separates statusless proactive output without completing it.
                if self.proactive_turn_active
                    && matches!(
                        phase,
                        nori_protocol::SessionPhase::Loading { .. }
                            | nori_protocol::SessionPhase::Prompting { .. }
                    )
                {
                    self.separate_proactive_turn();
                }
                let phase = match phase {
                    nori_protocol::SessionPhase::Idle => {
                        crate::presentation::session_runtime::SessionPhaseView::Idle
                    }
                    nori_protocol::SessionPhase::Loading { .. } => {
                        crate::presentation::session_runtime::SessionPhaseView::Loading
                    }
                    nori_protocol::SessionPhase::Prompting { request_id } => {
                        self.owned_prompt_request_id = Some(request_id);
                        crate::presentation::session_runtime::SessionPhaseView::Prompt
                    }
                    nori_protocol::SessionPhase::Cancelling { request_id } => {
                        self.owned_prompt_request_id = Some(request_id);
                        crate::presentation::session_runtime::SessionPhaseView::Cancelling
                    }
                };
                self.handle_client_phase_changed(phase);
            }
            nori_protocol::NoriEvent::SessionEnded(ended) => match ended.reason {
                nori_protocol::SessionEndReason::Shutdown => {
                    self.app_event_tx.send(AppEvent::ExitRequest);
                }
                nori_protocol::SessionEndReason::Closed => {
                    self.app_event_tx.send(AppEvent::SessionClosed);
                }
                nori_protocol::SessionEndReason::ConnectionLost
                | nori_protocol::SessionEndReason::SpawnFailed
                | nori_protocol::SessionEndReason::TimedOut => {}
            },
            nori_protocol::NoriEvent::QueueChanged(queue) => {
                self.bottom_pane.set_queued_user_messages(queue.prompts);
                self.request_redraw();
            }
            nori_protocol::NoriEvent::ContextCompacted(compacted) => {
                self.on_context_compacted(compacted.summary);
            }
            nori_protocol::NoriEvent::GoalChanged(Some(goal)) => {
                self.handle_thread_goal_updated(goal);
            }
            nori_protocol::NoriEvent::GoalChanged(None) => self.handle_thread_goal_cleared(),
            nori_protocol::NoriEvent::CapabilitiesChanged(capabilities) => {
                self.handle_session_capabilities_changed(
                    crate::presentation::SessionCapabilitiesView {
                        agent: self.session_agent_capabilities.clone(),
                        nori_client: Default::default(),
                        builtin_commands: capabilities.builtin_commands,
                    },
                );
            }
            nori_protocol::NoriEvent::PromptSummaryUpdated(summary) => {
                self.on_prompt_summary(summary.summary);
            }
            nori_protocol::NoriEvent::Notice(notice) => self.on_warning(notice.message),
            nori_protocol::NoriEvent::RequestFailed(failure) => {
                let completes_active_prompt =
                    failure.request_id.as_ref().is_some_and(|request_id| {
                        self.owned_prompt_request_id.as_ref() == Some(request_id)
                    });
                if completes_active_prompt
                    && let Some(request_id) = failure.request_id.as_ref()
                    && !self.unpaired_prompt_error_ids.remove(request_id)
                {
                    self.unpaired_prompt_error_ids.insert(request_id.clone());
                }
                if completes_active_prompt {
                    self.on_error(failure.message);
                    self.owned_prompt_request_id = None;
                    let failure = match failure.kind {
                        nori_protocol::RequestFailureKind::Retryable => {
                            crate::presentation::TurnFailure::Retryable
                        }
                        nori_protocol::RequestFailureKind::Fatal => {
                            crate::presentation::TurnFailure::Fatal
                        }
                    };
                    self.handle_client_prompt_completed(crate::presentation::PromptCompleted {
                        stop_reason: nori_protocol::acp::v1::StopReason::Cancelled,
                        last_agent_message: None,
                        failure: Some(failure),
                    });
                } else {
                    self.add_error_message(failure.message);
                }
            }
            nori_protocol::NoriEvent::HookOutput(output) => match output.level {
                nori_protocol::HookOutputLevel::Info => self.add_info_message(output.message, None),
                nori_protocol::HookOutputLevel::Warn => self.on_warning(output.message),
                nori_protocol::HookOutputLevel::Error => self.on_error(output.message),
            },
            nori_protocol::NoriEvent::ReplayStarted(started) => {
                self.flush_answer_stream_with_separator();
                self.flush_replay_message();
                self.replay_source = Some(started.source);
            }
            nori_protocol::NoriEvent::ReplayFinished => {
                self.flush_replay_message();
                self.replay_source = None;
            }
            nori_protocol::NoriEvent::Undo(_) | nori_protocol::NoriEvent::UserShell(_) => {}
            // TODO(#557): render forked-transcript lineage in the TUI history.
            nori_protocol::NoriEvent::SessionForked(forked) => {
                self.on_session_forked(forked);
            }
        }
    }

    fn on_session_forked(&mut self, forked: nori_protocol::SessionForked) {
        // The active conversation is now the forked child; the parent stays
        // resumable and is surfaced as `forked from:` on the status card.
        self.conversation_id =
            nori_harness::ConversationId::from_string(&forked.new_conversation_id).ok();
        self.forked_from =
            nori_harness::ConversationId::from_string(&forked.previous_conversation_id).ok();

        let mut lines: Vec<Line<'static>> = vec!["Session forked. To resume previous:".into()];
        if let Some(previous) = &self.forked_from {
            lines.push(
                crate::resume_command_for_conversation(previous)
                    .cyan()
                    .into(),
            );
        }
        self.add_plain_history_lines(lines);
    }

    fn on_session_started(&mut self, event: nori_protocol::SessionStarted) {
        self.session_configured_received = true;
        self.submit_harness_action(crate::app_event::HarnessAction::LoadCustomPrompts);
        self.bottom_pane.hide_status_indicator();
        self.update_approval_mode_label();
        self.bottom_pane.set_history_metadata(
            u64::try_from(event.history_log_id).unwrap_or_default(),
            usize::try_from(event.history_entry_count).unwrap_or_default(),
        );
        self.conversation_id = event
            .transcript_id
            .as_deref()
            .and_then(|id| nori_harness::ConversationId::from_string(id).ok());
        self.current_rollout_path = event.transcript_path;
        self.acp_session_id = self.cloud_mode.then(|| event.acp_session_id.to_string());
        self.refresh_cloud_session_indicator();
        self.add_to_history(history_cell::new_session_info(
            &self.config,
            self.config.active_agent.clone(),
            self.show_welcome_banner,
            self.cloud_session_identity(),
        ));
        if let Some(user_message) = self.initial_user_message.take() {
            self.submit_user_message(user_message);
        }
        if !self.suppress_session_configured_redraw {
            self.request_redraw();
        }
        self.refresh_acp_mode_config_snapshot();
        self.refresh_terminal_title();
    }

    pub(super) fn flush_answer_stream_with_separator(&mut self) {
        if let Some(mut controller) = self.stream_controller.take()
            && let Some(cell) = controller.finalize()
        {
            self.add_boxed_history(cell);
        }
    }

    pub(super) fn set_status_header(&mut self, header: String) {
        self.current_status_header = header.clone();
        self.bottom_pane.update_status_header(header);
    }

    pub(super) fn on_context_compacted(&mut self, summary: Option<String>) {
        // Step 1: Flush the streamed summary from the old session.
        self.flush_answer_stream_with_separator();
        self.pending_client_tool_cells.clear();

        // Step 2: Show "Context compacted" as an info message.
        self.add_info_message("Context compacted".to_owned(), None);

        // When the ACP backend provides a summary, show a session header
        // followed by the summary reprinted as the first assistant message
        // of the new session. This makes the session boundary visible.
        if let Some(summary) = summary {
            // Step 3: Insert a new session header (same card as a fresh session,
            // but without install hints since this is not the first launch).
            use crate::nori::session_header::DisplayMode;
            use crate::nori::session_header::NoriSessionHeaderCell;
            let header = NoriSessionHeaderCell::new(
                self.config.active_agent.clone(),
                self.config.cwd.clone(),
            )
            .with_display_mode(DisplayMode::Compact);
            self.add_to_history(history_cell::SessionInfoCell::new(
                history_cell::CompositeHistoryCell::new(vec![Box::new(header)]),
            ));

            // Step 4: Reprint the summary as the first assistant message of the
            // new session.
            self.handle_streaming_delta(summary);
            self.flush_answer_stream_with_separator();
        }

        self.request_redraw();
    }

    pub(super) fn on_agent_message_delta(&mut self, delta: String) {
        self.handle_streaming_delta(delta);
    }

    pub(super) fn on_agent_reasoning_delta(&mut self, delta: String) {
        self.flush_answer_stream_with_separator();

        // For reasoning deltas, do not stream to history. Accumulate the
        // current reasoning block and extract the first bold element
        // (between **/**) as the chunk header. Show this header as status.
        self.reasoning_buffer.push_str(&delta);

        if let Some(header) = extract_first_bold(&self.reasoning_buffer) {
            // Update the shimmer header to the extracted reasoning chunk header.
            self.set_status_header(header);
        } else {
            // Fallback while we don't yet have a bold header: leave existing header as-is.
        }
        self.request_redraw();
    }

    // Raw reasoning uses the same flow as summarized reasoning

    pub(super) fn on_task_started(&mut self) {
        self.start_task_presentation(true);
    }

    fn start_task_presentation(&mut self, owned: bool) {
        self.bottom_pane.clear_ctrl_c_quit_hint();
        // Owned requests enable task controls; proactive work gets display-only status.
        if owned {
            self.bottom_pane.set_task_running(true);
        } else {
            self.bottom_pane.ensure_status_indicator();
        }
        self.bottom_pane.set_interrupt_hint_visible(owned);
        self.set_status_header(crate::status_indicator_widget::pick_status_message(
            self.config.custom_working_messages,
            &self.config.custom_working_message_list,
        ));
        self.full_reasoning_buffer.clear();
        self.reasoning_buffer.clear();
        self.completed_client_tool_calls.clear();
        self.assistant_stream_seen_for_stats = false;
        self.request_redraw();
        self.refresh_terminal_title();
    }

    pub(super) fn on_task_complete(&mut self, last_agent_message: Option<String>) {
        self.finish_task_presentation(last_agent_message, true);

        // Loop mode: if iterations remain, fire the next iteration.
        if let Some(remaining) = self.loop_remaining
            && remaining > 0
            && let Some(prompt) = self.first_prompt_text.clone()
        {
            let total = self.loop_total.unwrap_or(0);
            self.app_event_tx.send(AppEvent::LoopIteration {
                prompt,
                remaining: remaining - 1,
                total,
            });
        }
    }

    fn finish_task_presentation(&mut self, last_agent_message: Option<String>, notify: bool) {
        // If a stream is currently active, finalize it.
        self.flush_answer_stream_with_separator();

        self.pending_client_tool_cells.clear();
        self.finalize_active_cell_as_failed();

        // Mark task stopped and request redraw now that all content is in history.
        self.bottom_pane.set_task_running(false);
        self.completed_client_tool_calls.clear();
        self.request_redraw();
        self.refresh_terminal_title();

        // Refresh system info (including git branch) on task completion.
        // This catches any branch changes that occurred during the agent's turn.
        self.app_event_tx
            .send(AppEvent::RefreshSystemInfoForDirectory {
                dir: self.config.cwd.clone(),
                agent: Some(self.config.active_agent.clone()),
            });

        if notify {
            // Emit a notification when the turn completes (suppressed if focused).
            self.notify(Notification::AgentTurnComplete {
                response: last_agent_message.unwrap_or_default(),
            });
        }
    }

    /// Finalize any active tool as failed and stop/clear running UI state.
    pub(super) fn finalize_turn(&mut self) {
        self.finalize_active_cell_as_failed();
        self.pending_client_tool_cells.clear();
        self.bottom_pane.set_task_running(false);
        self.completed_client_tool_calls.clear();
        self.stream_controller = None;
    }

    pub(super) fn on_error(&mut self, message: String) {
        // Display only. Loop lifecycle is owned by the prompt completion
        // (`handle_client_prompt_completed`), which carries the failure
        // disposition; deciding it here too would race across channels.
        self.finalize_turn();
        self.add_to_history(history_cell::new_error_event(message));
        self.request_redraw();
    }

    pub(super) fn on_warning(&mut self, message: impl Into<String>) {
        self.add_to_history(history_cell::new_warning_event(message.into()));
        self.request_redraw();
    }

    pub(super) fn on_plan_update(&mut self, update: PlanUpdate) {
        if self.plan_drawer_mode != PlanDrawerMode::Off {
            self.pinned_plan = Some(update);
            self.request_redraw();
        } else {
            self.add_to_history(history_cell::new_plan_update(update.clone()));
            self.pinned_plan = Some(update);
        }
    }

    pub(crate) fn on_history_entry_loaded(
        &mut self,
        log_id: i64,
        offset: i64,
        entry: Option<nori_harness::HistoryEntry>,
    ) {
        let (Ok(log_id), Ok(offset)) = (u64::try_from(log_id), usize::try_from(offset)) else {
            return;
        };
        self.bottom_pane
            .on_history_entry_response(log_id, offset, entry.map(|e| e.text));
    }

    pub(super) fn on_prompt_summary(&mut self, summary: String) {
        self.bottom_pane.set_prompt_summary(Some(summary));
    }

    pub(crate) fn on_undo_snapshots_loaded(&mut self, snapshots: Vec<nori_harness::UndoSnapshot>) {
        if snapshots.is_empty() {
            self.add_info_message("No undo snapshots available.".to_string(), None);
            return;
        }

        let items: Vec<SelectionItem> = snapshots
            .into_iter()
            .map(|snap| {
                let index = snap.index;
                let label = truncate_text(&snap.label, 60);
                let name = format!("[{}] {label}", snap.short_id);
                let tx = self.app_event_tx.clone();
                SelectionItem {
                    name,
                    display_shortcut: None,
                    description: None,
                    selected_description: None,
                    is_current: false,
                    actions: vec![Box::new(move |_| {
                        tx.send(AppEvent::HarnessAction(
                            crate::app_event::HarnessAction::UndoTo(index),
                        ));
                    })],
                    dismiss_on_select: true,
                    search_value: None,
                }
            })
            .collect();

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Undo to snapshot".to_string()),
            subtitle: None,
            footer_hint: Some(standard_popup_hint_line()),
            items,
            header: Box::new(()),
            is_searchable: false,
            ..Default::default()
        });
        self.request_redraw();
    }

    /// Periodic tick to commit at most one queued line to history with a small delay,
    /// animating the output.
    pub(crate) fn on_commit_tick(&mut self) {
        if let Some(controller) = self.stream_controller.as_mut() {
            let (cell, is_idle) = controller.on_commit_tick();
            if let Some(cell) = cell {
                // NOTE: Do NOT hide the status indicator here. The "Working (Xs)"
                // message should remain visible until the conversational turn fully
                // completes (when TaskComplete event arrives and set_task_running(false)
                // is called). Hiding it during streaming commits causes the indicator
                // to disappear prematurely while the agent is still processing.
                self.add_boxed_history(cell);
            }
            if is_idle {
                self.app_event_tx.send(AppEvent::StopCommitAnimation);
            }
        }
    }

    #[inline]
    pub(super) fn handle_streaming_delta(&mut self, delta: String) {
        // Always flush the active cell before streaming agent text. This ensures
        // tool cells appear in the correct chronological position (before the text
        // that follows them), even when tool calls haven't completed yet.
        self.flush_active_cell();

        if self.stream_controller.is_none() {
            if self.needs_final_message_separator {
                let elapsed_seconds = self
                    .bottom_pane
                    .status_widget()
                    .map(crate::status_indicator_widget::StatusIndicatorWidget::elapsed_seconds);
                self.add_to_history(history_cell::FinalMessageSeparator::new(elapsed_seconds));
                self.needs_final_message_separator = false;
            }
            self.stream_controller = Some(StreamController::new(
                self.last_rendered_width.get().map(|w| w.saturating_sub(2)),
            ));
        }
        if let Some(controller) = self.stream_controller.as_mut()
            && controller.push(&delta)
        {
            self.app_event_tx.send(AppEvent::StartCommitAnimation);
        }
        self.request_redraw();
    }

    /// Observes the parent directories of file paths to update the effective CWD tracker.
    /// If the effective CWD changes (after debounce), triggers a system info refresh.
    pub(super) fn observe_directories_from_paths<'a>(
        &mut self,
        paths: impl Iterator<Item = &'a std::path::Path>,
    ) {
        for file_path in paths {
            let absolute_path = if file_path.is_absolute() {
                file_path.to_path_buf()
            } else {
                self.config.cwd.join(file_path)
            };

            if self.effective_cwd_tracker.observe_file_path(&absolute_path) {
                let refresh_dir = crate::effective_cwd_tracker::find_git_root(&absolute_path)
                    .or_else(|| {
                        absolute_path
                            .parent()
                            .filter(|p| p.exists())
                            .map(std::path::Path::to_path_buf)
                    });

                if let Some(dir) = refresh_dir {
                    self.app_event_tx
                        .send(AppEvent::RefreshSystemInfoForDirectory {
                            dir,
                            agent: Some(self.config.active_agent.clone()),
                        });
                }
            }
        }
    }

    /// Handle Ctrl-C key press.
    pub(super) fn on_ctrl_c(&mut self) {
        // Ctrl+C bypasses BottomPane key routing, so gate it here: once exit
        // is in progress it must not interrupt, clear, or re-hint anything.
        if self.exiting {
            return;
        }
        if self.bottom_pane.on_ctrl_c() == CancellationEvent::Handled {
            return;
        }

        if self.bottom_pane.is_task_running() {
            self.bottom_pane.show_ctrl_c_quit_hint();
            self.submit_harness_action(crate::app_event::HarnessAction::Cancel);
            return;
        }

        self.begin_exit();
    }

    pub(crate) fn on_custom_prompts_loaded(
        &mut self,
        custom_prompts: Vec<nori_harness::CustomPrompt>,
    ) {
        let len = custom_prompts.len();
        tracing::debug!("received {len} custom prompts");
        // Forward to bottom pane so the slash popup can show them now.
        self.bottom_pane.set_custom_prompts(custom_prompts);
    }

    pub(crate) fn on_history_search_loaded(&mut self, entries: Vec<nori_harness::HistoryEntry>) {
        self.bottom_pane.on_search_history_response(entries);
        self.request_redraw();
    }

    pub(crate) fn handle_client_event(&mut self, event: crate::presentation::ClientEvent) {
        match event {
            crate::presentation::ClientEvent::ApprovalRequest(_) => {}
            crate::presentation::ClientEvent::ToolSnapshot(tool_snapshot) => {
                self.handle_client_tool_snapshot(tool_snapshot);
            }
            crate::presentation::ClientEvent::MessageDelta(message_delta) => {
                self.handle_client_message_delta(message_delta);
            }
            crate::presentation::ClientEvent::PlanSnapshot(plan_snapshot) => {
                self.handle_client_plan_snapshot(plan_snapshot);
            }
            crate::presentation::ClientEvent::SessionPhaseChanged(phase) => {
                self.handle_client_phase_changed(phase);
            }
            crate::presentation::ClientEvent::PromptCompleted(completed) => {
                self.handle_client_prompt_completed(completed);
            }
            crate::presentation::ClientEvent::LoadCompleted => {
                self.request_redraw();
            }
            crate::presentation::ClientEvent::QueueChanged(queue_changed) => {
                self.bottom_pane
                    .set_queued_user_messages(queue_changed.prompts);
                self.request_redraw();
            }
            crate::presentation::ClientEvent::ContextCompacted(context_compacted) => {
                self.on_context_compacted(context_compacted.summary);
            }
            crate::presentation::ClientEvent::ReplayEntry(replay_entry) => {
                self.handle_client_replay_entry(replay_entry);
            }
            crate::presentation::ClientEvent::AgentCommandsUpdate(update) => {
                self.bottom_pane.set_agent_commands(update.commands);
            }
            crate::presentation::ClientEvent::SessionConfigUpdate(update) => {
                self.handle_acp_session_config_update(&update.config_options);
            }
            crate::presentation::ClientEvent::SessionUpdateInfo(update) => {
                if update.kind == crate::presentation::SessionUpdateKind::ConfigOptions {
                    self.refresh_acp_mode_config_snapshot();
                    self.request_redraw();
                    return;
                }
                self.clear_pending_goal_edit_if_no_goal(&update);
                if let Some(patch) = update.session_info_patch.as_ref() {
                    let origin = crate::nori::session_info::SessionInfoOrigin::from_replay_source(
                        self.replay_source,
                    );
                    self.session_info_state.apply(patch, origin);
                    self.bottom_pane
                        .set_session_title(self.session_info_state.title().map(str::to_string));
                    // Verbose session-info dumps are for next-channel / debug
                    // builds only; stable releases keep the transcript quiet.
                    if crate::version::show_verbose_session_info_history() {
                        let display = crate::nori::session_info::display(
                            self.session_agent_info.as_ref(),
                            self.bottom_pane.agent_display_name(),
                            patch,
                            origin,
                        );
                        self.add_to_history(display.history_cell());
                    }
                } else if update.kind == crate::presentation::SessionUpdateKind::Usage
                    && let Some(usage) = update.usage
                {
                    self.bottom_pane.set_session_usage(Some(usage));
                } else {
                    self.add_info_message(update.message, update.hint);
                }
                self.request_redraw();
            }
            crate::presentation::ClientEvent::SessionModeChanged(update) => {
                self.handle_acp_session_mode_changed(&update.current_mode_id);
            }
            crate::presentation::ClientEvent::SessionCapabilitiesChanged(update) => {
                self.handle_session_capabilities_changed(update);
            }
            crate::presentation::ClientEvent::ThreadGoalUpdated(update) => {
                self.handle_thread_goal_updated(update.goal);
            }
            crate::presentation::ClientEvent::ThreadGoalCleared => {
                self.handle_thread_goal_cleared();
            }
            crate::presentation::ClientEvent::Warning(warning) => {
                self.on_warning(warning.message);
            }
        }
    }

    fn handle_client_message_delta(&mut self, message_delta: crate::presentation::MessageDelta) {
        match message_delta.stream {
            crate::presentation::MessageStream::User => {
                // A user chunk is an echo only while a local prompt owns the turn.
                if self.owned_prompt_request_id.is_none() && !message_delta.delta.is_empty() {
                    self.session_stats.record_user_message();
                    self.add_to_history(history_cell::new_user_prompt(message_delta.delta));
                    self.request_redraw();
                }
            }
            crate::presentation::MessageStream::Answer => {
                self.assistant_stream_seen_for_stats = true;
                self.on_agent_message_delta(message_delta.delta)
            }
            crate::presentation::MessageStream::Reasoning => {
                self.on_agent_reasoning_delta(message_delta.delta);
            }
        }
    }

    fn handle_client_plan_snapshot(&mut self, plan_snapshot: crate::presentation::PlanSnapshot) {
        self.on_plan_update(plan_snapshot_to_update_plan_args(plan_snapshot));
    }

    fn handle_client_phase_changed(
        &mut self,
        phase: crate::presentation::session_runtime::SessionPhaseView,
    ) {
        let previous_phase = self.acp_session_phase.replace(phase);

        match phase {
            crate::presentation::session_runtime::SessionPhaseView::Idle => {
                self.bottom_pane.set_task_running(false);
                self.bottom_pane.set_interrupt_hint_visible(false);
                if !matches!(
                    previous_phase,
                    Some(crate::presentation::session_runtime::SessionPhaseView::Idle)
                ) {
                    self.request_redraw();
                    self.refresh_terminal_title();
                }
            }
            crate::presentation::session_runtime::SessionPhaseView::Loading => {
                self.bottom_pane.set_task_running(true);
                self.bottom_pane.ensure_status_indicator();
                self.bottom_pane.set_interrupt_hint_visible(false);
                self.set_status_header("Loading session".to_string());
                self.request_redraw();
                self.refresh_terminal_title();
            }
            crate::presentation::session_runtime::SessionPhaseView::Prompt => {
                if matches!(
                    previous_phase,
                    Some(crate::presentation::session_runtime::SessionPhaseView::Prompt)
                        | Some(crate::presentation::session_runtime::SessionPhaseView::Cancelling)
                ) {
                    self.bottom_pane.set_task_running(true);
                    self.bottom_pane.ensure_status_indicator();
                    self.bottom_pane.set_interrupt_hint_visible(true);
                    self.request_redraw();
                    self.refresh_terminal_title();
                } else {
                    self.on_task_started();
                }
            }
            crate::presentation::session_runtime::SessionPhaseView::Cancelling => {
                self.bottom_pane.set_task_running(true);
                self.bottom_pane.ensure_status_indicator();
                self.bottom_pane.set_interrupt_hint_visible(false);
                self.set_status_header("Cancelling".to_string());
                self.request_redraw();
                self.refresh_terminal_title();
            }
        }
    }

    fn handle_client_prompt_completed(&mut self, completed: crate::presentation::PromptCompleted) {
        // The completion owns the loop lifecycle: a fatal failure disarms the
        // loop *before* on_task_complete can re-fire it, while a retryable
        // failure leaves it armed so the next iteration retries. Deciding this
        // here (rather than in on_error) keeps it on a single ordered event.
        if completed.failure == Some(crate::presentation::TurnFailure::Fatal) {
            self.cancel_loop();
        }
        // A failure already surfaces its own error cell; only a clean user
        // cancellation shows the generic "interrupted" notice.
        let interrupted = completed.stop_reason == nori_protocol::acp::v1::StopReason::Cancelled
            && completed.failure.is_none();
        let has_final_message = completed
            .last_agent_message
            .as_ref()
            .is_some_and(|message| !message.is_empty());
        if has_final_message || self.assistant_stream_seen_for_stats {
            self.session_stats.record_assistant_message();
        }
        self.assistant_stream_seen_for_stats = false;
        self.on_task_complete(completed.last_agent_message);
        if interrupted {
            self.add_to_history(history_cell::new_error_event(
                "Conversation interrupted - tell the model what to do differently. Something went wrong? Report the issue at https://github.com/tilework-tech/nori-cli/issues"
                    .to_owned(),
            ));
            self.request_redraw();
        }
    }

    fn handle_replay_client_event(&mut self, event: crate::presentation::ClientEvent) {
        match event {
            crate::presentation::ClientEvent::MessageDelta(delta) => {
                let same_message = self.replay_message.as_ref().is_some_and(|message| {
                    message.stream == delta.stream
                        && match (&message.message_id, &delta.message_id) {
                            (Some(current), Some(incoming)) => current == incoming,
                            (None, None) => true,
                            (Some(_), None) | (None, Some(_)) => false,
                        }
                });
                if same_message {
                    if let Some(message) = self.replay_message.as_mut() {
                        message.text.push_str(&delta.delta);
                    }
                } else {
                    self.flush_replay_message();
                    self.replay_message = Some(ReplayMessage {
                        stream: delta.stream,
                        message_id: delta.message_id,
                        text: delta.delta,
                    });
                }
            }
            crate::presentation::ClientEvent::PlanSnapshot(snapshot) => {
                self.flush_replay_message();
                self.handle_client_replay_entry(crate::presentation::ReplayEntry::PlanSnapshot {
                    snapshot,
                });
            }
            crate::presentation::ClientEvent::ToolSnapshot(snapshot) => {
                self.flush_replay_message();
                self.handle_client_replay_entry(crate::presentation::ReplayEntry::ToolSnapshot {
                    snapshot: Box::new(snapshot),
                });
            }
            crate::presentation::ClientEvent::ReplayEntry(replay_entry) => {
                self.flush_replay_message();
                self.handle_client_replay_entry(replay_entry);
            }
            event => {
                self.flush_replay_message();
                self.handle_client_event(event);
            }
        }
    }

    fn flush_replay_message(&mut self) {
        let Some(message) = self.replay_message.take() else {
            return;
        };
        let replay_entry = match message.stream {
            crate::presentation::MessageStream::User => {
                crate::presentation::ReplayEntry::UserMessage { text: message.text }
            }
            crate::presentation::MessageStream::Answer => {
                crate::presentation::ReplayEntry::AssistantMessage { text: message.text }
            }
            crate::presentation::MessageStream::Reasoning => {
                crate::presentation::ReplayEntry::ReasoningMessage { text: message.text }
            }
        };
        self.handle_client_replay_entry(replay_entry);
    }

    fn handle_client_replay_entry(&mut self, replay_entry: crate::presentation::ReplayEntry) {
        match replay_entry {
            crate::presentation::ReplayEntry::UserMessage { text } => {
                self.add_to_history(history_cell::new_user_prompt(text));
            }
            crate::presentation::ReplayEntry::AssistantMessage { text } => {
                self.handle_streaming_delta(text);
                self.flush_answer_stream_with_separator();
            }
            crate::presentation::ReplayEntry::ReasoningMessage { text } => {
                let cell = history_cell::new_reasoning_summary_block(text);
                self.add_boxed_history(cell);
            }
            crate::presentation::ReplayEntry::PlanSnapshot { snapshot } => {
                self.add_to_history(history_cell::new_plan_update(
                    plan_snapshot_to_update_plan_args(snapshot),
                ));
            }
            crate::presentation::ReplayEntry::ToolSnapshot { snapshot } => {
                self.handle_client_tool_snapshot(*snapshot);
            }
        }
        self.request_redraw();
    }

    fn handle_client_approval_request(
        &mut self,
        approval: crate::presentation::ApprovalRequest,
        request_id: nori_protocol::acp::v1::RequestId,
        options: Vec<nori_protocol::acp::v1::PermissionOption>,
    ) {
        let Some(request) =
            approval_request_from_client_event(approval, &self.config.cwd, request_id, options)
        else {
            return;
        };

        self.flush_answer_stream_with_separator();
        self.notify(Notification::ExecApprovalRequested {
            command: request.title.clone(),
        });
        self.bottom_pane.push_approval_request(request);
        self.request_redraw();
    }

    /// All ACP tool kinds route through ClientToolCell for native rendering.
    /// ClientToolCell auto-detects exploring tools (Read/Search) and renders
    /// them with "Explored" format, while Execute uses shell-style transcript.
    fn handle_client_tool_snapshot(&mut self, tool_snapshot: crate::presentation::ToolSnapshot) {
        // NOTE: The answer stream is finalized only on paths that insert a new
        // history cell. No-op updates (e.g., progress notifications for a
        // long-running tool whose cell was already flushed) must not finalize
        // the stream, or one streaming assistant message fragments into many
        // separate `•` cells.
        self.session_stats
            .record_client_tool_snapshot(&tool_snapshot);

        // For completed Create/Edit/Delete/Move, observe directories for footer refreshes.
        if matches!(
            tool_snapshot.kind,
            crate::presentation::ToolKind::Create
                | crate::presentation::ToolKind::Edit
                | crate::presentation::ToolKind::Delete
                | crate::presentation::ToolKind::Move
        ) && tool_snapshot.phase == crate::presentation::ToolPhase::Completed
        {
            self.observe_directories_from_paths(
                tool_snapshot.locations.iter().map(|l| l.path.as_path()),
            );
        }

        // Update existing active ClientToolCell if same call_id
        if let Some(cell) = self
            .active_cell
            .as_mut()
            .and_then(|c| c.as_any_mut().downcast_mut::<ClientToolCell>())
            && cell.call_id() == tool_snapshot.call_id
        {
            // The stream cannot be open while a tool cell is active: every
            // path that sets active_cell flushes the stream first, and every
            // answer delta clears active_cell. This lets in-place updates
            // skip the answer-stream flush.
            debug_assert!(self.stream_controller.is_none());
            cell.apply_snapshot(tool_snapshot);
            if !cell.is_active() && !cell.is_exploring() {
                self.flush_active_cell();
            }
            return;
        }

        // If this call_id was already flushed to history (e.g., due to
        // interleaved text streaming), skip creating a duplicate cell.
        if self
            .completed_client_tool_calls
            .contains(&tool_snapshot.call_id)
        {
            return;
        }

        // Check if this snapshot is for a buffered incomplete execute cell.
        // This allows completions to reach cells that were displaced from
        // active_cell by subsequent tool snapshots (parallel ACP calls).
        if let Some(mut buffered_cell) = self
            .pending_client_tool_cells
            .remove(&tool_snapshot.call_id)
        {
            buffered_cell.apply_snapshot(tool_snapshot);
            if !buffered_cell.is_active() {
                self.flush_answer_stream_with_separator();
                // Insert directly into history without flushing active_cell.
                // The normal add_boxed_history path flushes active_cell first
                // (to maintain chronological order), but that would incorrectly
                // mark the current active Execute cell as completed and discard
                // its later completion event.
                self.completed_client_tool_calls
                    .insert(buffered_cell.call_id().to_owned());
                self.needs_final_message_separator = true;
                self.app_event_tx
                    .send(AppEvent::InsertHistoryCell(Box::new(buffered_cell)));
            } else {
                // Still incomplete — put it back in the buffer.
                let call_id = buffered_cell.call_id().to_owned();
                self.pending_client_tool_cells
                    .insert(call_id, buffered_cell);
            }
            return;
        }

        // Merge into existing exploring cell when possible
        let is_new_exploring = crate::client_event_format::is_exploring_snapshot(&tool_snapshot);
        if is_new_exploring
            && let Some(cell) = self
                .active_cell
                .as_mut()
                .and_then(|c| c.as_any_mut().downcast_mut::<ClientToolCell>())
            && cell.is_exploring()
        {
            // Same invariant as the in-place update above: no open stream
            // while an exploring cell is active, so no flush is needed.
            debug_assert!(self.stream_controller.is_none());
            cell.merge_exploring(tool_snapshot);
            // Don't track in completed_client_tool_calls here — non-terminal
            // snapshots (Pending/InProgress) arrive first with empty invocations,
            // and the real path/query comes in a later tool_call_update. Tracking
            // is deferred to flush_active_cell, which marks all exploring call_ids
            // as completed when the cell leaves active_cell.
            return;
        }

        // A genuinely new tool call starts a new visual cell: finalize the
        // streamed answer so far to keep history chronological.
        self.flush_answer_stream_with_separator();

        // Buffer incomplete Execute ClientToolCells instead of flushing
        // them to history with wrong content (description text as output).
        if let Some(active) = self.active_cell.take() {
            if let Some(client_cell) = active.as_any().downcast_ref::<ClientToolCell>()
                && client_cell.is_active()
                && *client_cell.snapshot_kind() == crate::presentation::ToolKind::Execute
            {
                let call_id = client_cell.call_id().to_owned();
                if let Ok(boxed) = active.into_any().downcast::<ClientToolCell>() {
                    self.pending_client_tool_cells.insert(call_id, *boxed);
                }
            } else {
                self.active_cell = Some(active);
                self.flush_active_cell();
            }
        }
        let should_flush = !matches!(
            tool_snapshot.phase,
            crate::presentation::ToolPhase::Pending
                | crate::presentation::ToolPhase::PendingApproval
                | crate::presentation::ToolPhase::InProgress
        ) && !is_new_exploring;
        let mut cell = ClientToolCell::new(
            tool_snapshot,
            self.config.cwd.clone(),
            self.config.animations,
        );
        if is_new_exploring {
            cell.mark_exploring();
        }
        self.active_cell = Some(Box::new(cell));
        if should_flush {
            self.flush_active_cell();
        }
    }
}

fn is_turn_content_update(update: &nori_protocol::acp::v1::SessionUpdate) -> bool {
    // Session metadata and capability changes do not by themselves begin a proactive turn.
    matches!(
        update,
        nori_protocol::acp::v1::SessionUpdate::UserMessageChunk(_)
            | nori_protocol::acp::v1::SessionUpdate::AgentMessageChunk(_)
            | nori_protocol::acp::v1::SessionUpdate::AgentThoughtChunk(_)
            | nori_protocol::acp::v1::SessionUpdate::Plan(_)
            | nori_protocol::acp::v1::SessionUpdate::ToolCall(_)
            | nori_protocol::acp::v1::SessionUpdate::ToolCallUpdate(_)
    )
}

fn approval_request_from_client_event(
    approval: crate::presentation::ApprovalRequest,
    cwd: &std::path::Path,
    request_id: nori_protocol::acp::v1::RequestId,
    options: Vec<nori_protocol::acp::v1::PermissionOption>,
) -> Option<ApprovalRequest> {
    let crate::presentation::ApprovalSubject::ToolSnapshot(snapshot) = approval.subject;

    Some(ApprovalRequest {
        request_id,
        title: approval.title,
        kind: approval.kind,
        cwd: cwd.to_path_buf(),
        snapshot: Box::new(snapshot),
        options,
    })
}

fn plan_snapshot_to_update_plan_args(
    plan_snapshot: crate::presentation::PlanSnapshot,
) -> PlanUpdate {
    PlanUpdate {
        explanation: None,
        plan: plan_snapshot
            .entries
            .into_iter()
            .map(|entry| PlanItem {
                step: entry.step,
                status: match entry.status {
                    crate::presentation::PlanStatus::Pending => StepStatus::Pending,
                    crate::presentation::PlanStatus::InProgress => StepStatus::InProgress,
                    crate::presentation::PlanStatus::Completed => StepStatus::Completed,
                },
            })
            .collect(),
    }
}
