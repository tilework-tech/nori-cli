//! Bottom pane: shows the ChatComposer or a BottomPaneView, if one is active.
use std::path::PathBuf;

use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::queued_user_messages::QueuedUserMessages;
use crate::render::renderable::FlexRenderable;
use crate::render::renderable::Renderable;
use crate::render::renderable::RenderableItem;
use crate::tui::FrameRequester;
pub(crate) use bottom_pane_view::BottomPaneView;
use codex_file_search::FileMatch;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use std::time::Duration;

mod approval_overlay;
pub(crate) use approval_overlay::ApprovalOverlay;
pub(crate) use approval_overlay::ApprovalRequest;
mod bottom_pane_view;
mod chat_composer;
mod chat_composer_history;
mod command_popup;
mod file_search_popup;
mod footer;
mod history_search_popup;
mod list_selection_view;
mod prompt_args;
#[cfg(test)]
pub(crate) use list_selection_view::ListSelectionView;
pub(crate) use list_selection_view::SelectionViewParams;
mod paste_burst;
pub mod popup_consts;
mod queued_user_messages;
mod scroll_state;
mod selection_popup_common;
mod skill_popup;
mod textarea;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancellationEvent {
    Handled,
    NotHandled,
}

pub(crate) use chat_composer::ChatComposer;
pub(crate) use chat_composer::InputResult;
use codex_protocol::custom_prompts::CustomPrompt;

use crate::status_indicator_widget::StatusIndicatorWidget;
pub(crate) use list_selection_view::SelectionAction;
pub(crate) use list_selection_view::SelectionItem;

/// Pane displayed in the lower half of the chat UI.
pub(crate) struct BottomPane {
    /// Composer is retained even when a BottomPaneView is displayed so the
    /// input state is retained when the view is closed.
    composer: ChatComposer,

    /// Stack of views displayed instead of the composer (e.g. popups/modals).
    view_stack: Vec<Box<dyn BottomPaneView>>,

    app_event_tx: AppEventSender,
    frame_requester: FrameRequester,

    has_input_focus: bool,
    is_task_running: bool,
    ctrl_c_quit_hint: bool,
    esc_backtrack_hint: bool,
    animations_enabled: bool,
    custom_working_messages: bool,
    custom_working_message_list: Vec<String>,

    /// Inline status indicator shown above the composer while a task is running.
    status: Option<StatusIndicatorWidget>,
    /// Queued user messages to show above the composer while a turn is running.
    queued_user_messages: QueuedUserMessages,
    context_window_percent: Option<i64>,
    /// Display name of the current agent for use in approval dialogs.
    agent_display_name: String,
    /// Agent slug (e.g., "claude-code") used as prefix for agent commands.
    agent_slug: String,
    /// Whether vim mode is enabled, used to configure selection view behavior.
    vim_mode_enabled: bool,
    /// Whether ACP wire JSONL recording is enabled for future child subprocesses.
    acp_wire_recording_enabled: bool,
}

pub(crate) struct BottomPaneParams {
    pub(crate) app_event_tx: AppEventSender,
    pub(crate) frame_requester: FrameRequester,
    pub(crate) has_input_focus: bool,
    pub(crate) enhanced_keys_supported: bool,
    pub(crate) placeholder_text: String,
    pub(crate) disable_paste_burst: bool,
    pub(crate) animations_enabled: bool,
    pub(crate) custom_working_messages: bool,
    pub(crate) custom_working_message_list: Vec<String>,
    pub(crate) vertical_footer: bool,
    pub(crate) footer_segment_config: nori_config::FooterSegmentConfig,
    pub(crate) footer_layout_config: nori_config::FooterLayoutConfig,
    pub(crate) agent_display_name: String,
    pub(crate) agent_slug: String,
}

impl BottomPane {
    pub fn new(params: BottomPaneParams) -> Self {
        let BottomPaneParams {
            app_event_tx,
            frame_requester,
            has_input_focus,
            enhanced_keys_supported,
            placeholder_text,
            disable_paste_burst,
            animations_enabled,
            custom_working_messages,
            custom_working_message_list,
            vertical_footer,
            footer_segment_config,
            footer_layout_config,
            agent_display_name,
            agent_slug,
        } = params;
        let mut composer = ChatComposer::new(
            has_input_focus,
            app_event_tx.clone(),
            enhanced_keys_supported,
            placeholder_text,
            disable_paste_burst,
        );
        composer.set_vertical_footer(vertical_footer);
        composer.set_footer_segment_config(footer_segment_config);
        composer.set_footer_layout_config(footer_layout_config);

        // In debug builds, allow synchronous system info collection for E2E tests
        // via NORI_SYNC_SYSTEM_INFO=1. In release builds, always use default to
        // avoid blocking TUI startup.
        #[cfg(debug_assertions)]
        let system_info = if std::env::var("NORI_SYNC_SYSTEM_INFO").is_ok() {
            crate::system_info::SystemInfo::collect_sync()
        } else {
            crate::system_info::SystemInfo::default()
        };
        #[cfg(not(debug_assertions))]
        let system_info = crate::system_info::SystemInfo::default();
        composer.set_system_info(system_info);

        let acp_wire_recording_enabled = nori_config::NoriConfig::load()
            .map(|config| config.acp_proxy.enabled)
            .unwrap_or(false);

        let mut pane = Self {
            composer,
            view_stack: Vec::new(),
            app_event_tx,
            frame_requester,
            has_input_focus,
            is_task_running: false,
            ctrl_c_quit_hint: false,
            status: None,
            queued_user_messages: QueuedUserMessages::new(),
            esc_backtrack_hint: false,
            animations_enabled,
            custom_working_messages,
            custom_working_message_list,
            context_window_percent: None,
            agent_display_name,
            agent_slug,
            vim_mode_enabled: false,
            acp_wire_recording_enabled,
        };

        // Set description overrides for the slash command popup so that
        // /agent and /model show the current agent name from startup.
        if !pane.agent_display_name.is_empty() {
            let name = pane.agent_display_name.clone();
            pane.set_agent_display_name(name);
        }

        pane
    }

