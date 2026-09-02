use crate::key_hint::has_ctrl_or_alt;
use crate::nori::hotkey_match::matches_binding;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Margin;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::StatefulWidgetRef;
use ratatui::widgets::Widget;
use ratatui::widgets::WidgetRef;

use super::chat_composer_history::ChatComposerHistory;
use super::command_popup::CommandItem;
use super::command_popup::CommandPopup;
use super::file_search_popup::FileSearchPopup;
use super::footer::FooterMode;
use super::footer::FooterProps;
use super::footer::esc_hint_mode;
use super::footer::footer_height;
use super::footer::render_footer;
use super::footer::reset_mode_after_activity;
use super::footer::textarea_corner_segments;
use super::footer::toggle_shortcut_mode;
use super::history_search_popup::HistorySearchPopup;
use super::paste_burst::CharDecision;
use super::paste_burst::PasteBurst;
use super::skill_popup::SkillPickerItem;
use super::skill_popup::SkillPopup;
use crate::bottom_pane::paste_burst::FlushResult;
use crate::bottom_pane::prompt_args::expand_custom_prompt;
use crate::bottom_pane::prompt_args::expand_if_numeric_with_positional_args;
use crate::bottom_pane::prompt_args::extract_positional_args_for_prompt_line;
use crate::bottom_pane::prompt_args::parse_slash_name;
use crate::bottom_pane::prompt_args::prompt_argument_names;
use crate::bottom_pane::prompt_args::prompt_command_with_arg_placeholders;
use crate::bottom_pane::prompt_args::prompt_has_numeric_placeholders;
use crate::render::Insets;
use crate::render::RectExt;
use crate::render::renderable::Renderable;
use crate::slash_command::SlashCommand;
use crate::slash_command::built_in_slash_commands;
use crate::style::user_message_style;
use nori_harness::custom_prompts::CustomPrompt;
use nori_harness::custom_prompts::CustomPromptKind;
use nori_harness::custom_prompts::PROMPTS_CMD_PREFIX;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::textarea::TextArea;
use crate::bottom_pane::textarea::TextAreaState;
use crate::clipboard_paste::normalize_pasted_path;
use crate::clipboard_paste::pasted_image_format;
use crate::history_cell;
use crate::ui_consts::LIVE_PREFIX_COLS;
use codex_file_search::FileMatch;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

/// If the pasted content exceeds this number of characters, replace it with a
/// placeholder in the UI.
const LARGE_PASTE_CHAR_THRESHOLD: usize = 1000;
/// Result returned when the user interacts with the text area.
#[derive(Debug, PartialEq)]
pub enum InputResult {
    Submitted(String),
    Command(SlashCommand),
    None,
}

#[derive(Clone, Debug, PartialEq)]
struct AttachedImage {
    placeholder: String,
    path: PathBuf,
}

enum PromptSelectionMode {
    Completion,
    Submit,
}

enum PromptSelectionAction {
    Insert { text: String, cursor: Option<usize> },
    Submit { text: String },
}

#[derive(Clone, Copy)]
enum PromptIndicator {
    Normal,
    Shortcut,
    Slash,
    Shell,
}

impl PromptIndicator {
    fn hides_textarea_prefix(self) -> bool {
        matches!(self, PromptIndicator::Slash)
    }
}

