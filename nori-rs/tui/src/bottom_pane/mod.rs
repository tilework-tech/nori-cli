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
mod component_overlay_menu_view;
mod component_picker_view;
mod file_search_popup;
mod footer;
mod history_search_popup;
mod list_selection_view;
mod prompt_args;
pub(crate) use component_picker_view::ComponentPickerParams;
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
use nori_harness::custom_prompts::CustomPrompt;

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
    /// Display name of the current agent for use in approval dialogs.
    agent_display_name: String,
    /// Agent slug (e.g., "claude-code") used as prefix for agent commands.
    agent_slug: String,
    /// Whether vim mode is enabled, used to configure selection view behavior.
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
            agent_display_name,
            agent_slug,
            acp_wire_recording_enabled: false,
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
            let escape_handled = key_event.code == KeyCode::Esc
                && matches!(view.on_escape(), CancellationEvent::Handled);
            if escape_handled {
                if view.is_complete() {
                    self.view_stack.pop();
                    self.on_active_view_complete();
                }
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

    /// Show a generic list selection view with the provided items.
    pub(crate) fn show_selection_view(&mut self, params: list_selection_view::SelectionViewParams) {
        match params.presentation {
            list_selection_view::SelectionPresentation::List => {
                let view =
                    list_selection_view::ListSelectionView::new(params, self.app_event_tx.clone());
                self.push_view(Box::new(view));
            }
            list_selection_view::SelectionPresentation::Picker => {
                self.show_component_picker(ComponentPickerParams::from_selection(params));
            }
            list_selection_view::SelectionPresentation::Menu => {
                let view = component_overlay_menu_view::ComponentOverlayMenuView::new(
                    params,
                    self.app_event_tx.clone(),
                );
                self.push_view(Box::new(view));
            }
        }
    }

    /// Show a domain-free picker from the shared component crate, adapting
    /// typed outcomes into this application's event callbacks.
    pub(crate) fn show_component_picker(&mut self, params: ComponentPickerParams) {
        let view =
            component_picker_view::ComponentPickerView::new(params, self.app_event_tx.clone());
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
    pub(crate) fn set_agent_commands(
        &mut self,
        commands: Vec<crate::presentation::AgentCommandInfo>,
    ) {
        let prefix = self.agent_slug.clone();
        self.composer.set_agent_commands(commands, prefix);
        self.request_redraw();
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

    /// Update the agent-supplied session title displayed in the footer.
    pub(crate) fn set_session_title(&mut self, title: Option<String>) {
        self.composer.set_session_title(title);
        self.request_redraw();
    }

    /// Update ACP-reported session usage displayed in the footer.
    pub(crate) fn set_session_usage(
        &mut self,
        usage: Option<crate::presentation::session_runtime::SessionUsageState>,
    ) {
        self.composer.set_session_usage(usage);
        self.request_redraw();
    }

    /// Footer-derived values (git, context window, skillset version, titles)
    /// for the `/status` card.
    pub(crate) fn status_footer_values(&self) -> crate::nori::session_header::StatusFooterValues {
        self.composer.status_footer_values()
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
        statuses: &std::collections::HashMap<String, codex_rmcp_client::McpAuthStatus>,
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

    pub(crate) fn record_local_history_submission(&mut self, text: &str) {
        self.composer.record_local_history_submission(text);
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

    pub(crate) fn on_search_history_response(&mut self, entries: Vec<nori_harness::HistoryEntry>) {
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
    use crossterm::event::KeyModifiers;
    use insta::assert_snapshot;
    use nori_tui_components::PickerColumn;
    use nori_tui_components::PickerDensity;
    use nori_tui_components::PickerItem;
    use nori_tui_components::PickerState;
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
        ApprovalRequest {
            request_id: "1".to_string().into(),
            title: "Run echo ok".to_string(),
            kind: crate::presentation::ToolKind::Execute,
            cwd: std::env::current_dir().expect("current directory"),
            snapshot: Box::new(crate::presentation::ToolSnapshot {
                call_id: "call-1".to_string(),
                title: "Run echo ok".to_string(),
                kind: crate::presentation::ToolKind::Execute,
                phase: crate::presentation::ToolPhase::PendingApproval,
                locations: Vec::new(),
                invocation: Some(crate::presentation::Invocation::Command {
                    command: "echo ok".to_string(),
                }),
                artifacts: Vec::new(),
                raw_input: None,
                raw_output: None,
                owner_request_id: None,
            }),
            options: vec![nori_protocol::acp::v1::PermissionOption::new(
                nori_protocol::acp::v1::PermissionOptionId::new("allow-once"),
                "Allow",
                nori_protocol::acp::v1::PermissionOptionKind::AllowOnce,
            )],
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

    fn assert_selected_row_has_symmetric_rails(rendered: &str, label: &str) {
        let row = rendered
            .lines()
            .find(|row| row.contains(label))
            .unwrap_or_else(|| panic!("missing row containing {label:?}\n{rendered}"));
        let left = row.find('▏').expect("selected row left rail");
        let label = row.find(label).expect("selected row label");
        let right = row.find('▕').expect("selected row right rail");
        assert!(left < label && label < right, "{row}");
    }

    #[test]
    fn settings_selection_uses_the_shared_searchable_picker() {
        let mut pane = test_bottom_pane();
        let (tx, _rx) = unbounded_channel();
        pane.show_selection_view(crate::nori::config_picker::config_picker_params(
            &nori_config::NoriConfig::default(),
            AppEventSender::new(tx),
            None,
        ));

        let rendered = render_snapshot(&pane, Rect::new(0, 0, 100, 22));

        assert_selected_row_has_symmetric_rails(&rendered, "Pinned Plan Drawer");
        assert!(rendered.contains("/ search"), "{rendered}");
        assert_snapshot!("shared_settings_picker", rendered);

        pane.handle_key_event(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "resize reflow".chars() {
            pane.handle_key_event(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        let filtered = render_snapshot(&pane, Rect::new(0, 0, 100, 22));
        assert!(filtered.contains("Resize Reflow"), "{filtered}");
        assert!(!filtered.contains("Pinned Plan Drawer"), "{filtered}");
        assert_snapshot!("shared_settings_picker_search", filtered);
    }

    #[test]
    fn agent_selection_uses_the_shared_non_searchable_picker() {
        let mut pane = test_bottom_pane();
        let (tx, _rx) = unbounded_channel();
        pane.show_selection_view(crate::nori::agent_picker::agent_picker_params(
            "mock-model",
            AppEventSender::new(tx),
            false,
        ));

        let rendered = render_snapshot(&pane, Rect::new(0, 0, 100, 18));

        assert_selected_row_has_symmetric_rails(&rendered, "Mock ACP");
        assert!(!rendered.contains("/ search"), "{rendered}");
        assert!(rendered.contains("shift-tab rec on"), "{rendered}");
        assert_snapshot!("shared_agent_picker", rendered);

        pane.handle_key_event(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        pane.handle_key_event(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        let navigated = render_snapshot(&pane, Rect::new(0, 0, 100, 18));
        let mock_row = navigated
            .lines()
            .find(|row| row.contains("Mock ACP"))
            .expect("mock agent row");
        assert!(!mock_row.contains('▏') && !mock_row.contains('▕'));
        assert!(
            navigated
                .lines()
                .any(|row| row.contains('▏') && row.contains('▕')),
            "{navigated}"
        );
    }

    #[test]
    fn footer_segments_shared_picker_stays_open_after_a_toggle() {
        let (mut pane, mut rx) = test_bottom_pane_with_events();
        pane.show_selection_view(crate::nori::config_picker::footer_segments_picker_params(
            &nori_config::FooterSegmentConfig::default(),
            pane.app_event_tx.clone(),
        ));

        let rendered = render_snapshot(&pane, Rect::new(0, 0, 100, 18));
        assert_selected_row_has_symmetric_rails(&rendered, "Task Summary");

        pane.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(
            pane.has_active_view(),
            "the footer toggle picker should remain open after changing one segment"
        );
        assert!(
            matches!(rx.try_recv(), Ok(AppEvent::SetConfigFooterSegment(_, _))),
            "toggling a footer segment should still emit its config event"
        );
    }

    #[test]
    fn session_config_and_model_selections_use_the_shared_picker() {
        let option = nori_protocol::acp::v1::SessionConfigOption::select(
            "model",
            "Model",
            "stable",
            vec![
                nori_protocol::acp::v1::SessionConfigSelectOption::new("stable", "Stable"),
                nori_protocol::acp::v1::SessionConfigSelectOption::new("preview", "Preview"),
            ],
        )
        .category(nori_protocol::acp::v1::SessionConfigOptionCategory::Model);
        let mut pane = test_bottom_pane();

        pane.show_selection_view(
            crate::nori::session_config_picker::acp_session_config_picker_params(
                std::slice::from_ref(&option),
                None,
            ),
        );
        let config_render = render_snapshot(&pane, Rect::new(0, 0, 100, 16));
        assert_selected_row_has_symmetric_rails(&config_render, "Model");
        assert_snapshot!("shared_session_config_picker", config_render);

        pane.show_selection_view(
            crate::nori::session_config_picker::acp_session_config_value_picker_params(
                &option,
                &[nori_harness::OtherModel {
                    id: "experimental",
                    label: "Experimental",
                }],
            ),
        );
        let model_render = render_snapshot(&pane, Rect::new(0, 0, 100, 18));
        assert!(model_render.contains("Recommended"), "{model_render}");
        assert!(model_render.contains("Other"), "{model_render}");
        assert_selected_row_has_symmetric_rails(&model_render, "Stable");
        assert_snapshot!("shared_model_picker", model_render);
    }

    #[test]
    fn menu_selection_uses_the_shared_overlay_and_number_shortcuts() {
        let (mut pane, mut rx) = test_bottom_pane_with_events();
        pane.show_selection_view(
            SelectionViewParams {
                title: Some("Replace goal?".to_string()),
                subtitle: Some("Start the new objective now".to_string()),
                items: vec![
                    SelectionItem {
                        name: "Replace current goal".to_string(),
                        description: Some("Set the new objective and start it now".to_string()),
                        actions: vec![Box::new(|tx| tx.send(AppEvent::BeginExit))],
                        dismiss_on_select: true,
                        ..Default::default()
                    },
                    SelectionItem {
                        name: "Keep current goal".to_string(),
                        description: Some("Leave the current objective unchanged".to_string()),
                        dismiss_on_select: true,
                        ..Default::default()
                    },
                ],
                initial_selected_idx: Some(1),
                ..Default::default()
            }
            .menu(
                58,
                nori_tui_components::MenuDensity::Normal,
                nori_tui_components::MenuRowPattern::Plain,
                nori_tui_components::MenuPlacement::Centered,
            ),
        );

        assert!(
            pane.desired_height(80) >= 14,
            "subtitle-bearing menus reserve enough height to render their subtitle"
        );
        let rendered = render_snapshot(&pane, Rect::new(0, 0, 80, 16));
        assert!(rendered.contains("Replace goal?"), "{rendered}");
        assert!(rendered.contains("1  Replace current goal"), "{rendered}");
        assert!(rendered.contains("2  Keep current goal"), "{rendered}");
        assert_selected_row_has_symmetric_rails(&rendered, "Keep current goal");
        assert_snapshot!("shared_overlay_menu", rendered);

        pane.handle_key_event(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
        assert!(matches!(rx.try_recv(), Ok(AppEvent::BeginExit)));
        assert!(!pane.has_active_view());
    }

    #[test]
    fn searchable_selection_view_consumes_escape_before_bottom_pane_dismissal() {
        let mut pane = test_bottom_pane();
        pane.show_selection_view(SelectionViewParams {
            items: vec![SelectionItem {
                name: "Alpha".to_string(),
                search_value: Some("alpha".to_string()),
                ..Default::default()
            }],
            is_searchable: true,
            ..Default::default()
        });

        pane.handle_key_event(KeyEvent::new(
            KeyCode::Char('/'),
            crossterm::event::KeyModifiers::NONE,
        ));
        pane.handle_key_event(KeyEvent::new(
            KeyCode::Char('a'),
            crossterm::event::KeyModifiers::NONE,
        ));
        pane.handle_key_event(KeyEvent::new(
            KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));

        assert!(
            pane.has_active_view(),
            "first Escape should leave the searchable selection view open"
        );

        pane.handle_key_event(KeyEvent::new(
            KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(!pane.has_active_view());
    }

    #[test]
    fn searchable_component_picker_consumes_escape_before_bottom_pane_dismissal() {
        let mut pane = test_bottom_pane();
        pane.show_component_picker(ComponentPickerParams {
            state: PickerState::new(
                "Sessions",
                [PickerColumn::flexible("session", "Session")],
                [PickerItem::new("alpha".to_string(), "session", "Alpha")],
            ),
            actions: std::collections::BTreeMap::new(),
            on_dismiss: None,
            on_shift_tab: None,
            primary_column: "session".to_string(),
            detail_column: None,
            density: PickerDensity::Compact,
            show_title: true,
            show_details: true,
            keep_open: std::collections::BTreeSet::new(),
            footer_hints: None,
        });

        pane.handle_key_event(KeyEvent::new(
            KeyCode::Char('f'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        pane.handle_key_event(KeyEvent::new(
            KeyCode::Char('a'),
            crossterm::event::KeyModifiers::NONE,
        ));
        pane.handle_key_event(KeyEvent::new(
            KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));

        assert!(
            pane.has_active_view(),
            "first Escape should leave the component picker open"
        );

        pane.handle_key_event(KeyEvent::new(
            KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(!pane.has_active_view());
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
            Ok(AppEvent::HarnessAction(
                crate::app_event::HarnessAction::Cancel
            ))
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
            Ok(AppEvent::HarnessAction(
                crate::app_event::HarnessAction::Cancel
            ))
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
            Ok(AppEvent::HarnessAction(
                crate::app_event::HarnessAction::Cancel
            ))
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

        // Reject the schema-native permission request with Escape.
        use crossterm::event::KeyCode;
        use crossterm::event::KeyEvent;
        use crossterm::event::KeyModifiers;
        pane.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

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