    pub fn status_widget(&self) -> Option<&StatusIndicatorWidget> {
        self.status.as_ref()
    }

    pub(crate) fn context_window_percent(&self) -> Option<i64> {
        self.context_window_percent
    }

    fn active_view(&self) -> Option<&dyn BottomPaneView> {
        self.view_stack.last().map(std::convert::AsRef::as_ref)
    }

    /// Returns true if a popup or custom view is currently active.
    pub(crate) fn has_active_view(&self) -> bool {
        !self.view_stack.is_empty()
    }

    fn push_view(&mut self, view: Box<dyn BottomPaneView>) {
        self.view_stack.push(view);
        self.request_redraw();
    }

    /// Forward a key event to the active view or the composer.
    pub fn handle_key_event(&mut self, key_event: KeyEvent) -> InputResult {
        // Once exit is in progress every key is inert, including the
        // task-status Esc interrupt below — teardown owns the backend.
        if !self.composer.input_enabled() {
            return InputResult::None;
        }
        // If a modal/view is active, handle it here; otherwise forward to composer.
        if let Some(view) = self.view_stack.last_mut() {
            if key_event.code == KeyCode::Esc
                && matches!(view.on_ctrl_c(), CancellationEvent::Handled)
                && view.is_complete()
            {
                self.view_stack.pop();
                self.on_active_view_complete();
            } else {
                view.handle_key_event(key_event);
                if view.is_complete() {
                    self.view_stack.clear();
                    self.on_active_view_complete();
                }
            }
            self.request_redraw();
            InputResult::None
        } else {
            // If a task is running and a status line is visible, allow Esc to
            // send an interrupt even while the composer has focus.
            if matches!(key_event.code, crossterm::event::KeyCode::Esc)
                && self.is_task_running
                && !self
                    .composer
                    .should_handle_vim_escape_during_task(key_event)
                && let Some(status) = &self.status
            {
                // Send Op::Interrupt
                status.interrupt();
                self.request_redraw();
                return InputResult::None;
            }
            let (input_result, needs_redraw) = self.composer.handle_key_event(key_event);
            if needs_redraw {
                self.request_redraw();
            }
            if self.composer.is_in_paste_burst() {
                self.request_redraw_in(ChatComposer::recommended_paste_flush_delay());
            }
            input_result
        }
    }

    /// Handle Ctrl-C in the bottom pane. If a modal view is active it gets a
    /// chance to consume the event (e.g. to dismiss itself).
    pub(crate) fn on_ctrl_c(&mut self) -> CancellationEvent {
        if let Some(view) = self.view_stack.last_mut() {
            let event = view.on_ctrl_c();
            if matches!(event, CancellationEvent::Handled) {
                if view.is_complete() {
                    self.view_stack.pop();
                    self.on_active_view_complete();
                }
                self.show_ctrl_c_quit_hint();
            }
            event
        } else if self.composer_is_empty() {
            CancellationEvent::NotHandled
        } else {
            self.view_stack.pop();
            self.clear_composer_for_ctrl_c();
            self.show_ctrl_c_quit_hint();
            CancellationEvent::Handled
        }
    }

    pub(crate) fn show_exit_in_progress(&mut self) {
        self.view_stack.clear();
        self.composer.show_exit_in_progress();
        self.request_redraw();
    }

    pub fn handle_paste(&mut self, pasted: String) {
        if let Some(view) = self.view_stack.last_mut() {
            let needs_redraw = view.handle_paste(pasted);
            if view.is_complete() {
                self.on_active_view_complete();
            }
            if needs_redraw {
                self.request_redraw();
            }
        } else {
            let needs_redraw = self.composer.handle_paste(pasted);
            if needs_redraw {
                self.request_redraw();
            }
        }
    }

    pub(crate) fn insert_str(&mut self, text: &str) {
        self.composer.insert_str(text);
        self.request_redraw();
    }

    /// Replace the composer text with `text`.
    pub(crate) fn set_composer_text(&mut self, text: String) {
        self.composer.set_text_content(text);
        self.request_redraw();
    }

    pub(crate) fn clear_composer_for_ctrl_c(&mut self) {
        self.composer.clear_for_ctrl_c();
        self.request_redraw();
    }

    /// Get the current composer text (for tests and programmatic checks).
    pub(crate) fn composer_text(&self) -> String {
        self.composer.current_text()
    }

    /// Update the animated header shown to the left of the brackets in the
    /// status indicator. No-ops if the status indicator is not active.
    pub(crate) fn update_status_header(&mut self, header: String) {
        if let Some(status) = self.status.as_mut() {
            status.update_header(header);
            self.request_redraw();
        }
    }

    pub(crate) fn show_ctrl_c_quit_hint(&mut self) {
        self.ctrl_c_quit_hint = true;
        self.composer
            .set_ctrl_c_quit_hint(true, self.has_input_focus);
        self.request_redraw();
    }

    pub(crate) fn clear_ctrl_c_quit_hint(&mut self) {
        if self.ctrl_c_quit_hint {
            self.ctrl_c_quit_hint = false;
            self.composer
                .set_ctrl_c_quit_hint(false, self.has_input_focus);
            self.request_redraw();
        }
    }

    #[cfg(test)]
    pub(crate) fn ctrl_c_quit_hint_visible(&self) -> bool {
        self.ctrl_c_quit_hint
    }

    #[cfg(test)]
    pub(crate) fn status_indicator_visible(&self) -> bool {
        self.status.is_some()
    }