pub(crate) struct ChatComposer {
    textarea: TextArea,
    textarea_state: RefCell<TextAreaState>,
    input_enabled: bool,
    active_popup: ActivePopup,
    app_event_tx: AppEventSender,
    history: ChatComposerHistory,
    ctrl_c_quit_hint: bool,
    esc_backtrack_hint: bool,
    use_shift_enter_hint: bool,
    dismissed_file_popup_token: Option<String>,
    dismissed_skill_popup_token: Option<String>,
    dismissed_command_popup_text: Option<String>,
    current_file_query: Option<String>,
    pending_pastes: Vec<(String, String)>,
    is_shell_mode: bool,
    has_focus: bool,
    attached_images: Vec<AttachedImage>,
    placeholder_text: String,
    is_task_running: bool,
    // Non-bracketed paste burst tracker.
    paste_burst: PasteBurst,
    // When true, disables paste-burst logic and inserts characters immediately.
    disable_paste_burst: bool,
    custom_prompts: Vec<CustomPrompt>,
    agent_commands: Vec<crate::presentation::AgentCommandInfo>,
    agent_command_prefix: String,
    command_description_overrides: HashMap<SlashCommand, Line<'static>>,
    disabled_builtin_commands: HashMap<SlashCommand, Line<'static>>,
    footer_mode: FooterMode,
    footer_hint_override: Option<Vec<(String, String)>>,
    context_window_percent: Option<i64>,
    session_usage: Option<crate::presentation::session_runtime::SessionUsageState>,
    system_info: Option<crate::system_info::SystemInfo>,
    /// The approval mode label to display in the footer (e.g., "Read Only", "Agent", "Full Access").
    approval_mode_label: Option<String>,
    acp_mode_label: Option<String>,
    /// Cloud session id shown in the footer when attached to a cloud session.
    cloud_session: Option<String>,
    vim_enter_behavior: nori_config::VimEnterBehavior,
    vertical_footer: bool,
    prompt_summary: Option<String>,
    /// Agent-supplied session title, sanitized for single-line display.
    session_title: Option<String>,
    footer_segment_config: nori_config::FooterSegmentConfig,
    footer_layout_config: nori_config::FooterLayoutConfig,
}

/// Popup state – at most one can be visible at any time.
enum ActivePopup {
    None,
    Command(CommandPopup),
    Skill(SkillPopup),
    File(FileSearchPopup),
    HistorySearch(HistorySearchPopup),
}

const FOOTER_SPACING_HEIGHT: u16 = 0;

mod key_handling;
mod paste_handling;
mod popup_management;
mod rendering;

impl ChatComposer {
    pub fn new(
        has_input_focus: bool,
        app_event_tx: AppEventSender,
        enhanced_keys_supported: bool,
        placeholder_text: String,
        disable_paste_burst: bool,
    ) -> Self {
        let use_shift_enter_hint = enhanced_keys_supported;

        let mut this = Self {
            textarea: TextArea::new(),
            textarea_state: RefCell::new(TextAreaState::default()),
            input_enabled: true,
            active_popup: ActivePopup::None,
            app_event_tx,
            history: ChatComposerHistory::new(),
            ctrl_c_quit_hint: false,
            esc_backtrack_hint: false,
            use_shift_enter_hint,
            dismissed_file_popup_token: None,
            dismissed_skill_popup_token: None,
            dismissed_command_popup_text: None,
            current_file_query: None,
            pending_pastes: Vec::new(),
            is_shell_mode: false,
            has_focus: has_input_focus,
            attached_images: Vec::new(),
            placeholder_text,
            is_task_running: false,
            paste_burst: PasteBurst::default(),
            disable_paste_burst: false,
            custom_prompts: Vec::new(),
            agent_commands: Vec::new(),
            agent_command_prefix: String::new(),
            command_description_overrides: HashMap::new(),
            disabled_builtin_commands: HashMap::new(),
            footer_mode: FooterMode::ShortcutSummary,
            footer_hint_override: None,
            context_window_percent: None,
            session_usage: None,
            system_info: None,
            approval_mode_label: None,
            acp_mode_label: None,
            cloud_session: None,
            vim_enter_behavior: nori_config::VimEnterBehavior::Off,
            vertical_footer: false,
            prompt_summary: None,
            session_title: None,
            footer_segment_config: nori_config::FooterSegmentConfig::default(),
            footer_layout_config: nori_config::FooterLayoutConfig::default(),
        };
        // Apply configuration via the setter to keep side-effects centralized.
        this.set_disable_paste_burst(disable_paste_burst);
        this
    }

    /// Returns true if the composer currently contains no user input.
    pub(crate) fn is_empty(&self) -> bool {
        !self.is_shell_mode && self.textarea.is_empty()
    }

