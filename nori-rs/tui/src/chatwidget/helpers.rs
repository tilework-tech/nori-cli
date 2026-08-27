use super::*;

/// Cloud detach gets a short opportunity to finish its stdin-EOF path. Local
/// agents are torn down immediately; they have no remote detach work to flush.
const CLOUD_EXIT_CHILD_GRACE: std::time::Duration = std::time::Duration::from_secs(1);

impl ChatWidget {
    /// Set the vertical footer layout flag for the TUI.
    pub(crate) fn set_vertical_footer(&mut self, enabled: bool) {
        self.bottom_pane.set_vertical_footer(enabled);
    }

    pub(crate) fn set_custom_working_messages(&mut self, enabled: bool) {
        self.config.custom_working_messages = enabled;
        self.bottom_pane.set_custom_working_messages(enabled);
    }

    /// Set the plan drawer mode. The latest plan state is always retained so
    /// that switching to a visible mode shows the most recent plan immediately.
    pub(crate) fn set_plan_drawer_mode(&mut self, mode: PlanDrawerMode) {
        self.plan_drawer_mode = mode;
    }

    /// Cycle the plan drawer: Off -> Collapsed -> Expanded -> Collapsed -> ...
    pub(crate) fn toggle_plan_drawer(&mut self) {
        self.plan_drawer_mode = match self.plan_drawer_mode {
            PlanDrawerMode::Off => PlanDrawerMode::Collapsed,
            PlanDrawerMode::Collapsed => PlanDrawerMode::Expanded,
            PlanDrawerMode::Expanded => PlanDrawerMode::Collapsed,
        };
    }

    /// Return the current plan drawer mode.
    pub(crate) fn plan_drawer_mode(&self) -> PlanDrawerMode {
        self.plan_drawer_mode
    }

    pub(crate) fn add_info_message(&mut self, message: String, hint: Option<String>) {
        self.add_to_history(history_cell::new_info_event(message, hint));
        self.request_redraw();
    }

    /// The agent capability view from the latest SessionCapabilitiesChanged.
    pub(crate) fn agent_capabilities(&self) -> crate::presentation::AgentCapabilitiesView {
        self.session_agent_capabilities.clone()
    }

    /// The cloud session identity. Id presence IS the cloud signal inside the
    /// widget: `on_session_started` retains it only when the top-level CLI
    /// launched handroll in cloud mode.
    pub(crate) fn cloud_session_identity(&self) -> Option<CloudSessionInfo> {
        self.acp_session_id.as_ref().map(|id| CloudSessionInfo {
            id: id.clone(),
            title: self.cloud_session_title.clone(),
        })
    }

    /// Push the current cloud identity (or its absence) into the footer.
    /// Called from SessionConfigured — the only event that changes the id.
    pub(super) fn refresh_cloud_session_indicator(&mut self) {
        let cloud_session = self.cloud_session_identity().map(|info| info.id);
        self.bottom_pane.set_cloud_session(cloud_session);
    }

    /// A `/close` failed: surface the (already enhanced) error and unblock the
    /// session-switching commands that were held while the close was in flight.
    pub(crate) fn on_session_close_failed(&mut self, message: String) {
        self.session_close_in_flight = false;
        self.add_error_message(format!("Failed to close the session: {message}"));
    }

    /// Begin exiting: immediate feedback, input refused, and bounded backend
    /// cleanup. Idempotent.
    ///
    /// In cloud mode, exit is a *detach*: the connection drop is non-terminal
    /// and the session keeps running server-side, so the feedback says exactly
    /// that.
    pub(crate) fn begin_exit(&mut self) {
        if self.exiting {
            return;
        }
        self.exiting = true;
        self.bottom_pane.show_exit_in_progress();

        if self.cloud_mode && self.acp_session_id.is_some() {
            self.add_to_history(history_cell::new_info_event(
                "This session keeps running in the cloud.".to_string(),
                Some("reattach later from the `nori cloud` picker".to_string()),
            ));
        }
        self.request_redraw();

        let child_grace = if self.cloud_mode {
            CLOUD_EXIT_CHILD_GRACE
        } else {
            std::time::Duration::ZERO
        };
        self.submit_harness_action(crate::app_event::HarnessAction::Shutdown { child_grace });
    }