    pub(crate) fn show_esc_backtrack_hint(&mut self) {
        self.esc_backtrack_hint = true;
        self.composer.set_esc_backtrack_hint(true);
        self.request_redraw();
    }

    pub(crate) fn clear_esc_backtrack_hint(&mut self) {
        if self.esc_backtrack_hint {
            self.esc_backtrack_hint = false;
            self.composer.set_esc_backtrack_hint(false);
            self.request_redraw();
        }
    }

    // esc_backtrack_hint_visible removed; hints are controlled internally.

    pub fn set_task_running(&mut self, running: bool) {
        let was_running = self.is_task_running;
        self.is_task_running = running;
        self.composer.set_task_running(running);

        if running {
            if !was_running {
                if self.status.is_none() {
                    self.status = Some(StatusIndicatorWidget::new(
                        self.app_event_tx.clone(),
                        self.frame_requester.clone(),
                        self.animations_enabled,
                        self.custom_working_messages,
                        self.custom_working_message_list.clone(),
                    ));
                }
                if let Some(status) = self.status.as_mut() {
                    status.set_interrupt_hint_visible(true);
                }
                self.request_redraw();
            }
        } else {
            // Hide the status indicator when a task completes, but keep other modal views.
            self.hide_status_indicator();
        }
    }

    /// Hide the status indicator while leaving task-running state untouched.
    pub(crate) fn hide_status_indicator(&mut self) {
        if self.status.take().is_some() {
            self.request_redraw();
        }
    }

    pub(crate) fn ensure_status_indicator(&mut self) {
        if self.status.is_none() {
            self.status = Some(StatusIndicatorWidget::new(
                self.app_event_tx.clone(),
                self.frame_requester.clone(),
                self.animations_enabled,
                self.custom_working_messages,
                self.custom_working_message_list.clone(),
            ));
            self.request_redraw();
        }
    }

    pub(crate) fn set_interrupt_hint_visible(&mut self, visible: bool) {
        if let Some(status) = self.status.as_mut() {
            status.set_interrupt_hint_visible(visible);
            self.request_redraw();
        }
    }

    pub(crate) fn set_context_window_percent(&mut self, percent: Option<i64>) {
        if self.context_window_percent == percent {
            return;
        }

        self.context_window_percent = percent;
        self.composer.set_context_window_percent(percent);
        self.request_redraw();
    }

    /// Update the agent display name used in approval dialogs and slash command descriptions.
    pub(crate) fn set_agent_display_name(&mut self, name: String) {
        self.agent_display_name = name;
        self.refresh_agent_command_descriptions();
    }

    pub(crate) fn agent_display_name(&self) -> &str {
        &self.agent_display_name
    }

    pub(crate) fn set_acp_wire_recording_enabled(&mut self, enabled: bool) {
        self.acp_wire_recording_enabled = enabled;
        self.refresh_agent_command_descriptions();
        self.request_redraw();
    }

    fn refresh_agent_command_descriptions(&mut self) {
        let name = self.agent_display_name.clone();
        self.composer.set_command_description_override_line(
            crate::slash_command::SlashCommand::Agent,
            agent_command_description(&name, self.acp_wire_recording_enabled),
        );
        self.composer.set_command_description_override(
            crate::slash_command::SlashCommand::Model,
            format!(
                "{} (current: {name})",
                crate::slash_command::SlashCommand::Model.description()
            ),
        );
    }

    pub(crate) fn set_builtin_command_disabled(
        &mut self,
        cmd: crate::slash_command::SlashCommand,
        reason: Option<Line<'static>>,
    ) {
        self.composer.set_builtin_command_disabled(cmd, reason);
        self.request_redraw();
    }

    /// Set the vertical footer layout flag.
    pub(crate) fn set_vertical_footer(&mut self, vertical_footer: bool) {
        self.composer.set_vertical_footer(vertical_footer);
    }

    pub(crate) fn set_custom_working_messages(&mut self, enabled: bool) {
        self.custom_working_messages = enabled;
        if let Some(status) = self.status.as_mut() {
            status.update_header(crate::status_indicator_widget::pick_status_message(
                enabled,
                &self.custom_working_message_list,
            ));
            self.request_redraw();
        }
    }

    /// Update the hotkey configuration used by the textarea for editing bindings.
    pub(crate) fn set_hotkey_config(&mut self, config: nori_config::HotkeyConfig) {
        self.composer.set_hotkey_config(config);
    }

    pub(crate) fn set_vim_mode(&mut self, value: nori_config::VimEnterBehavior) {
        self.vim_mode_enabled = value.is_enabled();
        self.composer.set_vim_mode(value);
    }

    pub(crate) fn should_handle_vim_insert_escape(&self, key_event: KeyEvent) -> bool {
        self.composer.should_handle_vim_insert_escape(key_event)
    }

    /// Set a footer segment's enabled state.
    pub(crate) fn set_footer_segment_enabled(
        &mut self,
        segment: nori_config::FooterSegment,
        enabled: bool,
    ) {
        self.composer.set_footer_segment_enabled(segment, enabled);
    }

    #[cfg(test)]
    pub(crate) fn footer_segment_config(&self) -> nori_config::FooterSegmentConfig {
        self.composer.footer_segment_config()
    }

    /// Show a generic list selection view with the provided items.
    pub(crate) fn show_selection_view(
        &mut self,
        mut params: list_selection_view::SelectionViewParams,
    ) {
        // Automatically inject vim mode for searchable views so all callers
        // get the correct behavior without having to pass it explicitly.
        if params.is_searchable {
            params.vim_mode = self.vim_mode_enabled;
        }
        let view = list_selection_view::ListSelectionView::new(params, self.app_event_tx.clone());
        self.push_view(Box::new(view));
    }