    /// Record the history metadata advertised by `SessionConfiguredEvent` so
    /// that the composer can navigate cross-session history.
    pub(crate) fn set_history_metadata(&mut self, log_id: u64, entry_count: usize) {
        self.history.set_metadata(log_id, entry_count);
    }

    pub(crate) fn record_local_history_submission(&mut self, text: &str) {
        self.history.record_local_submission(text);
    }

    pub(crate) fn set_vertical_footer(&mut self, vertical_footer: bool) {
        self.vertical_footer = vertical_footer;
    }

    pub(crate) fn set_footer_segment_config(&mut self, config: nori_config::FooterSegmentConfig) {
        self.footer_segment_config = config;
    }

    pub(crate) fn set_footer_layout_config(&mut self, config: nori_config::FooterLayoutConfig) {
        self.footer_layout_config = config;
    }

    pub(crate) fn set_hotkey_config(&mut self, config: nori_config::HotkeyConfig) {
        self.textarea.set_hotkey_config(config);
    }

    pub(crate) fn set_vim_mode(&mut self, value: nori_config::VimEnterBehavior) {
        self.vim_enter_behavior = value;
        self.textarea.set_vim_mode_enabled(value.is_enabled());
    }

    pub(crate) fn should_handle_vim_insert_escape(&self, key_event: KeyEvent) -> bool {
        self.textarea.should_handle_vim_insert_escape(key_event)
    }

    pub(crate) fn should_handle_vim_escape_during_task(&self, key_event: KeyEvent) -> bool {
        self.popup_active()
            || self
                .textarea
                .should_handle_vim_escape_during_task(key_event)
    }

    pub(crate) fn is_vim_operator_pending(&self) -> bool {
        self.textarea.is_in_vim_normal_mode() && self.textarea.is_vim_operator_pending()
    }

    /// Set a footer segment's enabled state.
    pub(crate) fn set_footer_segment_enabled(
        &mut self,
        segment: nori_config::FooterSegment,
        enabled: bool,
    ) {
        self.footer_segment_config.set_enabled(segment, enabled);
    }

    /// Returns the current vim mode state (for testing).
    #[cfg(test)]
    pub(crate) fn vim_mode_state(&self) -> crate::bottom_pane::textarea::VimModeState {
        self.textarea.vim_mode_state()
    }

    /// Integrate an asynchronous response to an on-demand history lookup. If
    /// the entry is present and the offset matches the current cursor we
    /// immediately populate the textarea.
    pub(crate) fn on_history_entry_response(
        &mut self,
        log_id: u64,
        offset: usize,
        entry: Option<String>,
    ) -> bool {
        let Some(text) = self.history.on_entry_response(log_id, offset, entry) else {
            return false;
        };
        self.set_history_content(text);
        true
    }

    /// Deliver search history results to the popup (if still open).
    pub(crate) fn on_search_history_response(&mut self, entries: Vec<nori_harness::HistoryEntry>) {
        if let ActivePopup::HistorySearch(popup) = &mut self.active_popup {
            popup.set_entries(entries);
        }
    }

    /// Override the footer hint items displayed beneath the composer. Passing
    /// `None` restores the default shortcut footer.
    pub(crate) fn set_footer_hint_override(&mut self, items: Option<Vec<(String, String)>>) {
        self.footer_hint_override = items;
    }

    pub(super) fn input_enabled(&self) -> bool {
        self.input_enabled
    }

    /// Replace the entire composer content with `text` and reset cursor.
    pub(crate) fn set_text_content(&mut self, text: String) {
        if !self.input_enabled {
            return;
        }
        // Clear any existing content, placeholders, and attachments first.
        self.textarea.set_text("");
        self.pending_pastes.clear();
        self.attached_images.clear();
        if let Some(shell_body) = text.strip_prefix('!') {
            self.is_shell_mode = true;
            self.textarea.set_text(shell_body);
        } else {
            self.is_shell_mode = false;
            self.textarea.set_text(&text);
        }
        self.textarea.set_cursor(0);
        self.sync_selection_popups();
    }

