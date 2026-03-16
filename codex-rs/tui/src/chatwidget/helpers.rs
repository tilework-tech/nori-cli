use super::*;

impl ChatWidget {
    /// Set the agent in the widget's config copy.
    pub(crate) fn set_agent(&mut self, agent: &str) {
        self.session_header.set_agent(agent);
        self.config.model = agent.to_string();
        // Update the bottom pane's agent display name for approval dialogs
        let display_name = crate::nori::agent_picker::get_agent_info(agent)
            .map(|info| info.display_name)
            .unwrap_or_else(|| agent.to_string());
        self.bottom_pane.set_agent_display_name(display_name);
    }

    /// Set the vertical footer layout flag for the TUI.
    pub(crate) fn set_vertical_footer(&mut self, enabled: bool) {
        self.bottom_pane.set_vertical_footer(enabled);
    }

    /// Enable or disable the pinned plan drawer. The latest plan state is
    /// always retained so that re-enabling the drawer shows it immediately.
    pub(crate) fn set_pinned_plan_drawer(&mut self, enabled: bool) {
        self.pinned_plan_drawer = enabled;
    }

    /// Update the agent display name shown in approval dialogs.
    /// Used when ACP agent switch completes successfully.
    #[cfg(feature = "unstable")]
    pub(crate) fn update_agent_display_name(&mut self, display_name: String) {
        self.bottom_pane.set_agent_display_name(display_name);
    }

    pub(crate) fn add_info_message(&mut self, message: String, hint: Option<String>) {
        self.add_to_history(history_cell::new_info_event(message, hint));
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
        self.queue_user_message(UserMessage::from(text));
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
            &self.config.model,
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

    pub(crate) fn add_mcp_output(&mut self) {
        if self.config.mcp_servers.is_empty() {
            self.add_to_history(history_cell::empty_mcp_output());
        } else {
            self.submit_op(Op::ListMcpTools);
        }
    }

    /// Forward file-search results to the bottom pane.
    pub(crate) fn apply_file_search_result(&mut self, query: String, matches: Vec<FileMatch>) {
        self.bottom_pane.on_file_search_result(query, matches);
    }

    /// Update system info in the footer (for background refresh).
    pub(crate) fn apply_system_info_refresh(&mut self, info: crate::system_info::SystemInfo) {
        self.bottom_pane.set_system_info(info);
    }

    pub(crate) fn composer_text(&self) -> String {
        self.bottom_pane.composer_text()
    }

    /// Returns the first prompt text for this session, used for transcript matching.
    pub(crate) fn first_prompt_text(&self) -> Option<String> {
        self.first_prompt_text.clone()
    }

    /// Returns true if a popup or custom view is currently active in the bottom pane.
    pub(crate) fn has_active_popup(&self) -> bool {
        self.bottom_pane.has_active_view()
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

    /// Forward an `Op` directly to codex.
    pub(crate) fn submit_op(&self, op: Op) {
        // Record outbound operation for session replay fidelity.
        crate::session_log::log_outbound_op(&op);
        if let Err(e) = self.codex_op_tx.send(op) {
            tracing::error!("failed to submit op: {e}");
            // If we tried to send a Shutdown but the backend channel is dead,
            // trigger an exit directly since there is no backend to gracefully
            // shut down.
            if matches!(e.0, Op::Shutdown) {
                self.app_event_tx.send(AppEvent::ExitRequest);
            }
        }
    }

    pub(crate) fn token_usage(&self) -> TokenUsage {
        self.token_info
            .as_ref()
            .map(|ti| ti.total_token_usage.clone())
            .unwrap_or_default()
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

    /// Get a reference to the session statistics tracker.
    pub(crate) fn session_stats(&self) -> &SessionStats {
        &self.session_stats
    }

    pub(crate) fn clear_token_usage(&mut self) {
        self.token_info = None;
    }

    pub(super) fn as_renderable(&self) -> RenderableItem<'_> {
        let active_cell_renderable = match &self.active_cell {
            Some(cell) => RenderableItem::Borrowed(cell).inset(Insets::tlbr(1, 0, 0, 0)),
            None => RenderableItem::Owned(Box::new(())),
        };
        let mut flex = FlexRenderable::new();
        flex.push(1, active_cell_renderable);
        // Pinned plan drawer: renders the latest plan state between the active
        // cell and the bottom pane. When no plan has been received yet, this
        // contributes zero height. See `pinned_plan_drawer.rs` for the widget
        // and future collapsible mode TODO.
        if self.pinned_plan_drawer
            && let Some(plan) = &self.pinned_plan
        {
            flex.push(
                0,
                RenderableItem::Owned(Box::new(crate::pinned_plan_drawer::PinnedPlanDrawer::new(
                    plan,
                )))
                .inset(Insets::tlbr(1, 0, 0, 0)),
            );
        }
        flex.push(
            0,
            RenderableItem::Borrowed(&self.bottom_pane).inset(Insets::tlbr(1, 0, 0, 0)),
        );
        RenderableItem::Owned(Box::new(flex))
    }
}