    /// Replace the current top-of-stack selection view with a new one.
    ///
    /// This pops the existing view before pushing the replacement so the stack
    /// does not grow on repeated refreshes (e.g. toggling footer segments).
    pub(crate) fn replace_selection_view(
        &mut self,
        params: list_selection_view::SelectionViewParams,
    ) {
        debug_assert!(
            !self.view_stack.is_empty(),
            "replace_selection_view called with empty view stack"
        );
        self.view_stack.pop();
        self.show_selection_view(params);
    }

    pub(crate) fn update_selection_item(
        &mut self,
        stable_id: &str,
        name: String,
        description: Option<String>,
        search_value: String,
    ) {
        if let Some(view) = self.view_stack.last_mut()
            && view.update_selection_item(stable_id, name, description, search_value)
        {
            self.request_redraw();
        }
    }

    pub(crate) fn remove_selection_item(&mut self, stable_id: &str) {
        if let Some(view) = self.view_stack.last_mut()
            && view.remove_selection_item(stable_id)
        {
            self.request_redraw();
        }
    }

    /// Update the queued messages preview shown above the composer.
    pub(crate) fn set_queued_user_messages(&mut self, queued: Vec<String>) {
        self.queued_user_messages.messages = queued;
        self.request_redraw();
    }

    /// Update custom prompts available for the slash popup.
    pub(crate) fn set_custom_prompts(&mut self, prompts: Vec<CustomPrompt>) {
        self.composer.set_custom_prompts(prompts);
        self.request_redraw();
    }

    /// Update agent-provided commands available for the slash popup.
    pub(crate) fn set_agent_commands(&mut self, commands: Vec<nori_protocol::AgentCommandInfo>) {
        let prefix = self.agent_slug.clone();
        self.composer.set_agent_commands(commands, prefix);
        self.request_redraw();
    }

    /// Set the agent slug used as prefix for agent commands (e.g., "claude-code").
    /// Also refreshes the prefix on any already-stored agent commands.
    pub(crate) fn set_agent_slug(&mut self, slug: String) {
        self.agent_slug = slug.clone();
        self.composer.update_agent_command_prefix(slug);
    }

    /// Update system info displayed in the footer (for background refresh).
    pub(crate) fn set_system_info(&mut self, info: crate::system_info::SystemInfo) {
        self.composer.set_system_info(info);
        self.request_redraw();
    }

    /// Update the approval mode label displayed in the footer and the slash
    /// command description override for `/approvals`.
    pub(crate) fn set_approval_mode_label(&mut self, label: Option<String>) {
        if let Some(ref mode) = label {
            self.composer.set_command_description_override(
                crate::slash_command::SlashCommand::Approvals,
                format!(
                    "{} (current: {mode})",
                    crate::slash_command::SlashCommand::Approvals.description()
                ),
            );
        }
        self.composer.set_approval_mode_label(label);
        self.request_redraw();
    }

    pub(crate) fn set_acp_mode_label(&mut self, label: Option<String>) {
        self.composer.set_acp_mode_label(label);
        self.request_redraw();
    }

    /// Update the cloud session id displayed in the footer (None hides it).
    pub(crate) fn set_cloud_session(&mut self, cloud_session: Option<String>) {
        self.composer.set_cloud_session(cloud_session);
        self.request_redraw();
    }

    /// Update the prompt summary displayed in the footer.
    pub(crate) fn set_prompt_summary(&mut self, summary: Option<String>) {
        self.composer.set_prompt_summary(summary);
        self.request_redraw();
    }

    /// Update ACP-reported session usage displayed in the footer.
    pub(crate) fn set_session_usage(
        &mut self,
        usage: Option<nori_protocol::session_runtime::SessionUsageState>,
    ) {
        self.composer.set_session_usage(usage);
        self.request_redraw();
    }

    /// Get the prompt summary for status card display.
    pub(crate) fn prompt_summary(&self) -> Option<String> {
        self.composer.prompt_summary()
    }

    /// Get the token breakdown from transcript location (for status card display).
    pub(crate) fn transcript_token_breakdown(&self) -> Option<nori_harness::TranscriptTokenUsage> {
        self.composer.transcript_token_breakdown()
    }

    pub(crate) fn composer_is_empty(&self) -> bool {
        self.composer.is_empty()
    }

    pub(crate) fn is_task_running(&self) -> bool {
        self.is_task_running
    }

    pub(crate) fn has_active_overlay_or_popup(&self) -> bool {
        !self.view_stack.is_empty() || self.composer.popup_active()
    }

    /// Return true when the pane is in the regular composer state without any
    /// overlays or popups and not running a task. This is the safe context to
    /// use Esc-Esc for backtracking from the main view.
    pub(crate) fn is_normal_backtrack_mode(&self) -> bool {
        !self.is_task_running && !self.has_active_overlay_or_popup()
    }

    pub(crate) fn show_view(&mut self, view: Box<dyn BottomPaneView>) {
        self.push_view(view);
    }

    /// Forward MCP auth statuses to the active view (if any).
    pub(crate) fn update_mcp_auth_statuses(
        &mut self,
        statuses: &std::collections::HashMap<String, codex_protocol::protocol::McpAuthStatus>,
    ) {
        if let Some(view) = self.view_stack.last_mut() {
            view.update_mcp_auth_statuses(statuses);
            self.request_redraw();
        }
    }

    /// Forward MCP OAuth completion to the active view (if any).
    pub(crate) fn handle_mcp_oauth_complete(&mut self, server_name: &str, success: bool) {
        if let Some(view) = self.view_stack.last_mut() {
            view.handle_mcp_oauth_complete(server_name, success);
            self.request_redraw();
        }
    }