    /// Restore text rejected after submission without discarding draft attachments.
    pub(crate) fn restore_submission_text(&mut self, text: String) {
        if !self.input_enabled {
            return;
        }
        self.is_shell_mode = false;
        self.textarea.set_text(&text);
        self.textarea.set_cursor(text.len());
        self.sync_selection_popups();
    }

    fn set_history_content(&mut self, text: String) {
        if !self.input_enabled {
            return;
        }
        self.set_text_content(text);
        self.textarea.set_cursor(self.textarea.text().len());
    }

    pub(crate) fn clear_for_ctrl_c(&mut self) -> Option<String> {
        if !self.input_enabled || self.is_empty() {
            return None;
        }
        let previous = self.current_text();
        self.set_text_content(String::new());
        self.history.reset_navigation();
        self.history.record_local_submission(&previous);
        Some(previous)
    }

    /// Get the current composer text.
    pub(crate) fn current_text(&self) -> String {
        if self.is_shell_mode {
            format!("!{}", self.textarea.text())
        } else {
            self.textarea.text().to_string()
        }
    }

    /// Attempt to start a burst by retro-capturing recent chars before the cursor.
    pub fn attach_image(&mut self, path: PathBuf, width: u32, height: u32, _format_label: &str) {
        if !self.input_enabled {
            return;
        }
        let file_label = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".to_string());
        let placeholder = format!("[{file_label} {width}x{height}]");
        // Insert as an element to match large paste placeholder behavior:
        // styled distinctly and treated atomically for cursor/mutations.
        self.textarea.insert_element(&placeholder);
        self.attached_images
            .push(AttachedImage { placeholder, path });
    }

    pub fn take_recent_submission_images(&mut self) -> Vec<PathBuf> {
        let images = std::mem::take(&mut self.attached_images);
        images.into_iter().map(|img| img.path).collect()
    }

    /// Integrate results from an asynchronous file search.
    pub(crate) fn on_file_search_result(&mut self, query: String, matches: Vec<FileMatch>) {
        // Only apply if user is still editing a token starting with `query`.
        let current_opt = Self::current_at_token(&self.textarea);
        let Some(current_token) = current_opt else {
            return;
        };

        if !current_token.starts_with(&query) {
            return;
        }

        if let ActivePopup::File(popup) = &mut self.active_popup {
            popup.set_matches(&query, matches);
        }
    }

    pub fn set_ctrl_c_quit_hint(&mut self, show: bool, has_focus: bool) {
        self.ctrl_c_quit_hint = show;
        if show {
            self.footer_mode = FooterMode::CtrlCReminder;
        } else {
            self.footer_mode = reset_mode_after_activity(self.footer_mode);
        }
        self.set_has_focus(has_focus);
    }

    pub(crate) fn insert_str(&mut self, text: &str) {
        if !self.input_enabled {
            return;
        }
        self.textarea.insert_str(text);
        self.sync_selection_popups();
    }

    pub(crate) fn show_exit_in_progress(&mut self) {
        self.input_enabled = false;
        self.active_popup = ActivePopup::None;
        self.footer_hint_override = Some(Vec::new());
        self.ctrl_c_quit_hint = false;
        self.esc_backtrack_hint = false;
        self.paste_burst.clear_after_explicit_paste();
    }

    fn set_has_focus(&mut self, has_focus: bool) {
        self.has_focus = has_focus;
    }

    pub fn set_task_running(&mut self, running: bool) {
        self.is_task_running = running;
    }

    pub(crate) fn set_session_usage(
        &mut self,
        usage: Option<crate::presentation::session_runtime::SessionUsageState>,
    ) {
        self.session_usage = usage;
    }

    pub(crate) fn set_system_info(&mut self, info: crate::system_info::SystemInfo) {
        self.system_info = Some(info);
    }

    pub(crate) fn set_approval_mode_label(&mut self, label: Option<String>) {
        self.approval_mode_label = label;
    }

    pub(crate) fn set_acp_mode_label(&mut self, label: Option<String>) {
        self.acp_mode_label = label;
    }

    pub(crate) fn set_cloud_session(&mut self, cloud_session: Option<String>) {
        self.cloud_session = cloud_session;
    }

