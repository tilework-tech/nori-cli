use super::*;

impl ChatWidget {
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

    // --- Small event handlers ---
    pub(super) fn on_session_configured(
        &mut self,
        event: codex_core::protocol::SessionConfiguredEvent,
    ) {
        // Mark that we've received SessionConfigured - this unlocks event processing
        // when expected_agent is set (during agent switching)
        self.session_configured_received = true;

        // Clear the "Connecting to [Agent]" status indicator shown during agent startup
        self.bottom_pane.hide_status_indicator();

        // Update footer with current approval mode
        self.update_approval_mode_label();

        self.bottom_pane
            .set_history_metadata(event.history_log_id, event.history_entry_count);
        self.conversation_id = Some(event.session_id);
        self.current_rollout_path = Some(event.rollout_path.clone());
        let initial_messages = event.initial_messages.clone();
        let agent_for_header = event.model.clone();
        self.session_header.set_agent(&agent_for_header);
        self.add_to_history(history_cell::new_session_info(
            &self.config,
            event,
            self.show_welcome_banner,
        ));
        if let Some(messages) = initial_messages {
            self.replay_initial_messages(messages);
        }
        // Ask codex-core to enumerate custom prompts for this session.
        self.submit_op(Op::ListCustomPrompts);
        if let Some(user_message) = self.initial_user_message.take() {
            self.submit_user_message(user_message);
        }
        if !self.suppress_session_configured_redraw {
            self.request_redraw();
        }
    }

    pub(super) fn on_agent_message(&mut self, message: String) {
        // Track assistant message for session statistics
        self.session_stats.record_assistant_message();

        // If we have a stream_controller, then the final agent message is redundant and will be a
        // duplicate of what has already been streamed.
        if self.stream_controller.is_none() {
            self.handle_streaming_delta(message);
        }
        self.flush_answer_stream_with_separator();

        // Close the gate BEFORE flushing: any tool events arriving after this
        // point are stale and should be silently discarded.
        self.turn_finished = true;

        // Handle pending task_complete state.
        if self.task_complete_pending {
            self.bottom_pane.hide_status_indicator();
            self.task_complete_pending = false;
        }

        // Flush pending End events while discarding stale Begin events, so
        // completed tool results are rendered but new tool-call cells don't
        // appear below the agent's final message.
        let mut mgr = std::mem::take(&mut self.interrupts);
        mgr.flush_completions_and_clear(self);
        self.interrupts = mgr;

        self.request_redraw();
    }

    pub(super) fn on_context_compacted(
        &mut self,
        event: codex_core::protocol::ContextCompactedEvent,
    ) {
        // Step 1: Flush the streamed summary from the old session.
        self.flush_answer_stream_with_separator();
        self.turn_finished = true;

        // Step 2: Show "Context compacted" as an info message.
        self.add_info_message("Context compacted".to_owned(), None);

        // When the ACP backend provides a summary, show a session header
        // followed by the summary reprinted as the first assistant message
        // of the new session. This makes the session boundary visible.
        if let Some(summary) = event.summary {
            // Step 3: Insert a new session header (same card as a fresh session,
            // but without install hints since this is not the first launch).
            use crate::nori::session_header::DisplayMode;
            use crate::nori::session_header::NoriSessionHeaderCell;
            let header =
                NoriSessionHeaderCell::new(self.config.model.clone(), self.config.cwd.clone())
                    .with_display_mode(DisplayMode::Compact);
            self.add_to_history(history_cell::SessionInfoCell::new(
                history_cell::CompositeHistoryCell::new(vec![Box::new(header)]),
            ));

            // Step 4: Reprint the summary as the first assistant message of the
            // new session. Reset turn_finished so streaming works.
            self.turn_finished = false;
            self.handle_streaming_delta(summary);
            self.flush_answer_stream_with_separator();
            self.turn_finished = true;
        }

        self.request_redraw();
    }

    pub(super) fn on_agent_message_delta(&mut self, delta: String) {
        self.handle_streaming_delta(delta);
    }