    /// Called when the agent requests user approval.
    pub fn push_approval_request(&mut self, request: ApprovalRequest) {
        let request = if let Some(view) = self.view_stack.last_mut() {
            match view.try_consume_approval_request(request) {
                Some(request) => request,
                None => {
                    self.request_redraw();
                    return;
                }
            }
        } else {
            request
        };

        // Otherwise create a new approval modal overlay.
        let modal = ApprovalOverlay::new(
            request,
            self.app_event_tx.clone(),
            self.agent_display_name.clone(),
        );
        self.pause_status_timer_for_modal();
        self.push_view(Box::new(modal));
    }

    fn on_active_view_complete(&mut self) {
        self.resume_status_timer_after_modal();
    }

    fn pause_status_timer_for_modal(&mut self) {
        if let Some(status) = self.status.as_mut() {
            status.pause_timer();
        }
    }

    fn resume_status_timer_after_modal(&mut self) {
        if let Some(status) = self.status.as_mut() {
            status.resume_timer();
        }
    }

    /// Height (terminal rows) required by the current bottom pane.
    pub(crate) fn request_redraw(&self) {
        self.frame_requester.schedule_frame();
    }

    pub(crate) fn request_redraw_in(&self, dur: Duration) {
        self.frame_requester.schedule_frame_in(dur);
    }

    // --- History helpers ---

    pub(crate) fn set_history_metadata(&mut self, log_id: u64, entry_count: usize) {
        self.composer.set_history_metadata(log_id, entry_count);
    }

    pub(crate) fn flush_paste_burst_if_due(&mut self) -> bool {
        self.composer.flush_paste_burst_if_due()
    }

    pub(crate) fn is_in_paste_burst(&self) -> bool {
        self.composer.is_in_paste_burst()
    }

    pub(crate) fn on_history_entry_response(
        &mut self,
        log_id: u64,
        offset: usize,
        entry: Option<String>,
    ) {
        let updated = self
            .composer
            .on_history_entry_response(log_id, offset, entry);

        if updated {
            self.request_redraw();
        }
    }

    pub(crate) fn on_search_history_response(
        &mut self,
        entries: Vec<codex_protocol::message_history::HistoryEntry>,
    ) {
        self.composer.on_search_history_response(entries);
        self.request_redraw();
    }

    pub(crate) fn on_file_search_result(&mut self, query: String, matches: Vec<FileMatch>) {
        self.composer.on_file_search_result(query, matches);
        self.request_redraw();
    }

    pub(crate) fn attach_image(
        &mut self,
        path: PathBuf,
        width: u32,
        height: u32,
        format_label: &str,
    ) {
        if self.view_stack.is_empty() {
            self.composer
                .attach_image(path, width, height, format_label);
            self.request_redraw();
        }
    }

    pub(crate) fn take_recent_submission_images(&mut self) -> Vec<PathBuf> {
        self.composer.take_recent_submission_images()
    }

    fn as_renderable(&'_ self) -> RenderableItem<'_> {
        if let Some(view) = self.active_view() {
            RenderableItem::Borrowed(view)
        } else {
            let mut flex = FlexRenderable::new();
            if let Some(status) = &self.status {
                flex.push(0, RenderableItem::Borrowed(status));
            }
            flex.push(1, RenderableItem::Borrowed(&self.queued_user_messages));
            if self.status.is_some() || !self.queued_user_messages.messages.is_empty() {
                flex.push(0, RenderableItem::Owned("".into()));
            }
            let mut flex2 = FlexRenderable::new();
            flex2.push(1, RenderableItem::Owned(flex.into()));
            flex2.push(0, RenderableItem::Borrowed(&self.composer));
            RenderableItem::Owned(Box::new(flex2))
        }
    }
}

fn agent_command_description(agent_name: &str, recording_enabled: bool) -> Line<'static> {
    let command_description = crate::slash_command::SlashCommand::Agent.description();
    let prefix = if agent_name.is_empty() {
        command_description.to_string()
    } else {
        format!("{command_description} (current: {agent_name})")
    };
    let (symbol, status) = if recording_enabled {
        ("●", "on")
    } else {
        ("○", "off")
    };
    let symbol_span: Span<'static> = if recording_enabled {
        symbol.red()
    } else {
        symbol.into()
    };

    Line::from(vec![
        format!("{prefix} (").dim(),
        symbol_span,
        format!(" rec {status})").dim(),
    ])
}

impl Renderable for BottomPane {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.as_renderable().render(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.as_renderable().desired_height(width)
    }
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.as_renderable().cursor_pos(area)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use insta::assert_snapshot;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use tokio::sync::mpsc::error::TryRecvError;
    use tokio::sync::mpsc::unbounded_channel;