    pub(crate) fn handle_acp_session_config_update(
        &mut self,
        config_options: &[nori_protocol::acp::v1::SessionConfigOption],
    ) {
        let next_snapshot =
            crate::nori::session_config_history::snapshot_from_options(config_options);

        if let Some(previous_snapshot) = &self.acp_config_option_snapshot {
            let changes = crate::nori::session_config_history::changed_values(
                previous_snapshot,
                config_options,
            );
            if !changes.is_empty() {
                self.add_to_history(
                    crate::nori::session_config_history::new_agent_options_history_cell(
                        self.bottom_pane.agent_display_name(),
                        &changes,
                    ),
                );
            }
        } else if !next_snapshot.is_empty() {
            self.add_to_history(
                crate::nori::session_config_history::new_agent_options_initial_history_cell(
                    self.bottom_pane.agent_display_name(),
                    config_options,
                ),
            );
        }

        self.acp_config_option_snapshot = Some(next_snapshot);
        self.apply_acp_mode_config_snapshot(
            self.acp_mode_config_generation,
            crate::nori::session_config_mode::acp_mode_config_from_options(config_options),
        );
        self.request_redraw();
    }

    pub(crate) fn sync_acp_session_config_snapshot(
        &mut self,
        config_options: &[nori_protocol::acp::v1::SessionConfigOption],
    ) {
        self.acp_config_option_snapshot = Some(
            crate::nori::session_config_history::snapshot_from_options(config_options),
        );
        self.apply_acp_mode_config_snapshot(
            self.acp_mode_config_generation,
            crate::nori::session_config_mode::acp_mode_config_from_options(config_options),
        );
    }

    pub(crate) fn handle_acp_session_config_snapshot(
        &mut self,
        generation: i64,
        config_options: &[nori_protocol::acp::v1::SessionConfigOption],
    ) {
        if generation != self.acp_mode_config_generation {
            return;
        }

        self.sync_acp_session_config_snapshot(config_options);
    }

    pub(crate) fn add_acp_session_config_set_message(
        &mut self,
        option_name: &str,
        value_name: &str,
        saved_as_default: bool,
    ) {
        self.add_to_history(
            crate::nori::session_config_history::new_agent_option_set_history_cell(
                self.bottom_pane.agent_display_name(),
                option_name,
                value_name,
                saved_as_default,
            ),
        );
        self.request_redraw();
    }

    pub(crate) fn add_plain_history_lines(&mut self, lines: Vec<Line<'static>>) {
        self.add_boxed_history(Box::new(PlainHistoryCell::new(lines)));
        self.request_redraw();
    }

    pub(crate) fn add_error_message(&mut self, message: String) {
        self.add_to_history(history_cell::new_error_event(message));
        self.request_redraw();
    }

    pub(crate) fn add_warning_message(&mut self, message: String) {
        self.add_to_history(history_cell::new_warning_event(message));
        self.request_redraw();
    }

    /// Queue a plain text message to be submitted as a user turn. If no task
    /// is currently running the message is submitted immediately; otherwise
    /// it is appended to the pending queue.
    pub(crate) fn queue_text_as_user_message(&mut self, text: String) {
        self.submit_user_message(UserMessage::from(text));
    }

    /// Show "Connecting to [Agent]" status indicator during agent startup.
    ///
    /// Called when an ACP agent is being spawned and may take time
    /// (e.g., npx/bunx resolving dependencies).
    pub(crate) fn show_connecting_status(&mut self, display_name: &str) {
        let header = format!("Connecting to {display_name}");
        self.bottom_pane.ensure_status_indicator();
        self.bottom_pane.set_interrupt_hint_visible(false); // Can't interrupt during connect
        self.set_status_header(header);
        self.request_redraw();
    }

    pub(crate) fn on_agent_spawn_failed(&mut self, agent_name: &str, error: &str) {
        self.bottom_pane.hide_status_indicator();
        self.add_error_message(format!("Failed to start agent '{agent_name}': {error}"));
        self.open_agent_popup();
    }