    pub(crate) fn set_prompt_summary(&mut self, summary: Option<String>) {
        self.prompt_summary = summary;
    }

    pub(crate) fn set_session_title(&mut self, title: Option<String>) {
        self.session_title = title;
    }

    pub(crate) fn set_command_description_override(&mut self, cmd: SlashCommand, desc: String) {
        self.command_description_overrides
            .insert(cmd, Line::from(desc));
    }

    pub(crate) fn set_command_description_override_line(
        &mut self,
        cmd: SlashCommand,
        desc: Line<'static>,
    ) {
        self.command_description_overrides.insert(cmd, desc);
    }

    pub(crate) fn set_builtin_command_disabled(
        &mut self,
        cmd: SlashCommand,
        reason: Option<Line<'static>>,
    ) {
        if let Some(reason) = reason {
            self.disabled_builtin_commands.insert(cmd, reason);
        } else {
            self.disabled_builtin_commands.remove(&cmd);
        }
        if let ActivePopup::Command(popup) = &mut self.active_popup {
            popup.set_disabled_builtins(self.disabled_builtin_commands.clone());
        }
    }

    pub(crate) fn set_esc_backtrack_hint(&mut self, show: bool) {
        self.esc_backtrack_hint = show;
        if show {
            self.footer_mode = esc_hint_mode(self.footer_mode, self.is_task_running);
        } else {
            self.footer_mode = reset_mode_after_activity(self.footer_mode);
        }
    }

    /// Get the prompt summary for status card display.
    pub(crate) fn prompt_summary(&self) -> Option<String> {
        self.prompt_summary.clone()
    }

    /// Get the token breakdown from transcript location (for status card display).
    pub(crate) fn transcript_token_breakdown(&self) -> Option<nori_harness::TranscriptTokenUsage> {
        self.system_info
            .as_ref()
            .and_then(|s| s.transcript_location.as_ref())
            .and_then(|loc| loc.token_breakdown.clone())
    }
}