    fn snapshot_buffer(buf: &Buffer) -> String {
        let mut lines = Vec::new();
        for y in 0..buf.area().height {
            let mut row = String::new();
            for x in 0..buf.area().width {
                row.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
            lines.push(row);
        }
        lines.join("\n")
    }

    fn render_snapshot(pane: &BottomPane, area: Rect) -> String {
        let mut buf = Buffer::empty(area);
        pane.render(area, &mut buf);
        snapshot_buffer(&buf)
    }

    fn exec_request() -> ApprovalRequest {
        ApprovalRequest::Exec {
            id: "1".to_string(),
            command: vec!["echo".into(), "ok".into()],
            reason: None,
            risk: None,
        }
    }

    fn test_bottom_pane_with_events() -> (BottomPane, tokio::sync::mpsc::UnboundedReceiver<AppEvent>)
    {
        let (tx_raw, rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        (
            BottomPane::new(BottomPaneParams {
                app_event_tx: tx,
                frame_requester: FrameRequester::test_dummy(),
                has_input_focus: true,
                enhanced_keys_supported: false,
                placeholder_text: "Ask Nori to do anything".to_string(),
                disable_paste_burst: true,
                animations_enabled: true,
                custom_working_messages: true,
                custom_working_message_list: Vec::new(),
                vertical_footer: false,
                footer_segment_config: nori_config::FooterSegmentConfig::default(),
                footer_layout_config: nori_config::FooterLayoutConfig::default(),
                agent_display_name: String::new(),
                agent_slug: String::new(),
            }),
            rx,
        )
    }

    fn test_bottom_pane() -> BottomPane {
        test_bottom_pane_with_events().0
    }

    #[test]
    fn active_turn_vim_escape_enters_normal_then_debounces_interrupt() {
        let (mut pane, mut events) = test_bottom_pane_with_events();
        pane.set_vim_mode(nori_config::VimEnterBehavior::Submit);
        pane.set_task_running(true);

        let escape = KeyEvent::new(KeyCode::Esc, crossterm::event::KeyModifiers::NONE);
        pane.handle_key_event(escape);

        assert_eq!(
            pane.composer.vim_mode_state(),
            textarea::VimModeState::Normal
        );
        assert!(
            matches!(events.try_recv(), Err(TryRecvError::Empty)),
            "Insert-mode Escape must not interrupt the active turn",
        );

        pane.handle_key_event(escape);
        assert!(
            matches!(events.try_recv(), Err(TryRecvError::Empty)),
            "Escape must not interrupt immediately after entering Normal mode",
        );

        std::thread::sleep(Duration::from_millis(550));
        pane.handle_key_event(KeyEvent::new_with_kind(
            KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
            crossterm::event::KeyEventKind::Repeat,
        ));
        assert!(
            matches!(events.try_recv(), Err(TryRecvError::Empty)),
            "holding Escape must not interrupt the active turn",
        );

        pane.handle_key_event(escape);

        assert!(matches!(
            events.try_recv(),
            Ok(AppEvent::CodexOp(codex_protocol::protocol::Op::Interrupt))
        ));
    }

    #[test]
    fn active_turn_escape_cancels_pending_vim_operator_without_interrupting() {
        let (mut pane, mut events) = test_bottom_pane_with_events();
        pane.set_vim_mode(nori_config::VimEnterBehavior::Submit);
        pane.set_task_running(true);
        let escape = KeyEvent::new(KeyCode::Esc, crossterm::event::KeyModifiers::NONE);
        pane.handle_key_event(escape);
        std::thread::sleep(Duration::from_millis(550));

        pane.handle_key_event(KeyEvent::new(
            KeyCode::Char('d'),
            crossterm::event::KeyModifiers::NONE,
        ));
        pane.handle_key_event(escape);

        assert!(matches!(events.try_recv(), Err(TryRecvError::Empty)));

        pane.handle_key_event(KeyEvent::new(
            KeyCode::Char('i'),
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(
            pane.composer.vim_mode_state(),
            textarea::VimModeState::Insert
        );
    }

    #[test]
    fn active_turn_slash_picker_owns_escape_before_interrupt() {
        let (mut pane, mut events) = test_bottom_pane_with_events();
        let always_submit = *nori_config::VimEnterBehavior::all_variants()
            .iter()
            .find(|behavior| behavior.toml_value() == "always_submit")
            .expect("always-submit Vim behavior should be available");
        pane.set_vim_mode(always_submit);
        pane.set_task_running(true);

        pane.handle_key_event(KeyEvent::new(
            KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));
        pane.handle_key_event(KeyEvent::new(
            KeyCode::Char('/'),
            crossterm::event::KeyModifiers::NONE,
        ));
        pane.handle_key_event(KeyEvent::new(
            KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));
        std::thread::sleep(Duration::from_millis(550));

        pane.handle_key_event(KeyEvent::new(
            KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));

        assert!(!pane.composer.popup_active());
        assert!(matches!(events.try_recv(), Err(TryRecvError::Empty)));

        pane.handle_key_event(KeyEvent::new(
            KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(matches!(
            events.try_recv(),
            Ok(AppEvent::CodexOp(codex_protocol::protocol::Op::Interrupt))
        ));
    }

    #[test]
    fn active_turn_escape_interrupts_immediately_when_vim_is_disabled() {
        let (mut pane, mut events) = test_bottom_pane_with_events();
        pane.set_task_running(true);

        pane.handle_key_event(KeyEvent::new(
            KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));

        assert!(matches!(
            events.try_recv(),
            Ok(AppEvent::CodexOp(codex_protocol::protocol::Op::Interrupt))
        ));
    }

    #[test]
    fn active_overlay_or_popup_includes_active_views() {
        let mut pane = test_bottom_pane();

        assert!(!pane.has_active_overlay_or_popup());

        pane.push_approval_request(exec_request());

        assert!(pane.has_active_overlay_or_popup());
    }

    #[test]
    fn ctrl_c_on_modal_consumes_and_shows_quit_hint() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut pane = BottomPane::new(BottomPaneParams {
            app_event_tx: tx,
            frame_requester: FrameRequester::test_dummy(),
            has_input_focus: true,
            enhanced_keys_supported: false,
            placeholder_text: "Ask Nori to do anything".to_string(),
            disable_paste_burst: false,
            animations_enabled: true,
            custom_working_messages: true,
            custom_working_message_list: Vec::new(),
            vertical_footer: false,
            footer_segment_config: nori_config::FooterSegmentConfig::default(),
            footer_layout_config: nori_config::FooterLayoutConfig::default(),
            agent_display_name: String::new(),
            agent_slug: String::new(),
        });
        pane.push_approval_request(exec_request());
        assert_eq!(CancellationEvent::Handled, pane.on_ctrl_c());
        assert!(pane.ctrl_c_quit_hint_visible());
        assert_eq!(CancellationEvent::NotHandled, pane.on_ctrl_c());
    }

    // live ring removed; related tests deleted.

    #[test]
    fn overlay_not_shown_above_approval_modal() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut pane = BottomPane::new(BottomPaneParams {
            app_event_tx: tx,
            frame_requester: FrameRequester::test_dummy(),
            has_input_focus: true,
            enhanced_keys_supported: false,
            placeholder_text: "Ask Nori to do anything".to_string(),
            disable_paste_burst: false,
            animations_enabled: true,
            custom_working_messages: true,
            custom_working_message_list: Vec::new(),
            vertical_footer: false,
            footer_segment_config: nori_config::FooterSegmentConfig::default(),
            footer_layout_config: nori_config::FooterLayoutConfig::default(),
            agent_display_name: String::new(),
            agent_slug: String::new(),
        });

        // Create an approval modal (active view).
        pane.push_approval_request(exec_request());

        // Render and verify the top row does not include an overlay.
        let area = Rect::new(0, 0, 60, 6);
        let mut buf = Buffer::empty(area);
        pane.render(area, &mut buf);

        let mut r0 = String::new();
        for x in 0..area.width {
            r0.push(buf[(x, 0)].symbol().chars().next().unwrap_or(' '));
        }
        assert!(!r0.contains("•"), "overlay should not render above modal");
    }

    #[test]
    fn slash_agent_description_appends_recording_status_after_current_agent() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut pane = BottomPane::new(BottomPaneParams {
            app_event_tx: tx,
            frame_requester: FrameRequester::test_dummy(),
            has_input_focus: true,
            enhanced_keys_supported: false,
            placeholder_text: "Ask Nori to do anything".to_string(),
            disable_paste_burst: false,
            animations_enabled: true,
            custom_working_messages: true,
            custom_working_message_list: Vec::new(),
            vertical_footer: false,
            footer_segment_config: nori_config::FooterSegmentConfig::default(),
            footer_layout_config: nori_config::FooterLayoutConfig::default(),
            agent_display_name: "ElizACP".to_string(),
            agent_slug: "elizacp".to_string(),
        });

        pane.set_acp_wire_recording_enabled(false);
        for ch in ['/', 'a', 'g'] {
            let _ = pane.composer.handle_key_event(KeyEvent::new(
                KeyCode::Char(ch),
                crossterm::event::KeyModifiers::NONE,
            ));
            std::thread::sleep(ChatComposer::recommended_paste_flush_delay());
            let _ = pane.composer.flush_paste_burst_if_due();
        }

        let area = Rect::new(0, 0, 92, 6);
        let mut buf = Buffer::empty(area);
        pane.render(area, &mut buf);
        let rendered = snapshot_buffer(&buf);

        assert!(
            rendered.contains(
                "/agent  switch between available ACP agents (current: ElizACP) (○ rec off)"
            ),
            "expected /agent row to show current agent before recording status, got:\n{rendered}"
        );
    }

    #[test]
    fn composer_shown_after_denied_while_task_running() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut pane = BottomPane::new(BottomPaneParams {
            app_event_tx: tx,
            frame_requester: FrameRequester::test_dummy(),
            has_input_focus: true,
            enhanced_keys_supported: false,
            placeholder_text: "Ask Nori to do anything".to_string(),
            disable_paste_burst: false,
            animations_enabled: true,
            custom_working_messages: true,
            custom_working_message_list: Vec::new(),
            vertical_footer: false,
            footer_segment_config: nori_config::FooterSegmentConfig::default(),
            footer_layout_config: nori_config::FooterLayoutConfig::default(),
            agent_display_name: String::new(),
            agent_slug: String::new(),
        });

        // Start a running task so the status indicator is active above the composer.
        pane.set_task_running(true);

        // Push an approval modal (e.g., command approval) which should hide the status view.
        pane.push_approval_request(exec_request());

        // Simulate pressing 'n' (No) on the modal.
        use crossterm::event::KeyCode;
        use crossterm::event::KeyEvent;
        use crossterm::event::KeyModifiers;
        pane.handle_key_event(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

        // After denial, since the task is still running, the status indicator should be
        // visible above the composer. The modal should be gone.
        assert!(
            pane.view_stack.is_empty(),
            "no active modal view after denial"
        );

        // Render and ensure the top row includes the status indicator and a composer line below.
        // Give the animation thread a moment to tick.
        std::thread::sleep(Duration::from_millis(120));
        let area = Rect::new(0, 0, 40, 6);
        let mut buf = Buffer::empty(area);
        pane.render(area, &mut buf);
        let mut row0 = String::new();
        for x in 0..area.width {
            row0.push(buf[(x, 0)].symbol().chars().next().unwrap_or(' '));
        }
        assert!(
            pane.status_indicator_visible(),
            "expected status indicator visible after denial"
        );
        assert!(
            row0.contains("•"),
            "expected status indicator spinner on row 0: {row0:?}"
        );

        // Composer placeholder should be visible somewhere below.
        let mut found_composer = false;
        for y in 1..area.height {
            let mut row = String::new();
            for x in 0..area.width {
                row.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
            if row.contains("Ask Nori") {
                found_composer = true;
                break;
            }
        }
        assert!(
            found_composer,
            "expected composer visible under status line"
        );
    }

    #[test]
    fn status_indicator_visible_during_command_execution() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut pane = BottomPane::new(BottomPaneParams {
            app_event_tx: tx,
            frame_requester: FrameRequester::test_dummy(),
            has_input_focus: true,
            enhanced_keys_supported: false,
            placeholder_text: "Ask Nori to do anything".to_string(),
            disable_paste_burst: false,
            animations_enabled: true,
            custom_working_messages: true,
            custom_working_message_list: Vec::new(),
            vertical_footer: false,
            footer_segment_config: nori_config::FooterSegmentConfig::default(),
            footer_layout_config: nori_config::FooterLayoutConfig::default(),
            agent_display_name: String::new(),
            agent_slug: String::new(),
        });

        // Begin a task: show initial status.
        pane.set_task_running(true);

        // Use a height that allows the status line to be visible above the composer.
        let area = Rect::new(0, 0, 40, 6);
        let mut buf = Buffer::empty(area);
        pane.render(area, &mut buf);

        assert!(
            pane.status_indicator_visible(),
            "expected status indicator to be visible"
        );
    }

    #[test]
    fn status_and_composer_fill_height_without_bottom_padding() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut pane = BottomPane::new(BottomPaneParams {
            app_event_tx: tx,
            frame_requester: FrameRequester::test_dummy(),
            has_input_focus: true,
            enhanced_keys_supported: false,
            placeholder_text: "Ask Nori to do anything".to_string(),
            disable_paste_burst: false,
            animations_enabled: true,
            custom_working_messages: true,
            custom_working_message_list: Vec::new(),
            vertical_footer: false,
            footer_segment_config: nori_config::FooterSegmentConfig::default(),
            footer_layout_config: nori_config::FooterLayoutConfig::default(),
            agent_display_name: String::new(),
            agent_slug: String::new(),
        });

        // Activate spinner (status view replaces composer) with no live ring.
        pane.set_task_running(true);
        pane.update_status_header("Thinking really hard".to_string());

        // Use height == desired_height; expect spacer + status + composer rows without trailing padding.
        let height = pane.desired_height(30);
        assert!(
            height >= 3,
            "expected at least 3 rows to render spacer, status, and composer; got {height}"
        );
        let area = Rect::new(0, 0, 30, height);
        assert_snapshot!(
            "status_and_composer_fill_height_without_bottom_padding",
            render_snapshot(&pane, area)
        );
    }

    #[test]
    fn queued_messages_visible_when_status_hidden_snapshot() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut pane = BottomPane::new(BottomPaneParams {
            app_event_tx: tx,
            frame_requester: FrameRequester::test_dummy(),
            has_input_focus: true,
            enhanced_keys_supported: false,
            placeholder_text: "Ask Nori to do anything".to_string(),
            disable_paste_burst: false,
            animations_enabled: true,
            custom_working_messages: true,
            custom_working_message_list: Vec::new(),
            vertical_footer: false,
            footer_segment_config: nori_config::FooterSegmentConfig::default(),
            footer_layout_config: nori_config::FooterLayoutConfig::default(),
            agent_display_name: String::new(),
            agent_slug: String::new(),
        });

        pane.set_task_running(true);
        pane.set_queued_user_messages(vec!["Queued follow-up question".to_string()]);
        pane.hide_status_indicator();

        let width = 48;
        let height = pane.desired_height(width);
        let area = Rect::new(0, 0, width, height);
        assert_snapshot!(
            "queued_messages_visible_when_status_hidden_snapshot",
            render_snapshot(&pane, area)
        );
    }

    #[test]
    fn status_and_queued_messages_snapshot() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut pane = BottomPane::new(BottomPaneParams {
            app_event_tx: tx,
            frame_requester: FrameRequester::test_dummy(),
            has_input_focus: true,
            enhanced_keys_supported: false,
            placeholder_text: "Ask Nori to do anything".to_string(),
            disable_paste_burst: false,
            animations_enabled: true,
            custom_working_messages: true,
            custom_working_message_list: Vec::new(),
            vertical_footer: false,
            footer_segment_config: nori_config::FooterSegmentConfig::default(),
            footer_layout_config: nori_config::FooterLayoutConfig::default(),
            agent_display_name: String::new(),
            agent_slug: String::new(),
        });

        pane.set_task_running(true);
        pane.update_status_header("Thinking really hard".to_string());
        pane.set_queued_user_messages(vec!["Queued follow-up question".to_string()]);

        let width = 48;
        let height = pane.desired_height(width);
        let area = Rect::new(0, 0, width, height);
        assert_snapshot!(
            "status_and_queued_messages_snapshot",
            render_snapshot(&pane, area)
        );
    }