    pub(super) fn on_agent_reasoning_delta(&mut self, delta: String) {
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

    pub(super) fn on_agent_reasoning_final(&mut self) {
        // At the end of a reasoning block, record transcript-only content.
        self.full_reasoning_buffer.push_str(&self.reasoning_buffer);
        if !self.full_reasoning_buffer.is_empty() {
            let cell = history_cell::new_reasoning_summary_block(
                self.full_reasoning_buffer.clone(),
                &self.config,
            );
            self.add_boxed_history(cell);
        }
        self.reasoning_buffer.clear();
        self.full_reasoning_buffer.clear();
        self.request_redraw();
    }

    pub(super) fn on_reasoning_section_break(&mut self) {
        // Start a new reasoning block for header extraction and accumulate transcript.
        self.full_reasoning_buffer.push_str(&self.reasoning_buffer);
        self.full_reasoning_buffer.push_str("\n\n");
        self.reasoning_buffer.clear();
    }

    // Raw reasoning uses the same flow as summarized reasoning

    pub(super) fn on_task_started(&mut self) {
        self.bottom_pane.clear_ctrl_c_quit_hint();
        self.bottom_pane.set_task_running(true);
        self.retry_status_header = None;
        self.bottom_pane.set_interrupt_hint_visible(true);
        self.set_status_header(crate::status_indicator_widget::random_status_message());
        self.full_reasoning_buffer.clear();
        self.reasoning_buffer.clear();
        self.turn_finished = false;
        self.request_redraw();
    }

    pub(super) fn on_task_complete(&mut self, last_agent_message: Option<String>) {
        // If a stream is currently active, finalize it.
        self.flush_answer_stream_with_separator();
        // Process any deferred completion events (ExecEnd, McpEnd, PatchEnd) so
        // in-progress tool cells transition to their finished state ("Running" →
        // "Ran"). Discard begin events that would create new cells below the
        // agent's final message.
        let mut mgr = std::mem::take(&mut self.interrupts);
        let discarded = mgr.flush_completions_and_clear(self);
        self.interrupts = mgr;
        if discarded > 0 {
            debug!("on_task_complete: discarded {discarded} deferred begin/other interrupt events");
        }

        // Drain any pending ExecCells that weren't completed (e.g., due to interruption).
        self.pending_exec_cells.drain_failed();

        // Mark task stopped and request redraw now that all content is in history.
        self.bottom_pane.set_task_running(false);
        self.running_commands.clear();
        self.suppressed_exec_calls.clear();
        self.last_unified_wait = None;
        self.request_redraw();

        // Refresh system info (including git branch) on task completion.
        // This catches any branch changes that occurred during the agent's turn.
        self.app_event_tx
            .send(AppEvent::RefreshSystemInfoForDirectory {
                dir: self.config.cwd.clone(),
                agent: Some(self.config.model.clone()),
            });

        // If there is a queued user message, send exactly one now to begin the next turn.
        self.maybe_send_next_queued_input();
        // Emit a notification when the turn completes (suppressed if focused).
        self.notify(Notification::AgentTurnComplete {
            response: last_agent_message.unwrap_or_default(),
        });

        // Loop mode: if iterations remain, fire the next iteration.
        #[cfg(feature = "nori-config")]
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

    pub(crate) fn set_token_info(&mut self, info: Option<TokenUsageInfo>) {
        match info {
            Some(info) => self.apply_token_info(info),
            None => {
                self.bottom_pane.set_context_window_percent(None);
                self.token_info = None;
            }
        }
    }

    pub(super) fn apply_token_info(&mut self, info: TokenUsageInfo) {
        let percent = self.context_used_percent(&info);
        self.bottom_pane.set_context_window_percent(percent);
        self.token_info = Some(info);
    }

    pub(super) fn context_used_percent(&self, info: &TokenUsageInfo) -> Option<i64> {
        info.model_context_window
            .or(self.config.model_context_window)
            .map(|window| {
                let remaining = info
                    .last_token_usage
                    .percent_of_context_window_remaining(window);
                (100 - remaining).clamp(0, 100)
            })
    }

    pub(crate) fn on_rate_limit_snapshot(&mut self, snapshot: Option<RateLimitSnapshot>) {
        if let Some(snapshot) = snapshot {
            let warnings = self.rate_limit_warnings.take_warnings(
                snapshot
                    .secondary
                    .as_ref()
                    .map(|window| window.used_percent),
                snapshot
                    .secondary
                    .as_ref()
                    .and_then(|window| window.window_minutes),
                snapshot.primary.as_ref().map(|window| window.used_percent),
                snapshot
                    .primary
                    .as_ref()
                    .and_then(|window| window.window_minutes),
            );

            let display = crate::status::rate_limit_snapshot_display(&snapshot, Local::now());
            self.rate_limit_snapshot = Some(display);

            if !warnings.is_empty() {
                for warning in warnings {
                    self.add_to_history(history_cell::new_warning_event(warning));
                }
                self.request_redraw();
            }
        } else {
            self.rate_limit_snapshot = None;
        }
    }

    /// Finalize any active exec as failed and stop/clear running UI state.
    pub(super) fn finalize_turn(&mut self) {
        // Ensure any spinner is replaced by a red ✗ and flushed into history.
        self.finalize_active_cell_as_failed();
        // Reset running state and clear streaming buffers.
        self.bottom_pane.set_task_running(false);
        self.running_commands.clear();
        self.suppressed_exec_calls.clear();
        self.last_unified_wait = None;
        self.stream_controller = None;
    }

    pub(super) fn on_error(&mut self, message: String) {
        self.finalize_turn();
        self.cancel_loop();
        self.add_to_history(history_cell::new_error_event(message));
        self.request_redraw();

        // After an error ends the turn, try sending the next queued input.
        self.maybe_send_next_queued_input();
    }

    pub(super) fn on_warning(&mut self, message: impl Into<String>) {
        self.add_to_history(history_cell::new_warning_event(message.into()));
        self.request_redraw();
    }

    pub(super) fn on_mcp_startup_update(&mut self, ev: McpStartupUpdateEvent) {
        let mut status = self.mcp_startup_status.take().unwrap_or_default();
        if let McpStartupStatus::Failed { error } = &ev.status {
            self.on_warning(error);
        }
        status.insert(ev.server, ev.status);
        self.mcp_startup_status = Some(status);
        self.bottom_pane.set_task_running(true);
        if let Some(current) = &self.mcp_startup_status {
            let total = current.len();
            let mut starting: Vec<_> = current
                .iter()
                .filter_map(|(name, state)| {
                    if matches!(state, McpStartupStatus::Starting) {
                        Some(name)
                    } else {
                        None
                    }
                })
                .collect();
            starting.sort();
            if let Some(first) = starting.first() {
                let completed = total.saturating_sub(starting.len());
                let max_to_show = 3;
                let mut to_show: Vec<String> = starting
                    .iter()
                    .take(max_to_show)
                    .map(ToString::to_string)
                    .collect();
                if starting.len() > max_to_show {
                    to_show.push("…".to_string());
                }
                let header = if total > 1 {
                    format!(
                        "Starting MCP servers ({completed}/{total}): {}",
                        to_show.join(", ")
                    )
                } else {
                    format!("Booting MCP server: {first}")
                };
                self.set_status_header(header);
            }
        }
        self.request_redraw();
    }

    pub(super) fn on_mcp_startup_complete(&mut self, ev: McpStartupCompleteEvent) {
        let mut parts = Vec::new();
        if !ev.failed.is_empty() {
            let failed_servers: Vec<_> = ev.failed.iter().map(|f| f.server.clone()).collect();
            parts.push(format!("failed: {}", failed_servers.join(", ")));
        }
        if !ev.cancelled.is_empty() {
            self.on_warning(format!(
                "MCP startup interrupted. The following servers were not initialized: {}",
                ev.cancelled.join(", ")
            ));
        }
        if !parts.is_empty() {
            self.on_warning(format!("MCP startup incomplete ({})", parts.join("; ")));
        }

        self.mcp_startup_status = None;
        self.bottom_pane.set_task_running(false);
        self.maybe_send_next_queued_input();
        self.request_redraw();
    }

    /// Handle a turn aborted due to user interrupt (Esc).
    /// When there are queued user messages, restore them into the composer
    /// separated by newlines rather than auto‑submitting the next one.
    pub(super) fn on_interrupted_turn(&mut self, _reason: TurnAbortReason) {
        // Finalize, log a gentle prompt, and clear running state.
        self.finalize_turn();
        self.cancel_loop();

        self.add_to_history(history_cell::new_error_event(
            "Conversation interrupted - tell the model what to do differently. Something went wrong? Report the issue at https://github.com/tilework-tech/nori-cli/issues".to_owned(),
        ));

        // If any messages were queued during the task, restore them into the composer.
        if !self.queued_user_messages.is_empty() {
            let queued_text = self
                .queued_user_messages
                .iter()
                .map(|m| m.text.clone())
                .collect::<Vec<_>>()
                .join("\n");
            let existing_text = self.bottom_pane.composer_text();
            let combined = if existing_text.is_empty() {
                queued_text
            } else if queued_text.is_empty() {
                existing_text
            } else {
                format!("{queued_text}\n{existing_text}")
            };
            self.bottom_pane.set_composer_text(combined);
            // Clear the queue and update the status indicator list.
            self.queued_user_messages.clear();
            self.refresh_queued_user_messages();
        }

        self.request_redraw();
    }

    pub(super) fn on_plan_update(&mut self, update: UpdatePlanArgs) {
        self.add_to_history(history_cell::new_plan_update(update));
    }

    pub(super) fn on_exec_approval_request(&mut self, id: String, ev: ExecApprovalRequestEvent) {
        // Approval requests must be handled immediately, not deferred. In ACP mode,
        // the agent subprocess is blocked waiting for the user's approval decision.
        // If we defer the approval popup, we create a deadlock: the agent waits for
        // approval, but TaskComplete (which would flush the queue) won't arrive until
        // the agent finishes, which won't happen until approval is granted.
        self.handle_exec_approval_now(id, ev);
    }

    pub(super) fn on_apply_patch_approval_request(
        &mut self,
        id: String,
        ev: ApplyPatchApprovalRequestEvent,
    ) {
        // Same as on_exec_approval_request: handle immediately to avoid deadlock.
        self.handle_apply_patch_approval_now(id, ev);
    }

    pub(super) fn on_elicitation_request(&mut self, ev: ElicitationRequestEvent) {
        let ev2 = ev.clone();
        self.defer_or_handle(
            |q| q.push_elicitation(ev),
            |s| s.handle_elicitation_request_now(ev2),
        );
    }

    pub(super) fn on_exec_command_begin(&mut self, ev: ExecCommandBeginEvent) {
        if self.turn_finished {
            return;
        }
        self.flush_answer_stream_with_separator();
        let ev2 = ev.clone();
        self.defer_or_handle(|q| q.push_exec_begin(ev), |s| s.handle_exec_begin_now(ev2));
    }

    pub(super) fn on_exec_command_output_delta(
        &mut self,
        _ev: codex_core::protocol::ExecCommandOutputDeltaEvent,
    ) {
        // TODO: Handle streaming exec output if/when implemented
    }

    pub(super) fn on_patch_apply_begin(&mut self, event: PatchApplyBeginEvent) {
        if self.turn_finished {
            return;
        }
        // Track Edit tool call for session statistics
        self.session_stats.record_tool_call("Edit");

        // Observe directories from file paths to potentially update footer git info.
        self.observe_directories_from_changes(&event.changes);

        self.add_to_history(history_cell::new_patch_event(
            event.changes,
            &self.config.cwd,
        ));
    }

    pub(super) fn on_view_image_tool_call(&mut self, event: ViewImageToolCallEvent) {
        if self.turn_finished {
            return;
        }
        // Track ViewImage tool call for session statistics
        self.session_stats.record_tool_call("ViewImage");

        self.flush_answer_stream_with_separator();
        self.add_to_history(history_cell::new_view_image_tool_call(
            event.path,
            &self.config.cwd,
        ));
        self.request_redraw();
    }

    pub(super) fn on_patch_apply_end(&mut self, event: codex_core::protocol::PatchApplyEndEvent) {
        if self.turn_finished {
            return;
        }
        self.flush_answer_stream_with_separator();
        let ev2 = event.clone();
        self.defer_or_handle(
            |q| q.push_patch_end(event),
            |s| s.handle_patch_apply_end_now(ev2),
        );
    }

    pub(super) fn on_exec_command_end(&mut self, ev: ExecCommandEndEvent) {
        if self.turn_finished {
            return;
        }
        self.flush_answer_stream_with_separator();
        let ev2 = ev.clone();
        self.defer_or_handle(|q| q.push_exec_end(ev), |s| s.handle_exec_end_now(ev2));
    }

    pub(super) fn on_mcp_tool_call_begin(&mut self, ev: McpToolCallBeginEvent) {
        if self.turn_finished {
            return;
        }
        self.flush_answer_stream_with_separator();
        let ev2 = ev.clone();
        self.defer_or_handle(|q| q.push_mcp_begin(ev), |s| s.handle_mcp_begin_now(ev2));
    }

    pub(super) fn on_mcp_tool_call_end(&mut self, ev: McpToolCallEndEvent) {
        if self.turn_finished {
            return;
        }
        self.flush_answer_stream_with_separator();
        let ev2 = ev.clone();
        self.defer_or_handle(|q| q.push_mcp_end(ev), |s| s.handle_mcp_end_now(ev2));
    }

    pub(super) fn on_web_search_begin(&mut self, _ev: WebSearchBeginEvent) {
        self.flush_answer_stream_with_separator();
    }

    pub(super) fn on_web_search_end(&mut self, ev: WebSearchEndEvent) {
        // Track WebSearch tool call for session statistics
        self.session_stats.record_tool_call("WebSearch");

        self.flush_answer_stream_with_separator();
        self.add_to_history(history_cell::new_web_search_call(format!(
            "Searched: {}",
            ev.query
        )));
    }

    pub(super) fn on_get_history_entry_response(
        &mut self,
        event: codex_core::protocol::GetHistoryEntryResponseEvent,
    ) {
        let codex_core::protocol::GetHistoryEntryResponseEvent {
            offset,
            log_id,
            entry,
        } = event;
        self.bottom_pane
            .on_history_entry_response(log_id, offset, entry.map(|e| e.text));
    }

    pub(super) fn on_shutdown_complete(&mut self) {
        self.request_exit();
    }

    pub(super) fn on_turn_diff(&mut self, unified_diff: String) {
        debug!("TurnDiffEvent: {unified_diff}");
    }

    pub(super) fn on_deprecation_notice(&mut self, event: DeprecationNoticeEvent) {
        let DeprecationNoticeEvent { summary, details } = event;
        self.add_to_history(history_cell::new_deprecation_notice(summary, details));
        self.request_redraw();
    }

    pub(super) fn on_background_event(&mut self, message: String) {
        debug!("BackgroundEvent: {message}");
        self.bottom_pane.ensure_status_indicator();
        self.bottom_pane.set_interrupt_hint_visible(true);
        self.set_status_header(message);
    }

    pub(super) fn on_prompt_summary(&mut self, summary: String) {
        self.bottom_pane.set_prompt_summary(Some(summary));
    }

    pub(super) fn on_undo_started(&mut self, event: UndoStartedEvent) {
        self.bottom_pane.ensure_status_indicator();
        self.bottom_pane.set_interrupt_hint_visible(false);
        let message = event
            .message
            .unwrap_or_else(|| "Undo in progress...".to_string());
        self.set_status_header(message);
    }

    pub(super) fn on_undo_completed(&mut self, event: UndoCompletedEvent) {
        let UndoCompletedEvent { success, message } = event;
        self.bottom_pane.hide_status_indicator();
        let message = message.unwrap_or_else(|| {
            if success {
                "Undo completed successfully.".to_string()
            } else {
                "Undo failed.".to_string()
            }
        });
        if success {
            self.add_info_message(message, None);
        } else {
            self.add_error_message(message);
        }
    }

    pub(super) fn on_undo_list_result(&mut self, event: UndoListResultEvent) {
        if event.snapshots.is_empty() {
            self.add_info_message("No undo snapshots available.".to_string(), None);
            return;
        }

        let items: Vec<SelectionItem> = event
            .snapshots
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
                        tx.send(AppEvent::CodexOp(Op::UndoTo { index }));
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

    pub(super) fn on_stream_error(&mut self, message: String) {
        if self.retry_status_header.is_none() {
            self.retry_status_header = Some(self.current_status_header.clone());
        }
        self.set_status_header(message);
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
    pub(super) fn defer_or_handle(
        &mut self,
        push: impl FnOnce(&mut InterruptManager),
        handle: impl FnOnce(&mut Self),
    ) {
        // Preserve deterministic FIFO across queued interrupts: once anything
        // is queued due to an active write cycle, continue queueing until the
        // queue is flushed to avoid reordering (e.g., ExecEnd before ExecBegin).
        if self.stream_controller.is_some() || !self.interrupts.is_empty() {
            push(&mut self.interrupts);
        } else {
            handle(self);
        }
    }

    #[inline]
    pub(super) fn handle_streaming_delta(&mut self, delta: String) {
        // Before streaming agent content, flush any active exec cell group.
        // EXCEPT: Don't flush incomplete ExecCells - they should remain visible in
        // active_cell during streaming. Streaming content goes to history (scrollback),
        // while active_cell renders separately at the bottom. Flushing incomplete
        // ExecCells would move them to pending_exec_cells, making them invisible
        // until task completion.
        let should_flush = self
            .active_cell
            .as_ref()
            .map(|cell| {
                cell.as_any()
                    .downcast_ref::<ExecCell>()
                    .map(|exec| !exec.is_active())
                    .unwrap_or(true)
            })
            .unwrap_or(true);

        if should_flush {
            self.flush_active_cell();
        }

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

    pub(crate) fn handle_exec_end_now(&mut self, ev: ExecCommandEndEvent) {
        let running = self.running_commands.remove(&ev.call_id);
        if self.suppressed_exec_calls.remove(&ev.call_id) {
            return;
        }
        let (command, parsed, source) = match running {
            Some(rc) => (rc.command, rc.parsed_cmd, rc.source),
            None => (
                vec![ev.call_id.clone()],
                Vec::new(),
                ExecCommandSource::Agent,
            ),
        };
        let is_unified_exec_interaction =
            matches!(source, ExecCommandSource::UnifiedExecInteraction);

        // First check if there's a pending ExecCell for this call_id
        // (saved when the incomplete cell was flushed due to streaming)
        if let Some(pending_cell) = self.pending_exec_cells.retrieve(&ev.call_id) {
            // Preserve any existing active_cell before replacing with pending cell.
            // This ensures cells aren't lost when multiple ExecCells exist concurrently
            // (e.g., when a new tool call begins after text streaming flushes an incomplete cell).
            self.flush_active_cell();
            // Move the pending cell to active_cell so we can complete it
            self.active_cell = Some(pending_cell);
        } else {
            // Normal flow: check if active_cell is an ExecCell
            let needs_new = self
                .active_cell
                .as_ref()
                .map(|cell| cell.as_any().downcast_ref::<ExecCell>().is_none())
                .unwrap_or(true);

            if needs_new {
                self.flush_active_cell();
                self.active_cell = Some(Box::new(new_active_exec_command(
                    ev.call_id.clone(),
                    command,
                    parsed,
                    source,
                    None,
                    self.config.animations,
                )));
            }
        }

        if let Some(cell) = self
            .active_cell
            .as_mut()
            .and_then(|c| c.as_any_mut().downcast_mut::<ExecCell>())
        {
            let output = if is_unified_exec_interaction {
                CommandOutput {
                    exit_code: ev.exit_code,
                    formatted_output: String::new(),
                    aggregated_output: String::new(),
                }
            } else {
                CommandOutput {
                    exit_code: ev.exit_code,
                    formatted_output: ev.formatted_output.clone(),
                    aggregated_output: ev.aggregated_output.clone(),
                }
            };
            cell.complete_call(&ev.call_id, output, ev.duration);

            let is_active = cell.is_active();
            let is_exploring = cell.is_exploring_cell();

            // After completing a call, decide whether to keep the cell or flush it:
            //
            // 1. If cell still has pending calls (is_active), KEEP IT IN active_cell
            //    so it remains visible during streaming. Previously it was saved to
            //    pending_exec_cells which made it invisible - that was the bug.
            //
            // 2. If cell is fully complete AND is an exploring cell, keep it in
            //    active_cell to allow grouping with subsequent exploring commands.
            //
            // 3. If cell is fully complete AND is NOT an exploring cell, flush it
            //    to history immediately.
            if !is_active && !is_exploring {
                self.flush_active_cell();
            }
        }
    }

    pub(crate) fn handle_patch_apply_end_now(
        &mut self,
        event: codex_core::protocol::PatchApplyEndEvent,
    ) {
        // Observe directories from file paths to potentially update footer git info.
        self.observe_directories_from_changes(&event.changes);

        // If the patch was successful, just let the "Edited" block stand.
        // Otherwise, add a failure block.
        if !event.success {
            self.add_to_history(history_cell::new_patch_apply_failure(event.stderr));
        }
    }

    /// Observes the parent directories of changed files to update the effective CWD tracker.
    /// If the effective CWD changes (after debounce), triggers a system info refresh.
    ///
    /// Uses the git repository root for the refresh directory rather than the file's parent
    /// to ensure git commands work correctly. Also skips directories that don't exist yet
    /// (which can happen when creating new files in new directories).
    pub(super) fn observe_directories_from_changes(
        &mut self,
        changes: &std::collections::HashMap<PathBuf, codex_core::protocol::FileChange>,
    ) {
        for file_path in changes.keys() {
            // Resolve relative paths against config.cwd before extracting parent
            let absolute_path = if file_path.is_absolute() {
                file_path.clone()
            } else {
                self.config.cwd.join(file_path)
            };

            if self.effective_cwd_tracker.observe_file_path(&absolute_path) {
                // Find the git root for this path, falling back to parent directory
                // This ensures git commands work correctly even when the immediate
                // parent directory doesn't exist yet (new file in new directory)
                let refresh_dir = crate::effective_cwd_tracker::find_git_root(&absolute_path)
                    .or_else(|| {
                        // Fall back to parent directory only if it exists
                        absolute_path
                            .parent()
                            .filter(|p| p.exists())
                            .map(std::path::Path::to_path_buf)
                    });

                if let Some(dir) = refresh_dir {
                    self.app_event_tx
                        .send(AppEvent::RefreshSystemInfoForDirectory {
                            dir,
                            agent: Some(self.config.model.clone()),
                        });
                }
            }
        }
    }

    pub(crate) fn handle_exec_approval_now(&mut self, id: String, ev: ExecApprovalRequestEvent) {
        self.flush_answer_stream_with_separator();
        let command = shlex::try_join(ev.command.iter().map(String::as_str))
            .unwrap_or_else(|_| ev.command.join(" "));
        self.notify(Notification::ExecApprovalRequested { command });

        let request = ApprovalRequest::Exec {
            id,
            command: ev.command,
            reason: ev.reason,
            risk: ev.risk,
        };
        self.bottom_pane.push_approval_request(request);
        self.request_redraw();
    }

    pub(crate) fn handle_apply_patch_approval_now(
        &mut self,
        id: String,
        ev: ApplyPatchApprovalRequestEvent,
    ) {
        self.flush_answer_stream_with_separator();

        let request = ApprovalRequest::ApplyPatch {
            id,
            reason: ev.reason,
            changes: ev.changes.clone(),
            cwd: self.config.cwd.clone(),
        };
        self.bottom_pane.push_approval_request(request);
        self.request_redraw();
        self.notify(Notification::EditApprovalRequested {
            cwd: self.config.cwd.clone(),
            changes: ev.changes.keys().cloned().collect(),
        });
    }

    pub(crate) fn handle_elicitation_request_now(&mut self, ev: ElicitationRequestEvent) {
        self.flush_answer_stream_with_separator();

        self.notify(Notification::ElicitationRequested {
            server_name: ev.server_name.clone(),
        });

        let request = ApprovalRequest::McpElicitation {
            server_name: ev.server_name,
            request_id: ev.id,
            message: ev.message,
        };
        self.bottom_pane.push_approval_request(request);
        self.request_redraw();
    }

    pub(crate) fn handle_exec_begin_now(&mut self, ev: ExecCommandBeginEvent) {
        // Track Bash tool call for session statistics
        self.session_stats.record_tool_call("Bash");

        // Check if any parsed commands are Read operations to SKILL.md files
        for parsed_cmd in &ev.parsed_cmd {
            if let codex_protocol::parse_command::ParsedCommand::Read { path, .. } = parsed_cmd
                && let Some(skill_name) = extract_skill_from_read_path(path.to_str())
            {
                self.session_stats.record_skill(&skill_name);
            }
        }

        // Observe the command's working directory to potentially update footer git info.
        // If the effective CWD changes (after debounce), trigger a system info refresh.
        if self.effective_cwd_tracker.observe_directory(ev.cwd.clone()) {
            self.app_event_tx
                .send(AppEvent::RefreshSystemInfoForDirectory {
                    dir: ev.cwd.clone(),
                    agent: Some(self.config.model.clone()),
                });
        }

        // Ensure the status indicator is visible while the command runs.
        self.running_commands.insert(
            ev.call_id.clone(),
            RunningCommand {
                command: ev.command.clone(),
                parsed_cmd: ev.parsed_cmd.clone(),
                source: ev.source,
            },
        );
        let is_wait_interaction = matches!(ev.source, ExecCommandSource::UnifiedExecInteraction)
            && ev
                .interaction_input
                .as_deref()
                .map(str::is_empty)
                .unwrap_or(true);
        let command_display = ev.command.join(" ");
        let should_suppress_unified_wait = is_wait_interaction
            && self
                .last_unified_wait
                .as_ref()
                .is_some_and(|wait| wait.is_duplicate(&command_display));
        if is_wait_interaction {
            self.last_unified_wait = Some(UnifiedExecWaitState::new(command_display));
        } else {
            self.last_unified_wait = None;
        }
        if should_suppress_unified_wait {
            self.suppressed_exec_calls.insert(ev.call_id);
            return;
        }
        let interaction_input = ev.interaction_input.clone();

        // Check if we can add this call to an existing ExecCell
        if let Some(cell) = self
            .active_cell
            .as_mut()
            .and_then(|c| c.as_any_mut().downcast_mut::<ExecCell>())
            && let Some(new_exec) = cell.with_added_call(
                ev.call_id.clone(),
                ev.command.clone(),
                ev.parsed_cmd.clone(),
                ev.source,
                interaction_input.clone(),
            )
        {
            *cell = new_exec;
        } else {
            self.flush_active_cell();

            self.active_cell = Some(Box::new(new_active_exec_command(
                ev.call_id.clone(),
                ev.command.clone(),
                ev.parsed_cmd,
                ev.source,
                interaction_input,
                self.config.animations,
            )));
        }

        self.request_redraw();
    }

    pub(crate) fn handle_mcp_begin_now(&mut self, ev: McpToolCallBeginEvent) {
        // Track tool call for session statistics
        self.session_stats.record_tool_call(&ev.invocation.tool);

        // Check if this is a Skill tool call and extract skill name
        if ev.invocation.tool == "Skill"
            && let Some(skill_name) = extract_skill_from_raw_input(ev.invocation.arguments.as_ref())
        {
            self.session_stats.record_skill(&skill_name);
        }

        // Check if this is a Task tool call and extract subagent type
        if ev.invocation.tool == "Task"
            && let Some(subagent_type) =
                extract_subagent_from_raw_input(ev.invocation.arguments.as_ref())
        {
            self.session_stats.record_subagent(&subagent_type);
        }

        self.flush_answer_stream_with_separator();
        self.flush_active_cell();
        self.active_cell = Some(Box::new(history_cell::new_active_mcp_tool_call(
            ev.call_id,
            ev.invocation,
            self.config.animations,
        )));
        self.request_redraw();
    }

    pub(crate) fn handle_mcp_end_now(&mut self, ev: McpToolCallEndEvent) {
        self.flush_answer_stream_with_separator();

        let McpToolCallEndEvent {
            call_id,
            invocation,
            duration,
            result,
        } = ev;

        // If this is a Task tool call, scan the result text for skill paths
        // This captures skills used by subagents whose tool calls are not directly visible
        if invocation.tool == "Task"
            && let Ok(tool_result) = &result
        {
            for content_block in &tool_result.content {
                if let mcp_types::ContentBlock::TextContent(text_content) = content_block {
                    for skill_name in extract_skills_from_text(&text_content.text) {
                        self.session_stats.record_skill(&skill_name);
                    }
                }
            }
        }

        let extra_cell = match self
            .active_cell
            .as_mut()
            .and_then(|cell| cell.as_any_mut().downcast_mut::<McpToolCallCell>())
        {
            Some(cell) if cell.call_id() == call_id => cell.complete(duration, result),
            _ => {
                self.flush_active_cell();
                let mut cell = history_cell::new_active_mcp_tool_call(
                    call_id,
                    invocation,
                    self.config.animations,
                );
                let extra_cell = cell.complete(duration, result);
                self.active_cell = Some(Box::new(cell));
                extra_cell
            }
        };

        self.flush_active_cell();
        if let Some(extra) = extra_cell {
            self.add_boxed_history(extra);
        }
    }

    /// Handle Ctrl-C key press.
    pub(super) fn on_ctrl_c(&mut self) {
        if self.bottom_pane.on_ctrl_c() == CancellationEvent::Handled {
            return;
        }

        if self.bottom_pane.is_task_running() {
            self.bottom_pane.show_ctrl_c_quit_hint();
            self.submit_op(Op::Interrupt);
            return;
        }

        self.submit_op(Op::Shutdown);
    }

    pub(super) fn on_list_mcp_tools(&mut self, ev: McpListToolsResponseEvent) {
        self.add_to_history(history_cell::new_mcp_tools_output(
            &self.config,
            ev.tools,
            ev.resources,
            ev.resource_templates,
            &ev.auth_statuses,
        ));
    }

    pub(super) fn on_list_custom_prompts(&mut self, ev: ListCustomPromptsResponseEvent) {
        let len = ev.custom_prompts.len();
        debug!("received {len} custom prompts");
        // Forward to bottom pane so the slash popup can show them now.
        self.bottom_pane.set_custom_prompts(ev.custom_prompts);
    }
}