    pub(crate) fn add_memory_output(&mut self) {
        let files = crate::nori::session_header::active_instruction_file_contents(
            &self.config.active_agent,
            &self.config.cwd,
        );

        if files.is_empty() {
            self.add_info_message("No active instruction files found.".to_string(), None);
            return;
        }

        let mut lines: Vec<Line<'static>> = vec!["/memory".magenta().into()];

        for (path, contents) in files {
            let display_path = crate::nori::session_header::format_instruction_path(&path);
            lines.push(Line::from(""));
            lines.push(Line::from(display_path.bold()));
            for line in contents.lines() {
                lines.push(Line::from(line.to_string().dim()));
            }
        }

        self.add_plain_history_lines(lines);
    }

    /// Forward file-search results to the bottom pane.
    pub(crate) fn apply_file_search_result(&mut self, query: String, matches: Vec<FileMatch>) {
        self.bottom_pane.on_file_search_result(query, matches);
    }

    /// Update system info in the footer (for background refresh).
    pub(crate) fn apply_system_info_refresh(&mut self, info: crate::system_info::SystemInfo) {
        if let Some(transcript_location) = &info.transcript_location {
            for subagent in &transcript_location.subagents_used {
                self.session_stats.record_subagent(subagent);
            }
        }
        self.bottom_pane.set_system_info(info);
    }

    pub(crate) fn composer_text(&self) -> String {
        self.bottom_pane.composer_text()
    }

    /// Returns the first prompt text for this session, used for transcript matching.
    pub(crate) fn first_prompt_text(&self) -> Option<String> {
        self.first_prompt_text.clone()
    }

    pub(crate) fn take_initial_input(&mut self) -> (Option<String>, Vec<PathBuf>) {
        let Some(message) = self.initial_user_message.take() else {
            return (None, Vec::new());
        };
        (self.first_prompt_text.take(), message.image_paths)
    }

    /// Returns true if a popup or custom view is currently active in the bottom pane.
    pub(crate) fn has_active_popup(&self) -> bool {
        self.bottom_pane.has_active_view()
    }

    pub(crate) fn has_active_overlay_or_popup(&self) -> bool {
        self.bottom_pane.has_active_overlay_or_popup()
    }

    pub(crate) fn composer_is_empty(&self) -> bool {
        self.bottom_pane.composer_is_empty()
    }

    /// True when the UI is in the regular composer state with no running task,
    /// no modal overlay (e.g. approvals or status indicator), and no composer popups.
    /// In this state Esc-Esc backtracking is enabled.
    pub(crate) fn is_normal_backtrack_mode(&self) -> bool {
        self.bottom_pane.is_normal_backtrack_mode()
    }

    pub(crate) fn insert_str(&mut self, text: &str) {
        self.bottom_pane.insert_str(text);
    }

    /// Replace the composer content with the provided text and reset cursor.
    pub(crate) fn set_composer_text(&mut self, text: String) {
        self.bottom_pane.set_composer_text(text);
    }

    pub(crate) fn show_esc_backtrack_hint(&mut self) {
        self.bottom_pane.show_esc_backtrack_hint();
    }

    pub(crate) fn clear_esc_backtrack_hint(&mut self) {
        self.bottom_pane.clear_esc_backtrack_hint();
    }

    pub(crate) fn shutdown_harness_session(&self) {
        let Some(handle) = self.harness_handle.clone() else {
            return;
        };
        let child_grace = if self.cloud_mode {
            CLOUD_EXIT_CHILD_GRACE
        } else {
            std::time::Duration::ZERO
        };
        tokio::spawn(async move {
            if let Err(error) = handle.shutdown_with_grace(child_grace).await {
                tracing::warn!(%error, "failed to shut down replaced harness session");
            }
        });
    }

    pub(crate) fn harness_handle(&self) -> Option<HarnessHandle> {
        self.harness_handle.clone()
    }