    #[test]
    fn replace_selection_view_does_not_stack() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut pane = BottomPane::new(BottomPaneParams {
            app_event_tx: tx,
            frame_requester: FrameRequester::test_dummy(),
            has_input_focus: true,
            enhanced_keys_supported: false,
            placeholder_text: "Ask Nori to do anything".to_string(),
            disable_paste_burst: false,
            animations_enabled: true,
            custom_working_messages: true,
            custom_working_message_list: Vec::new(),
            vertical_footer: false,
            footer_segment_config: nori_config::FooterSegmentConfig::default(),
            footer_layout_config: nori_config::FooterLayoutConfig::default(),
            agent_display_name: String::new(),
            agent_slug: String::new(),
        });

        // Push the initial selection view.
        let params1 = list_selection_view::SelectionViewParams {
            title: Some("Picker v1".to_string()),
            items: vec![list_selection_view::SelectionItem {
                name: "Item A".to_string(),
                dismiss_on_select: false,
                ..Default::default()
            }],
            ..Default::default()
        };
        pane.show_selection_view(params1);
        assert_eq!(pane.view_stack.len(), 1, "one view after initial push");

        // Replace with a new selection view — stack should stay at 1, not grow to 2.
        let params2 = list_selection_view::SelectionViewParams {
            title: Some("Picker v2".to_string()),
            items: vec![list_selection_view::SelectionItem {
                name: "Item B".to_string(),
                dismiss_on_select: false,
                ..Default::default()
            }],
            ..Default::default()
        };
        pane.replace_selection_view(params2);
        assert_eq!(
            pane.view_stack.len(),
            1,
            "view stack should not grow after replace"
        );

        // Verify the replacement view is actually on the stack by rendering.
        let area = Rect::new(0, 0, 40, 10);
        let snapshot = render_snapshot(&pane, area);
        assert!(
            snapshot.contains("Picker v2"),
            "expected replacement picker title in rendered output: {snapshot:?}"
        );
        assert!(
            !snapshot.contains("Picker v1"),
            "old picker title should not appear after replacement: {snapshot:?}"
        );
    }
}