impl Renderable for ChatComposer {
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        if !self.input_enabled {
            return None;
        }
        let [_, textarea_rect, _] = self.layout_areas(area);
        let textarea_rect = Self::textarea_body_rect(textarea_rect, self.prompt_indicator());
        let state = *self.textarea_state.borrow();
        self.textarea.cursor_pos_with_state(textarea_rect, state)
    }

    fn desired_height(&self, width: u16) -> u16 {
        if !self.input_enabled {
            return 3;
        }
        let footer_props = self.footer_props();
        let footer_hint_height = self
            .custom_footer_height()
            .unwrap_or_else(|| footer_height(&footer_props));
        let footer_spacing = Self::footer_spacing(footer_hint_height);
        let footer_total_height = footer_hint_height + footer_spacing;
        const COLS_WITH_MARGIN: u16 = LIVE_PREFIX_COLS + 1;
        let hidden_prefix_cols = if self.prompt_indicator().hides_textarea_prefix() {
            1
        } else {
            0
        };
        self.textarea.desired_height(
            width
                .saturating_sub(COLS_WITH_MARGIN)
                .saturating_add(hidden_prefix_cols),
        ) + 2
            + match &self.active_popup {
                ActivePopup::None => footer_total_height,
                ActivePopup::Command(c) => c.calculate_required_height(width),
                ActivePopup::Skill(c) => c.calculate_required_height(width),
                ActivePopup::File(c) => c.calculate_required_height(),
                ActivePopup::HistorySearch(c) => c.calculate_required_height(),
            }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let [composer_rect, textarea_rect, popup_rect] = self.layout_areas(area);
        if !self.input_enabled {
            // Frozen exit state: no popups, no footer, no cursor — just a dim
            // prompt and message over the composer block.
            Block::default()
                .style(user_message_style())
                .render(composer_rect, buf);
            if !textarea_rect.is_empty() {
                let prompt = "›".dim();
                buf.set_span(
                    textarea_rect.x - LIVE_PREFIX_COLS,
                    textarea_rect.y,
                    &prompt,
                    textarea_rect.width,
                );
                Line::from("Exiting…".dim()).render(textarea_rect, buf);
            }
            return;
        }
        match &self.active_popup {
            ActivePopup::Command(popup) => {
                popup.render_ref(popup_rect, buf);
            }
            ActivePopup::Skill(popup) => {
                popup.render_ref(popup_rect, buf);
            }
            ActivePopup::File(popup) => {
                popup.render_ref(popup_rect, buf);
            }
            ActivePopup::HistorySearch(popup) => {
                popup.render_ref(popup_rect, buf);
            }
            ActivePopup::None => {
                let footer_props = self.footer_props();
                let custom_height = self.custom_footer_height();
                let footer_hint_height =
                    custom_height.unwrap_or_else(|| footer_height(&footer_props));
                let footer_spacing = Self::footer_spacing(footer_hint_height);
                let hint_rect = if footer_spacing > 0 && footer_hint_height > 0 {
                    let [_, hint_rect] = Layout::vertical([
                        Constraint::Length(footer_spacing),
                        Constraint::Length(footer_hint_height),
                    ])
                    .areas(popup_rect);
                    hint_rect
                } else {
                    popup_rect
                };
                if let Some(items) = self.footer_hint_override.as_ref() {
                    if !items.is_empty() {
                        let mut spans = Vec::with_capacity(items.len() * 4);
                        for (idx, (key, label)) in items.iter().enumerate() {
                            spans.push(" ".into());
                            spans.push(Span::styled(key.clone(), Style::default().bold()));
                            spans.push(format!(" {label}").into());
                            if idx + 1 != items.len() {
                                spans.push("   ".into());
                            }
                        }
                        let mut custom_rect = hint_rect;
                        if custom_rect.width > 2 {
                            custom_rect.x += 2;
                            custom_rect.width = custom_rect.width.saturating_sub(2);
                        }
                        Line::from(spans).render(custom_rect, buf);
                    }
                } else {
                    render_footer(hint_rect, buf, &footer_props);
                }
            }
        }
        let style = user_message_style();
        Block::default().style(style).render(composer_rect, buf);
        render_textarea_corner_segments(
            composer_rect,
            buf,
            textarea_corner_segments(&self.footer_props()),
        );
        let prompt_indicator = self.prompt_indicator();
        if !textarea_rect.is_empty() {
            let prompt = match prompt_indicator {
                PromptIndicator::Normal => "›".bold(),
                PromptIndicator::Shortcut => {
                    Span::styled("?", Style::default().fg(Color::DarkGray))
                }
                PromptIndicator::Slash => "/".cyan(),
                PromptIndicator::Shell => "!".red(),
            };
            buf.set_span(
                textarea_rect.x - LIVE_PREFIX_COLS,
                textarea_rect.y,
                &prompt,
                textarea_rect.width,
            );
        }

        let body_rect = Self::textarea_body_rect(textarea_rect, prompt_indicator);
        let mut state = self.textarea_state.borrow_mut();
        StatefulWidgetRef::render_ref(&(&self.textarea), body_rect, buf, &mut state);
        if prompt_indicator.hides_textarea_prefix() && !textarea_rect.is_empty() {
            buf.set_string(
                textarea_rect.x.saturating_sub(1),
                textarea_rect.y,
                " ",
                user_message_style(),
            );
        }
        if !self.is_shell_mode && self.textarea.text().is_empty() {
            let placeholder_text = if matches!(prompt_indicator, PromptIndicator::Shortcut) {
                self.placeholder_text
                    .strip_prefix("? ")
                    .unwrap_or(&self.placeholder_text)
            } else {
                &self.placeholder_text
            };
            let placeholder = Span::from(placeholder_text).dim();
            Line::from(vec![placeholder]).render(body_rect.inner(Margin::new(0, 0)), buf);
        }
    }
}

impl ChatComposer {
    fn textarea_body_rect(textarea_rect: Rect, prompt_indicator: PromptIndicator) -> Rect {
        if prompt_indicator.hides_textarea_prefix() {
            Rect {
                x: textarea_rect.x.saturating_sub(1),
                width: textarea_rect.width.saturating_add(1),
                ..textarea_rect
            }
        } else {
            textarea_rect
        }
    }