    pub(crate) fn submit_harness_action(&self, action: crate::app_event::HarnessAction) {
        let Some(handle) = self.harness_handle.clone() else {
            if matches!(action, crate::app_event::HarnessAction::Shutdown { .. }) {
                self.app_event_tx.send(AppEvent::ExitRequest);
            } else {
                self.app_event_tx.send(AppEvent::HarnessActionFailed(
                    "No active harness session".to_string(),
                ));
            }
            return;
        };
        let app_event_tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            use crate::app_event::HarnessAction;
            let result = match action {
                HarnessAction::Cancel => handle.cancel().await,
                HarnessAction::Shutdown { child_grace } => {
                    let result = handle.shutdown_with_grace(child_grace).await;
                    if result.is_err() {
                        app_event_tx.send(AppEvent::ExitRequest);
                    }
                    result
                }
                HarnessAction::Compact => handle.compact().await,
                HarnessAction::Branch => handle.branch().await,
                HarnessAction::UndoTo(index) => handle.undo_to(index).await,
                HarnessAction::LoadUndoSnapshots => match handle.undo_snapshots().await {
                    Ok(snapshots) => {
                        app_event_tx.send(AppEvent::UndoSnapshotsLoaded(snapshots));
                        Ok(())
                    }
                    Err(error) => Err(error),
                },
                HarnessAction::RunUserShell(command) => handle.run_user_shell(command).await,
                HarnessAction::AddHistory(text) => handle.add_history(text).await,
                HarnessAction::HistoryEntry { log_id, offset } => {
                    match handle.history_entry(log_id, offset).await {
                        Ok(entry) => {
                            app_event_tx.send(AppEvent::HistoryEntryLoaded {
                                log_id,
                                offset,
                                entry,
                            });
                            Ok(())
                        }
                        Err(error) => Err(error),
                    }
                }
                HarnessAction::SearchHistory { max_results } => {
                    match handle.search_history(max_results).await {
                        Ok(entries) => {
                            app_event_tx.send(AppEvent::HistorySearchLoaded(entries));
                            Ok(())
                        }
                        Err(error) => Err(error),
                    }
                }
                HarnessAction::LoadCustomPrompts => match handle.custom_prompts().await {
                    Ok(prompts) => {
                        app_event_tx.send(AppEvent::CustomPromptsLoaded(prompts));
                        Ok(())
                    }
                    Err(error) => Err(error),
                },
                HarnessAction::LoadGoal => match handle.goal().await {
                    Ok(goal) => {
                        app_event_tx.send(AppEvent::GoalLoaded(goal));
                        Ok(())
                    }
                    Err(error) => Err(error),
                },
                HarnessAction::SetGoal { objective, status } => {
                    handle.set_goal(objective, status).await.map(drop)
                }
                HarnessAction::SetGoalStatus(status) => {
                    handle.set_goal_status(status).await.map(drop)
                }
                HarnessAction::ClearGoal => handle.clear_goal().await,
                HarnessAction::RespondToAgent {
                    request_id,
                    response,
                } => handle.respond_to_agent(request_id, *response).await,
            };
            if let Err(error) = result {
                app_event_tx.send(AppEvent::HarnessActionFailed(error.to_string()));
            }
        });
    }

    pub(crate) fn submit_prompt(&self, content: Vec<nori_protocol::acp::v1::ContentBlock>) {
        let Some(handle) = self.harness_handle.clone() else {
            self.app_event_tx.send(AppEvent::HarnessActionFailed(
                "No active harness session".to_string(),
            ));
            return;
        };
        let app_event_tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            if let Err(error) = handle.prompt(content).await {
                app_event_tx.send(AppEvent::HarnessActionFailed(error.to_string()));
            }
        });
    }

    pub(crate) fn token_usage(&self) -> crate::ui_types::TokenUsage {
        crate::ui_types::TokenUsage::default()
    }

    pub(crate) fn conversation_id(&self) -> Option<ConversationId> {
        self.conversation_id
    }

    pub(crate) fn rollout_path(&self) -> Option<PathBuf> {
        self.current_rollout_path.clone()
    }

    /// Return a reference to the widget's current config (includes any
    /// runtime overrides applied via TUI, e.g., model or approval policy).
    pub(crate) fn config_ref(&self) -> &Config {
        &self.config
    }

    /// Replace the runtime config snapshot after an in-TUI edit.
    pub(crate) fn set_config(&mut self, config: Config) {
        self.config = config;
    }

    /// Forward MCP auth statuses to the active bottom pane view.
    pub(crate) fn update_mcp_auth_statuses(
        &mut self,
        statuses: &std::collections::HashMap<String, codex_rmcp_client::McpAuthStatus>,
    ) {
        self.bottom_pane.update_mcp_auth_statuses(statuses);
    }

    /// Forward MCP OAuth completion to the active bottom pane view.
    pub(crate) fn handle_mcp_oauth_complete(&mut self, server_name: &str, success: bool) {
        self.bottom_pane
            .handle_mcp_oauth_complete(server_name, success);
    }

    /// Get a reference to the session statistics tracker.
    pub(crate) fn session_stats(&self) -> &SessionStats {
        &self.session_stats
    }

    pub(crate) fn clear_token_usage(&mut self) {}

    pub(super) fn as_renderable(&self) -> RenderableItem<'_> {
        let active_cell_renderable = match &self.active_cell {
            Some(cell) => RenderableItem::Borrowed(cell).inset(Insets::tlbr(1, 0, 0, 0)),
            None => RenderableItem::Owned(Box::new(())),
        };
        let mut flex = FlexRenderable::new();
        flex.push(1, active_cell_renderable);
        // Pinned plan drawer: renders the latest plan state between the active
        // cell and the bottom pane. When no plan has been received yet, the
        // guard on `pinned_plan` being `Some` means the drawer contributes
        // zero height.
        if let Some(plan) = &self.pinned_plan {
            match self.plan_drawer_mode {
                PlanDrawerMode::Collapsed => {
                    flex.push(
                        0,
                        RenderableItem::Owned(Box::new(
                            crate::pinned_plan_drawer::PinnedPlanDrawerCollapsed::new(plan),
                        ))
                        .inset(Insets::tlbr(1, 0, 0, 0)),
                    );
                }
                PlanDrawerMode::Expanded => {
                    flex.push(
                        0,
                        RenderableItem::Owned(Box::new(
                            crate::pinned_plan_drawer::PinnedPlanDrawer::new(plan),
                        ))
                        .inset(Insets::tlbr(1, 0, 0, 0)),
                    );
                }
                PlanDrawerMode::Off => {}
            }
        }
        flex.push(
            0,
            RenderableItem::Borrowed(&self.bottom_pane).inset(Insets::tlbr(1, 0, 0, 0)),
        );
        RenderableItem::Owned(Box::new(flex))
    }

    // --- Terminal title management ---

    /// Returns the project name derived from the working directory.
    fn project_name(&self) -> String {
        self.config
            .cwd
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// Whether the terminal title spinner should animate right now.
    pub(crate) fn should_animate_terminal_title_spinner(&self) -> bool {
        self.config.animations && self.terminal_title_has_active_progress()
    }

    /// Whether there is active progress that warrants showing the spinner.
    fn terminal_title_has_active_progress(&self) -> bool {
        self.bottom_pane.is_task_running()
    }

    /// Recompute and write the terminal title. Schedules the next animation
    /// frame if the spinner is active.
    pub(crate) fn refresh_terminal_title(&mut self) {
        let now = Instant::now();
        let spinner_frame = if self.should_animate_terminal_title_spinner() {
            Some(crate::terminal_title::spinner_frame_at(
                self.terminal_title_animation_origin,
                now,
            ))
        } else {
            None
        };

        let project = self.project_name();
        if project.is_empty() {
            return;
        }

        let title = crate::terminal_title::compose_title(&project, spinner_frame);

        // Skip redundant writes.
        if self.last_terminal_title.as_deref() == Some(&title) {
            // Still schedule the next frame so the animation continues.
            if spinner_frame.is_some() {
                self.frame_requester
                    .schedule_frame_in(crate::terminal_title::SPINNER_INTERVAL);
            }
            return;
        }

        if let Err(err) = crate::terminal_title::set_terminal_title(&title) {
            tracing::debug!(error = %err, "failed to set terminal title");
        }
        self.last_terminal_title = Some(title);

        if spinner_frame.is_some() {
            self.frame_requester
                .schedule_frame_in(crate::terminal_title::SPINNER_INTERVAL);
        }
    }

    /// Clear the managed terminal title and reset the cache.
    pub(crate) fn clear_managed_terminal_title(&mut self) -> std::io::Result<()> {
        self.last_terminal_title = None;
        crate::terminal_title::clear_terminal_title()
    }

    /// Called before every frame draw to advance the terminal title spinner.
    pub(crate) fn pre_draw_tick(&mut self) {
        if self.should_animate_terminal_title_spinner() {
            self.refresh_terminal_title();
        }
    }
}