    fn prompt_indicator(&self) -> PromptIndicator {
        if self.is_shell_mode {
            PromptIndicator::Shell
        } else if self.textarea.text().starts_with('/') {
            PromptIndicator::Slash
        } else if matches!(self.footer_mode(), FooterMode::ShortcutOverlay) {
            PromptIndicator::Shortcut
        } else {
            PromptIndicator::Normal
        }
    }

    pub(super) fn is_editing_slash_command_name(&self) -> bool {
        let text = self.textarea.text();
        let first_line_end = text.find('\n').unwrap_or(text.len());
        let first_line = &text[..first_line_end];
        let cursor = self.textarea.cursor();
        let caret_on_first_line = cursor <= first_line_end;

        if !first_line.starts_with('/') || !caret_on_first_line {
            return false;
        }

        let token_end = first_line
            .char_indices()
            .find(|(_, c)| c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(first_line.len());
        cursor <= token_end
    }
}

fn render_textarea_corner_segments(
    composer_rect: Rect,
    buf: &mut Buffer,
    segments: super::footer::TextareaCornerSegments,
) {
    if composer_rect.is_empty() {
        return;
    }

    render_left_corner(composer_rect.x + 1, composer_rect.y, buf, segments.top_left);
    render_right_corner(
        composer_rect.right().saturating_sub(1),
        composer_rect.y,
        buf,
        segments.top_right,
    );

    let bottom_y = composer_rect.bottom().saturating_sub(1);
    render_left_corner(composer_rect.x + 1, bottom_y, buf, segments.bottom_left);
    render_right_corner(
        composer_rect.right().saturating_sub(1),
        bottom_y,
        buf,
        segments.bottom_right,
    );
}

fn render_left_corner(x: u16, y: u16, buf: &mut Buffer, line: Line<'static>) {
    let width = line.width() as u16;
    if width > 0 {
        line.render(Rect::new(x, y, width, 1), buf);
    }
}

fn render_right_corner(right_edge: u16, y: u16, buf: &mut Buffer, line: Line<'static>) {
    let width = line.width() as u16;
    if width == 0 {
        return;
    }
    let x = right_edge.saturating_sub(width).saturating_add(1);
    line.render(Rect::new(x, y, width, 1), buf);
}

fn prompt_selection_action(
    prompt: &CustomPrompt,
    first_line: &str,
    mode: PromptSelectionMode,
) -> PromptSelectionAction {
    let named_args = prompt_argument_names(&prompt.content);
    let has_numeric = prompt_has_numeric_placeholders(&prompt.content);

    match mode {
        PromptSelectionMode::Completion => {
            if !named_args.is_empty() {
                let (text, cursor) =
                    prompt_command_with_arg_placeholders(&prompt.name, &named_args);
                return PromptSelectionAction::Insert {
                    text,
                    cursor: Some(cursor),
                };
            }
            if has_numeric {
                let text = format!("/{PROMPTS_CMD_PREFIX}:{} ", prompt.name);
                return PromptSelectionAction::Insert { text, cursor: None };
            }
            let text = format!("/{PROMPTS_CMD_PREFIX}:{}", prompt.name);
            PromptSelectionAction::Insert { text, cursor: None }
        }
        PromptSelectionMode::Submit => {
            if !named_args.is_empty() {
                let (text, cursor) =
                    prompt_command_with_arg_placeholders(&prompt.name, &named_args);
                return PromptSelectionAction::Insert {
                    text,
                    cursor: Some(cursor),
                };
            }
            if has_numeric {
                if let Some(expanded) = expand_if_numeric_with_positional_args(prompt, first_line) {
                    return PromptSelectionAction::Submit { text: expanded };
                }
                let text = format!("/{PROMPTS_CMD_PREFIX}:{} ", prompt.name);
                return PromptSelectionAction::Insert { text, cursor: None };
            }
            PromptSelectionAction::Submit {
                text: prompt.content.clone(),
            }
        }
    }
}

#[cfg(test)]
mod tests;
