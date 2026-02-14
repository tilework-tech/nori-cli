use crate::key_hint::has_ctrl_or_alt;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Margin;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::StatefulWidgetRef;
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
use super::footer::toggle_shortcut_mode;
use super::history_search_popup::HistorySearchPopup;
use super::paste_burst::CharDecision;
use super::paste_burst::PasteBurst;
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
use codex_protocol::custom_prompts::CustomPrompt;
use codex_protocol::custom_prompts::CustomPromptKind;
use codex_protocol::custom_prompts::PROMPTS_CMD_PREFIX;

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

pub(crate) struct ChatComposer {
    textarea: TextArea,
    textarea_state: RefCell<TextAreaState>,
    active_popup: ActivePopup,
    app_event_tx: AppEventSender,
    history: ChatComposerHistory,
    ctrl_c_quit_hint: bool,
    esc_backtrack_hint: bool,
    use_shift_enter_hint: bool,
    dismissed_file_popup_token: Option<String>,
    current_file_query: Option<String>,
    pending_pastes: Vec<(String, String)>,
    has_focus: bool,
    attached_images: Vec<AttachedImage>,
    placeholder_text: String,
    is_task_running: bool,
    // Non-bracketed paste burst tracker.
    paste_burst: PasteBurst,
    // When true, disables paste-burst logic and inserts characters immediately.
    disable_paste_burst: bool,
    custom_prompts: Vec<CustomPrompt>,
    footer_mode: FooterMode,
    footer_hint_override: Option<Vec<(String, String)>>,
    context_window_percent: Option<i64>,
    system_info: Option<crate::system_info::SystemInfo>,
    /// The approval mode label to display in the footer (e.g., "Read Only", "Agent", "Full Access").
    approval_mode_label: Option<String>,
    vertical_footer: bool,
    prompt_summary: Option<String>,
    footer_segment_config: codex_acp::config::FooterSegmentConfig,
}

/// Popup state – at most one can be visible at any time.
enum ActivePopup {
    None,
    Command(CommandPopup),
    File(FileSearchPopup),
    HistorySearch(HistorySearchPopup),
}

const FOOTER_SPACING_HEIGHT: u16 = 0;

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
            active_popup: ActivePopup::None,
            app_event_tx,
            history: ChatComposerHistory::new(),
            ctrl_c_quit_hint: false,
            esc_backtrack_hint: false,
            use_shift_enter_hint,
            dismissed_file_popup_token: None,
            current_file_query: None,
            pending_pastes: Vec::new(),
            has_focus: has_input_focus,
            attached_images: Vec::new(),
            placeholder_text,
            is_task_running: false,
            paste_burst: PasteBurst::default(),
            disable_paste_burst: false,
            custom_prompts: Vec::new(),
            footer_mode: FooterMode::ShortcutSummary,
            footer_hint_override: None,
            context_window_percent: None,
            system_info: None,
            approval_mode_label: None,
            vertical_footer: false,
            prompt_summary: None,
            footer_segment_config: codex_acp::config::FooterSegmentConfig::default(),
        };
        // Apply configuration via the setter to keep side-effects centralized.
        this.set_disable_paste_burst(disable_paste_burst);
        this
    }

    fn layout_areas(&self, area: Rect) -> [Rect; 3] {
        let footer_props = self.footer_props();
        let footer_hint_height = self
            .custom_footer_height()
            .unwrap_or_else(|| footer_height(&footer_props));
        let footer_spacing = Self::footer_spacing(footer_hint_height);
        let footer_total_height = footer_hint_height + footer_spacing;
        let popup_constraint = match &self.active_popup {
            ActivePopup::Command(popup) => {
                Constraint::Max(popup.calculate_required_height(area.width))
            }
            ActivePopup::File(popup) => Constraint::Max(popup.calculate_required_height()),
            ActivePopup::HistorySearch(popup) => Constraint::Max(popup.calculate_required_height()),
            ActivePopup::None => Constraint::Max(footer_total_height),
        };
        let [composer_rect, popup_rect] =
            Layout::vertical([Constraint::Min(3), popup_constraint]).areas(area);
        let textarea_rect = composer_rect.inset(Insets::tlbr(1, LIVE_PREFIX_COLS, 1, 1));
        [composer_rect, textarea_rect, popup_rect]
    }

    fn footer_spacing(footer_hint_height: u16) -> u16 {
        if footer_hint_height == 0 {
            0
        } else {
            FOOTER_SPACING_HEIGHT
        }
    }

    /// Returns true if the composer currently contains no user input.
    pub(crate) fn is_empty(&self) -> bool {
        self.textarea.is_empty()
    }

    /// Record the history metadata advertised by `SessionConfiguredEvent` so
    /// that the composer can navigate cross-session history.
    pub(crate) fn set_history_metadata(&mut self, log_id: u64, entry_count: usize) {
        self.history.set_metadata(log_id, entry_count);
    }

    pub(crate) fn set_vertical_footer(&mut self, vertical_footer: bool) {
        self.vertical_footer = vertical_footer;
    }

    pub(crate) fn set_hotkey_config(&mut self, config: codex_acp::config::HotkeyConfig) {
        self.textarea.set_hotkey_config(config);
    }

    pub(crate) fn set_vim_mode_enabled(&mut self, enabled: bool) {
        self.textarea.set_vim_mode_enabled(enabled);
    }

    /// Set a footer segment's enabled state.
    #[cfg(feature = "nori-config")]
    pub(crate) fn set_footer_segment_enabled(
        &mut self,
        segment: codex_acp::config::FooterSegment,
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
        self.set_text_content(text);
        true
    }

    /// Deliver search history results to the popup (if still open).
    pub(crate) fn on_search_history_response(
        &mut self,
        entries: Vec<codex_protocol::message_history::HistoryEntry>,
    ) {
        if let ActivePopup::HistorySearch(popup) = &mut self.active_popup {
            popup.set_entries(entries);
        }
    }

    pub fn handle_paste(&mut self, pasted: String) -> bool {
        let char_count = pasted.chars().count();
        if char_count > LARGE_PASTE_CHAR_THRESHOLD {
            let placeholder = format!("[Pasted Content {char_count} chars]");
            self.textarea.insert_element(&placeholder);
            self.pending_pastes.push((placeholder, pasted));
        } else if char_count > 1 && self.handle_paste_image_path(pasted.clone()) {
            self.textarea.insert_str(" ");
        } else {
            self.textarea.insert_str(&pasted);
        }
        // Explicit paste events should not trigger Enter suppression.
        self.paste_burst.clear_after_explicit_paste();
        // Keep popup sync consistent with key handling: prefer slash popup; only
        // sync file popup when slash popup is NOT active.
        self.sync_command_popup();
        if matches!(self.active_popup, ActivePopup::Command(_)) {
            self.dismissed_file_popup_token = None;
        } else {
            self.sync_file_search_popup();
        }
        true
    }

    pub fn handle_paste_image_path(&mut self, pasted: String) -> bool {
        let Some(path_buf) = normalize_pasted_path(&pasted) else {
            return false;
        };

        match image::image_dimensions(&path_buf) {
            Ok((w, h)) => {
                tracing::info!("OK: {pasted}");
                let format_label = pasted_image_format(&path_buf).label();
                self.attach_image(path_buf, w, h, format_label);
                true
            }
            Err(err) => {
                tracing::trace!("ERR: {err}");
                false
            }
        }
    }

    pub(crate) fn set_disable_paste_burst(&mut self, disabled: bool) {
        let was_disabled = self.disable_paste_burst;
        self.disable_paste_burst = disabled;
        if disabled && !was_disabled {
            self.paste_burst.clear_window_after_non_char();
        }
    }

    /// Override the footer hint items displayed beneath the composer. Passing
    /// `None` restores the default shortcut footer.
    pub(crate) fn set_footer_hint_override(&mut self, items: Option<Vec<(String, String)>>) {
        self.footer_hint_override = items;
    }

    /// Replace the entire composer content with `text` and reset cursor.
    pub(crate) fn set_text_content(&mut self, text: String) {
        // Clear any existing content, placeholders, and attachments first.
        self.textarea.set_text("");
        self.pending_pastes.clear();
        self.attached_images.clear();
        self.textarea.set_text(&text);
        self.textarea.set_cursor(0);
        self.sync_command_popup();
        self.sync_file_search_popup();
    }

    pub(crate) fn clear_for_ctrl_c(&mut self) -> Option<String> {
        if self.is_empty() {
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
        self.textarea.text().to_string()
    }

    /// Attempt to start a burst by retro-capturing recent chars before the cursor.
    pub fn attach_image(&mut self, path: PathBuf, width: u32, height: u32, _format_label: &str) {
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

    pub(crate) fn flush_paste_burst_if_due(&mut self) -> bool {
        self.handle_paste_burst_flush(Instant::now())
    }

    pub(crate) fn is_in_paste_burst(&self) -> bool {
        self.paste_burst.is_active()
    }

    pub(crate) fn recommended_paste_flush_delay() -> Duration {
        PasteBurst::recommended_flush_delay()
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
        self.textarea.insert_str(text);
        self.sync_command_popup();
        self.sync_file_search_popup();
    }

    /// Handle a key event coming from the main UI.
    pub fn handle_key_event(&mut self, key_event: KeyEvent) -> (InputResult, bool) {
        let result = match &mut self.active_popup {
            ActivePopup::Command(_) => self.handle_key_event_with_slash_popup(key_event),
            ActivePopup::File(_) => self.handle_key_event_with_file_popup(key_event),
            ActivePopup::HistorySearch(_) => self.handle_key_event_with_history_popup(key_event),
            ActivePopup::None => self.handle_key_event_without_popup(key_event),
        };

        // The history search popup manages its own lifecycle; skip the
        // slash/file popup sync that would otherwise clobber it.
        if !matches!(self.active_popup, ActivePopup::HistorySearch(_)) {
            self.sync_command_popup();
            if matches!(self.active_popup, ActivePopup::Command(_)) {
                self.dismissed_file_popup_token = None;
            } else {
                self.sync_file_search_popup();
            }
        }

        result
    }

    /// Return true if either the slash-command popup or the file-search popup is active.
    pub(crate) fn popup_active(&self) -> bool {
        !matches!(self.active_popup, ActivePopup::None)
    }

    /// Handle key event when the slash-command popup is visible.
    fn handle_key_event_with_slash_popup(&mut self, key_event: KeyEvent) -> (InputResult, bool) {
        if self.handle_shortcut_overlay_key(&key_event) {
            return (InputResult::None, true);
        }
        if key_event.code == KeyCode::Esc {
            let next_mode = esc_hint_mode(self.footer_mode, self.is_task_running);
            if next_mode != self.footer_mode {
                self.footer_mode = next_mode;
                return (InputResult::None, true);
            }
        } else {
            self.footer_mode = reset_mode_after_activity(self.footer_mode);
        }
        let ActivePopup::Command(popup) = &mut self.active_popup else {
            unreachable!();
        };

        match key_event {
            KeyEvent {
                code: KeyCode::Up, ..
            }
            | KeyEvent {
                code: KeyCode::Char('p'),
                modifiers: KeyModifiers::CONTROL,
                ..
            } => {
                popup.move_up();
                (InputResult::None, true)
            }
            KeyEvent {
                code: KeyCode::Down,
                ..
            }
            | KeyEvent {
                code: KeyCode::Char('n'),
                modifiers: KeyModifiers::CONTROL,
                ..
            } => {
                popup.move_down();
                (InputResult::None, true)
            }
            KeyEvent {
                code: KeyCode::Esc, ..
            } => {
                // Dismiss the slash popup; keep the current input untouched.
                self.active_popup = ActivePopup::None;
                (InputResult::None, true)
            }
            KeyEvent {
                code: KeyCode::Tab, ..
            } => {
                // Ensure popup filtering/selection reflects the latest composer text
                // before applying completion.
                let first_line = self.textarea.text().lines().next().unwrap_or("");
                popup.on_composer_text_change(first_line.to_string());
                if let Some(sel) = popup.selected_item() {
                    let mut cursor_target: Option<usize> = None;
                    match sel {
                        CommandItem::Builtin(cmd) => {
                            let starts_with_cmd = first_line
                                .trim_start()
                                .starts_with(&format!("/{}", cmd.command()));
                            if !starts_with_cmd {
                                self.textarea.set_text(&format!("/{} ", cmd.command()));
                            }
                            if !self.textarea.text().is_empty() {
                                cursor_target = Some(self.textarea.text().len());
                            }
                        }
                        CommandItem::UserPrompt(idx) => {
                            if let Some(prompt) = popup.prompt(idx) {
                                match prompt_selection_action(
                                    prompt,
                                    first_line,
                                    PromptSelectionMode::Completion,
                                ) {
                                    PromptSelectionAction::Insert { text, cursor } => {
                                        let target = cursor.unwrap_or(text.len());
                                        self.textarea.set_text(&text);
                                        cursor_target = Some(target);
                                    }
                                    PromptSelectionAction::Submit { .. } => {}
                                }
                            }
                        }
                    }
                    if let Some(pos) = cursor_target {
                        self.textarea.set_cursor(pos);
                    }
                }
                (InputResult::None, true)
            }
            KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                ..
            } => {
                // If the current line starts with a custom prompt name and includes
                // positional args for a numeric-style template, expand and submit
                // immediately regardless of the popup selection.
                let first_line = self.textarea.text().lines().next().unwrap_or("");
                if let Some((name, _rest)) = parse_slash_name(first_line)
                    && let Some(prompt_name) = name.strip_prefix(&format!("{PROMPTS_CMD_PREFIX}:"))
                    && let Some(prompt) = self.custom_prompts.iter().find(|p| p.name == prompt_name)
                    && let Some(expanded) =
                        expand_if_numeric_with_positional_args(prompt, first_line)
                {
                    self.textarea.set_text("");
                    return (InputResult::Submitted(expanded), true);
                }

                if let Some(sel) = popup.selected_item() {
                    match sel {
                        CommandItem::Builtin(cmd) => {
                            self.textarea.set_text("");
                            return (InputResult::Command(cmd), true);
                        }
                        CommandItem::UserPrompt(idx) => {
                            if let Some(prompt) = popup.prompt(idx) {
                                if matches!(prompt.kind, CustomPromptKind::Script { .. }) {
                                    let args = extract_positional_args_for_prompt_line(
                                        first_line,
                                        &prompt.name,
                                    );
                                    self.app_event_tx.send(AppEvent::ExecuteScript {
                                        prompt: prompt.clone(),
                                        args,
                                    });
                                    self.textarea.set_text("");
                                    return (InputResult::None, true);
                                }
                                match prompt_selection_action(
                                    prompt,
                                    first_line,
                                    PromptSelectionMode::Submit,
                                ) {
                                    PromptSelectionAction::Submit { text } => {
                                        self.textarea.set_text("");
                                        return (InputResult::Submitted(text), true);
                                    }
                                    PromptSelectionAction::Insert { text, cursor } => {
                                        let target = cursor.unwrap_or(text.len());
                                        self.textarea.set_text(&text);
                                        self.textarea.set_cursor(target);
                                        return (InputResult::None, true);
                                    }
                                }
                            }
                            return (InputResult::None, true);
                        }
                    }
                }
                // Fallback to default newline handling if no command selected.
                self.handle_key_event_without_popup(key_event)
            }
            input => self.handle_input_basic(input),
        }
    }

    #[inline]
    fn clamp_to_char_boundary(text: &str, pos: usize) -> usize {
        let mut p = pos.min(text.len());
        if p < text.len() && !text.is_char_boundary(p) {
            p = text
                .char_indices()
                .map(|(i, _)| i)
                .take_while(|&i| i <= p)
                .last()
                .unwrap_or(0);
        }
        p
    }

    #[inline]
    fn handle_non_ascii_char(&mut self, input: KeyEvent) -> (InputResult, bool) {
        if let KeyEvent {
            code: KeyCode::Char(ch),
            ..
        } = input
        {
            let now = Instant::now();
            if self.paste_burst.try_append_char_if_active(ch, now) {
                return (InputResult::None, true);
            }
        }
        if let Some(pasted) = self.paste_burst.flush_before_modified_input() {
            self.handle_paste(pasted);
        }
        self.textarea.input(input);
        let text_after = self.textarea.text();
        self.pending_pastes
            .retain(|(placeholder, _)| text_after.contains(placeholder));
        (InputResult::None, true)
    }

    /// Handle key events when file search popup is visible.
    fn handle_key_event_with_file_popup(&mut self, key_event: KeyEvent) -> (InputResult, bool) {
        if self.handle_shortcut_overlay_key(&key_event) {
            return (InputResult::None, true);
        }
        if key_event.code == KeyCode::Esc {
            let next_mode = esc_hint_mode(self.footer_mode, self.is_task_running);
            if next_mode != self.footer_mode {
                self.footer_mode = next_mode;
                return (InputResult::None, true);
            }
        } else {
            self.footer_mode = reset_mode_after_activity(self.footer_mode);
        }
        let ActivePopup::File(popup) = &mut self.active_popup else {
            unreachable!();
        };

        match key_event {
            KeyEvent {
                code: KeyCode::Up, ..
            }
            | KeyEvent {
                code: KeyCode::Char('p'),
                modifiers: KeyModifiers::CONTROL,
                ..
            } => {
                popup.move_up();
                (InputResult::None, true)
            }
            KeyEvent {
                code: KeyCode::Down,
                ..
            }
            | KeyEvent {
                code: KeyCode::Char('n'),
                modifiers: KeyModifiers::CONTROL,
                ..
            } => {
                popup.move_down();
                (InputResult::None, true)
            }
            KeyEvent {
                code: KeyCode::Esc, ..
            } => {
                // Hide popup without modifying text, remember token to avoid immediate reopen.
                if let Some(tok) = Self::current_at_token(&self.textarea) {
                    self.dismissed_file_popup_token = Some(tok);
                }
                self.active_popup = ActivePopup::None;
                (InputResult::None, true)
            }
            KeyEvent {
                code: KeyCode::Tab, ..
            }
            | KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                ..
            } => {
                let Some(sel) = popup.selected_match() else {
                    self.active_popup = ActivePopup::None;
                    return (InputResult::None, true);
                };

                let sel_path = sel.to_string();
                // If selected path looks like an image (png/jpeg), attach as image instead of inserting text.
                let is_image = Self::is_image_path(&sel_path);
                if is_image {
                    // Determine dimensions; if that fails fall back to normal path insertion.
                    let path_buf = PathBuf::from(&sel_path);
                    if let Ok((w, h)) = image::image_dimensions(&path_buf) {
                        // Remove the current @token (mirror logic from insert_selected_path without inserting text)
                        // using the flat text and byte-offset cursor API.
                        let cursor_offset = self.textarea.cursor();
                        let text = self.textarea.text();
                        // Clamp to a valid char boundary to avoid panics when slicing.
                        let safe_cursor = Self::clamp_to_char_boundary(text, cursor_offset);
                        let before_cursor = &text[..safe_cursor];
                        let after_cursor = &text[safe_cursor..];

                        // Determine token boundaries in the full text.
                        let start_idx = before_cursor
                            .char_indices()
                            .rfind(|(_, c)| c.is_whitespace())
                            .map(|(idx, c)| idx + c.len_utf8())
                            .unwrap_or(0);
                        let end_rel_idx = after_cursor
                            .char_indices()
                            .find(|(_, c)| c.is_whitespace())
                            .map(|(idx, _)| idx)
                            .unwrap_or(after_cursor.len());
                        let end_idx = safe_cursor + end_rel_idx;

                        self.textarea.replace_range(start_idx..end_idx, "");
                        self.textarea.set_cursor(start_idx);

                        let format_label = match Path::new(&sel_path)
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(str::to_ascii_lowercase)
                        {
                            Some(ext) if ext == "png" => "PNG",
                            Some(ext) if ext == "jpg" || ext == "jpeg" => "JPEG",
                            _ => "IMG",
                        };
                        self.attach_image(path_buf, w, h, format_label);
                        // Add a trailing space to keep typing fluid.
                        self.textarea.insert_str(" ");
                    } else {
                        // Fallback to plain path insertion if metadata read fails.
                        self.insert_selected_path(&sel_path);
                    }
                } else {
                    // Non-image: inserting file path.
                    self.insert_selected_path(&sel_path);
                }
                // No selection: treat Enter as closing the popup/session.
                self.active_popup = ActivePopup::None;
                (InputResult::None, true)
            }
            input => self.handle_input_basic(input),
        }
    }

    /// Handle key events when the history search popup is visible.
    fn handle_key_event_with_history_popup(&mut self, key_event: KeyEvent) -> (InputResult, bool) {
        let ActivePopup::HistorySearch(popup) = &mut self.active_popup else {
            unreachable!();
        };

        let in_vim_normal = popup.is_vim_normal_mode();

        match key_event {
            // Esc: in vim mode, first press enters normal mode; second press closes
            KeyEvent {
                code: KeyCode::Esc, ..
            } => {
                if popup.vim_mode && !in_vim_normal {
                    popup.set_vim_normal_mode(true);
                } else {
                    self.active_popup = ActivePopup::None;
                }
                (InputResult::None, true)
            }
            // Enter: select the highlighted entry and close
            KeyEvent {
                code: KeyCode::Enter,
                ..
            } => {
                let selected = popup.selected_text().map(String::from);
                self.active_popup = ActivePopup::None;
                if let Some(text) = selected {
                    self.set_text_content(text);
                }
                (InputResult::None, true)
            }
            // Up / Ctrl+P / k (in vim normal): move selection up
            KeyEvent {
                code: KeyCode::Up, ..
            }
            | KeyEvent {
                code: KeyCode::Char('p'),
                modifiers: KeyModifiers::CONTROL,
                ..
            } => {
                popup.move_up();
                (InputResult::None, true)
            }
            KeyEvent {
                code: KeyCode::Char('k'),
                ..
            } if in_vim_normal => {
                popup.move_up();
                (InputResult::None, true)
            }
            // Down / Ctrl+N / j (in vim normal): move selection down
            KeyEvent {
                code: KeyCode::Down,
                ..
            }
            | KeyEvent {
                code: KeyCode::Char('n'),
                modifiers: KeyModifiers::CONTROL,
                ..
            } => {
                popup.move_down();
                (InputResult::None, true)
            }
            KeyEvent {
                code: KeyCode::Char('j'),
                ..
            } if in_vim_normal => {
                popup.move_down();
                (InputResult::None, true)
            }
            // i in vim normal: enter insert mode
            KeyEvent {
                code: KeyCode::Char('i'),
                ..
            } if in_vim_normal => {
                popup.set_vim_normal_mode(false);
                (InputResult::None, true)
            }
            // Backspace: remove last char from search query
            KeyEvent {
                code: KeyCode::Backspace,
                ..
            } if !in_vim_normal => {
                popup.pop_char();
                (InputResult::None, true)
            }
            // Printable character input (insert mode): append to search query
            KeyEvent {
                code: KeyCode::Char(ch),
                modifiers,
                ..
            } if !in_vim_normal && !has_ctrl_or_alt(modifiers) => {
                popup.push_char(ch);
                (InputResult::None, true)
            }
            // All other keys: consume without action
            _ => (InputResult::None, true),
        }
    }

    fn is_image_path(path: &str) -> bool {
        let lower = path.to_ascii_lowercase();
        lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg")
    }

    /// Extract the `@token` that the cursor is currently positioned on, if any.
    ///
    /// The returned string **does not** include the leading `@`.
    ///
    /// Behavior:
    /// - The cursor may be anywhere *inside* the token (including on the
    ///   leading `@`). It does **not** need to be at the end of the line.
    /// - A token is delimited by ASCII whitespace (space, tab, newline).
    /// - If the token under the cursor starts with `@`, that token is
    ///   returned without the leading `@`. This includes the case where the
    ///   token is just "@" (empty query), which is used to trigger a UI hint
    fn current_at_token(textarea: &TextArea) -> Option<String> {
        let cursor_offset = textarea.cursor();
        let text = textarea.text();

        // Adjust the provided byte offset to the nearest valid char boundary at or before it.
        let mut safe_cursor = cursor_offset.min(text.len());
        // If we're not on a char boundary, move back to the start of the current char.
        if safe_cursor < text.len() && !text.is_char_boundary(safe_cursor) {
            // Find the last valid boundary <= cursor_offset.
            safe_cursor = text
                .char_indices()
                .map(|(i, _)| i)
                .take_while(|&i| i <= cursor_offset)
                .last()
                .unwrap_or(0);
        }

        // Split the line around the (now safe) cursor position.
        let before_cursor = &text[..safe_cursor];
        let after_cursor = &text[safe_cursor..];

        // Detect whether we're on whitespace at the cursor boundary.
        let at_whitespace = if safe_cursor < text.len() {
            text[safe_cursor..]
                .chars()
                .next()
                .map(char::is_whitespace)
                .unwrap_or(false)
        } else {
            false
        };

        // Left candidate: token containing the cursor position.
        let start_left = before_cursor
            .char_indices()
            .rfind(|(_, c)| c.is_whitespace())
            .map(|(idx, c)| idx + c.len_utf8())
            .unwrap_or(0);
        let end_left_rel = after_cursor
            .char_indices()
            .find(|(_, c)| c.is_whitespace())
            .map(|(idx, _)| idx)
            .unwrap_or(after_cursor.len());
        let end_left = safe_cursor + end_left_rel;
        let token_left = if start_left < end_left {
            Some(&text[start_left..end_left])
        } else {
            None
        };

        // Right candidate: token immediately after any whitespace from the cursor.
        let ws_len_right: usize = after_cursor
            .chars()
            .take_while(|c| c.is_whitespace())
            .map(char::len_utf8)
            .sum();
        let start_right = safe_cursor + ws_len_right;
        let end_right_rel = text[start_right..]
            .char_indices()
            .find(|(_, c)| c.is_whitespace())
            .map(|(idx, _)| idx)
            .unwrap_or(text.len() - start_right);
        let end_right = start_right + end_right_rel;
        let token_right = if start_right < end_right {
            Some(&text[start_right..end_right])
        } else {
            None
        };

        let left_at = token_left
            .filter(|t| t.starts_with('@'))
            .map(|t| t[1..].to_string());
        let right_at = token_right
            .filter(|t| t.starts_with('@'))
            .map(|t| t[1..].to_string());

        if at_whitespace {
            if right_at.is_some() {
                return right_at;
            }
            if token_left.is_some_and(|t| t == "@") {
                return None;
            }
            return left_at;
        }
        if after_cursor.starts_with('@') {
            return right_at.or(left_at);
        }
        left_at.or(right_at)
    }

    /// Replace the active `@token` (the one under the cursor) with `path`.
    ///
    /// The algorithm mirrors `current_at_token` so replacement works no matter
    /// where the cursor is within the token and regardless of how many
    /// `@tokens` exist in the line.
    fn insert_selected_path(&mut self, path: &str) {
        let cursor_offset = self.textarea.cursor();
        let text = self.textarea.text();
        // Clamp to a valid char boundary to avoid panics when slicing.
        let safe_cursor = Self::clamp_to_char_boundary(text, cursor_offset);

        let before_cursor = &text[..safe_cursor];
        let after_cursor = &text[safe_cursor..];

        // Determine token boundaries.
        let start_idx = before_cursor
            .char_indices()
            .rfind(|(_, c)| c.is_whitespace())
            .map(|(idx, c)| idx + c.len_utf8())
            .unwrap_or(0);

        let end_rel_idx = after_cursor
            .char_indices()
            .find(|(_, c)| c.is_whitespace())
            .map(|(idx, _)| idx)
            .unwrap_or(after_cursor.len());
        let end_idx = safe_cursor + end_rel_idx;

        // If the path contains whitespace, wrap it in double quotes so the
        // local prompt arg parser treats it as a single argument. Avoid adding
        // quotes when the path already contains one to keep behavior simple.
        let needs_quotes = path.chars().any(char::is_whitespace);
        let inserted = if needs_quotes && !path.contains('"') {
            format!("\"{path}\"")
        } else {
            path.to_string()
        };

        // Replace the slice `[start_idx, end_idx)` with the chosen path and a trailing space.
        let mut new_text =
            String::with_capacity(text.len() - (end_idx - start_idx) + inserted.len() + 1);
        new_text.push_str(&text[..start_idx]);
        new_text.push_str(&inserted);
        new_text.push(' ');
        new_text.push_str(&text[end_idx..]);

        self.textarea.set_text(&new_text);
        let new_cursor = start_idx.saturating_add(inserted.len()).saturating_add(1);
        self.textarea.set_cursor(new_cursor);
    }

    /// Handle key event when no popup is visible.
    fn handle_key_event_without_popup(&mut self, key_event: KeyEvent) -> (InputResult, bool) {
        if self.handle_shortcut_overlay_key(&key_event) {
            return (InputResult::None, true);
        }
        if key_event.code == KeyCode::Esc {
            if self.is_empty() {
                let next_mode = esc_hint_mode(self.footer_mode, self.is_task_running);
                if next_mode != self.footer_mode {
                    self.footer_mode = next_mode;
                    return (InputResult::None, true);
                }
            }
        } else {
            self.footer_mode = reset_mode_after_activity(self.footer_mode);
        }
        match key_event {
            KeyEvent {
                code: KeyCode::Char('d'),
                modifiers: crossterm::event::KeyModifiers::CONTROL,
                kind: KeyEventKind::Press,
                ..
            } if self.is_empty() => {
                self.app_event_tx.send(AppEvent::ExitRequest);
                (InputResult::None, true)
            }
            // -------------------------------------------------------------
            // History navigation (Up / Down) – only when the composer is not
            // empty or when the cursor is at the correct position, to avoid
            // interfering with normal cursor movement.
            // -------------------------------------------------------------
            KeyEvent {
                code: KeyCode::Up | KeyCode::Down,
                ..
            }
            | KeyEvent {
                code: KeyCode::Char('p') | KeyCode::Char('n'),
                modifiers: KeyModifiers::CONTROL,
                ..
            } => {
                if self
                    .history
                    .should_handle_navigation(self.textarea.text(), self.textarea.cursor())
                {
                    let replace_text = match key_event.code {
                        KeyCode::Up => self.history.navigate_up(&self.app_event_tx),
                        KeyCode::Down => self.history.navigate_down(&self.app_event_tx),
                        KeyCode::Char('p') => self.history.navigate_up(&self.app_event_tx),
                        KeyCode::Char('n') => self.history.navigate_down(&self.app_event_tx),
                        _ => unreachable!(),
                    };
                    if let Some(text) = replace_text {
                        self.set_text_content(text);
                        return (InputResult::None, true);
                    }
                }
                self.handle_input_basic(key_event)
            }
            KeyEvent {
                code: KeyCode::Char('r'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press,
                ..
            } => {
                let vim_mode = self.textarea.vim_mode_state_if_enabled().is_some();
                self.active_popup = ActivePopup::HistorySearch(HistorySearchPopup::new(vim_mode));
                self.app_event_tx.send(AppEvent::CodexOp(
                    codex_protocol::protocol::Op::SearchHistoryRequest { max_results: 500 },
                ));
                (InputResult::None, true)
            }
            KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                ..
            } => {
                // If the first line is a bare built-in slash command (no args),
                // dispatch it even when the slash popup isn't visible. This preserves
                // the workflow: type a prefix ("/di"), press Tab to complete to
                // "/diff ", then press Enter to run it. Tab moves the cursor beyond
                // the '/name' token and our caret-based heuristic hides the popup,
                // but Enter should still dispatch the command rather than submit
                // literal text.
                let first_line = self.textarea.text().lines().next().unwrap_or("");
                if let Some((name, rest)) = parse_slash_name(first_line)
                    && rest.is_empty()
                    && let Some((_n, cmd)) = built_in_slash_commands()
                        .into_iter()
                        .find(|(n, _)| *n == name)
                {
                    self.textarea.set_text("");
                    return (InputResult::Command(cmd), true);
                }
                // If we're in a paste-like burst capture, treat Enter as part of the burst
                // and accumulate it rather than submitting or inserting immediately.
                // Do not treat Enter as paste inside a slash-command context.
                let in_slash_context = matches!(self.active_popup, ActivePopup::Command(_))
                    || self
                        .textarea
                        .text()
                        .lines()
                        .next()
                        .unwrap_or("")
                        .starts_with('/');
                if self.paste_burst.is_active() && !in_slash_context {
                    let now = Instant::now();
                    if self.paste_burst.append_newline_if_active(now) {
                        return (InputResult::None, true);
                    }
                }
                // If we have pending placeholder pastes, replace them in the textarea text
                // and continue to the normal submission flow to handle slash commands.
                if !self.pending_pastes.is_empty() {
                    let mut text = self.textarea.text().to_string();
                    for (placeholder, actual) in &self.pending_pastes {
                        if text.contains(placeholder) {
                            text = text.replace(placeholder, actual);
                        }
                    }
                    self.textarea.set_text(&text);
                    self.pending_pastes.clear();
                }

                // During a paste-like burst, treat Enter as a newline instead of submit.
                let now = Instant::now();
                if self
                    .paste_burst
                    .newline_should_insert_instead_of_submit(now)
                    && !in_slash_context
                {
                    self.textarea.insert_str("\n");
                    self.paste_burst.extend_window(now);
                    return (InputResult::None, true);
                }
                let mut text = self.textarea.text().to_string();
                let original_input = text.clone();
                let input_starts_with_space = original_input.starts_with(' ');
                self.textarea.set_text("");

                // Replace all pending pastes in the text
                for (placeholder, actual) in &self.pending_pastes {
                    if text.contains(placeholder) {
                        text = text.replace(placeholder, actual);
                    }
                }
                self.pending_pastes.clear();

                // If there is neither text nor attachments, suppress submission entirely.
                let has_attachments = !self.attached_images.is_empty();
                text = text.trim().to_string();
                if let Some((name, _rest)) = parse_slash_name(&text) {
                    let treat_as_plain_text = input_starts_with_space || name.contains('/');
                    if !treat_as_plain_text {
                        let is_builtin = built_in_slash_commands()
                            .into_iter()
                            .any(|(command_name, _)| command_name == name);
                        let prompt_prefix = format!("{PROMPTS_CMD_PREFIX}:");
                        let is_known_prompt = name
                            .strip_prefix(&prompt_prefix)
                            .map(|prompt_name| {
                                self.custom_prompts
                                    .iter()
                                    .any(|prompt| prompt.name == prompt_name)
                            })
                            .unwrap_or(false);
                        if !is_builtin && !is_known_prompt {
                            let message = format!(
                                r#"Unrecognized command '/{name}'. Type "/" for a list of supported commands."#
                            );
                            self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                                history_cell::new_info_event(message, None),
                            )));
                            self.textarea.set_text(&original_input);
                            self.textarea.set_cursor(original_input.len());
                            return (InputResult::None, true);
                        }
                    }
                }

                // Intercept script-kind prompts before attempting expand_custom_prompt,
                // since scripts have empty content and would just submit empty text.
                if let Some((name, _rest)) = parse_slash_name(&text)
                    && let Some(prompt_name) = name.strip_prefix(&format!("{PROMPTS_CMD_PREFIX}:"))
                    && let Some(prompt) = self.custom_prompts.iter().find(|p| p.name == prompt_name)
                    && matches!(prompt.kind, CustomPromptKind::Script { .. })
                {
                    let args =
                        extract_positional_args_for_prompt_line(&original_input, &prompt.name);
                    self.app_event_tx.send(AppEvent::ExecuteScript {
                        prompt: prompt.clone(),
                        args,
                    });
                    self.textarea.set_text("");
                    return (InputResult::None, true);
                }

                let expanded_prompt = match expand_custom_prompt(&text, &self.custom_prompts) {
                    Ok(expanded) => expanded,
                    Err(err) => {
                        self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                            history_cell::new_error_event(err.user_message()),
                        )));
                        self.textarea.set_text(&original_input);
                        self.textarea.set_cursor(original_input.len());
                        return (InputResult::None, true);
                    }
                };
                if let Some(expanded) = expanded_prompt {
                    text = expanded;
                }
                if text.is_empty() && !has_attachments {
                    return (InputResult::None, true);
                }
                if !text.is_empty() {
                    self.history.record_local_submission(&text);
                }
                // Do not clear attached_images here; ChatWidget drains them via take_recent_submission_images().
                (InputResult::Submitted(text), true)
            }
            input => self.handle_input_basic(input),
        }
    }

    fn handle_paste_burst_flush(&mut self, now: Instant) -> bool {
        match self.paste_burst.flush_if_due(now) {
            FlushResult::Paste(pasted) => {
                self.handle_paste(pasted);
                true
            }
            FlushResult::Typed(ch) => {
                // Mirror insert_str() behavior so popups stay in sync when a
                // pending fast char flushes as normal typed input.
                self.textarea.insert_str(ch.to_string().as_str());
                // Keep popup sync consistent with key handling: prefer slash popup; only
                // sync file popup when slash popup is NOT active.
                self.sync_command_popup();
                if matches!(self.active_popup, ActivePopup::Command(_)) {
                    self.dismissed_file_popup_token = None;
                } else {
                    self.sync_file_search_popup();
                }
                true
            }
            FlushResult::None => false,
        }
    }

    /// Handle generic Input events that modify the textarea content.
    fn handle_input_basic(&mut self, input: KeyEvent) -> (InputResult, bool) {
        // If we have a buffered non-bracketed paste burst and enough time has
        // elapsed since the last char, flush it before handling a new input.
        let now = Instant::now();
        self.handle_paste_burst_flush(now);

        if !matches!(input.code, KeyCode::Esc) {
            self.footer_mode = reset_mode_after_activity(self.footer_mode);
        }

        // In vim Normal mode, bypass paste burst detection and send input directly
        // to the textarea so vim navigation keys (h/j/k/l) work correctly.
        if self.textarea.is_in_vim_normal_mode() {
            self.textarea.input(input);
            return (InputResult::None, true);
        }

        // If we're capturing a burst and receive Enter, accumulate it instead of inserting.
        if matches!(input.code, KeyCode::Enter)
            && self.paste_burst.is_active()
            && self.paste_burst.append_newline_if_active(now)
        {
            return (InputResult::None, true);
        }

        // Intercept plain Char inputs to optionally accumulate into a burst buffer.
        if let KeyEvent {
            code: KeyCode::Char(ch),
            modifiers,
            ..
        } = input
        {
            let has_ctrl_or_alt = has_ctrl_or_alt(modifiers);
            if !has_ctrl_or_alt {
                // Non-ASCII characters (e.g., from IMEs) can arrive in quick bursts and be
                // misclassified by paste heuristics. Flush any active burst buffer and insert
                // non-ASCII characters directly.
                if !ch.is_ascii() {
                    return self.handle_non_ascii_char(input);
                }

                match self.paste_burst.on_plain_char(ch, now) {
                    CharDecision::BufferAppend => {
                        self.paste_burst.append_char_to_buffer(ch, now);
                        return (InputResult::None, true);
                    }
                    CharDecision::BeginBuffer { retro_chars } => {
                        let cur = self.textarea.cursor();
                        let txt = self.textarea.text();
                        let safe_cur = Self::clamp_to_char_boundary(txt, cur);
                        let before = &txt[..safe_cur];
                        if let Some(grab) =
                            self.paste_burst
                                .decide_begin_buffer(now, before, retro_chars as usize)
                        {
                            if !grab.grabbed.is_empty() {
                                self.textarea.replace_range(grab.start_byte..safe_cur, "");
                            }
                            self.paste_burst.begin_with_retro_grabbed(grab.grabbed, now);
                            self.paste_burst.append_char_to_buffer(ch, now);
                            return (InputResult::None, true);
                        }
                        // If decide_begin_buffer opted not to start buffering,
                        // fall through to normal insertion below.
                    }
                    CharDecision::BeginBufferFromPending => {
                        // First char was held; now append the current one.
                        self.paste_burst.append_char_to_buffer(ch, now);
                        return (InputResult::None, true);
                    }
                    CharDecision::RetainFirstChar => {
                        // Keep the first fast char pending momentarily.
                        return (InputResult::None, true);
                    }
                }
            }
            if let Some(pasted) = self.paste_burst.flush_before_modified_input() {
                self.handle_paste(pasted);
            }
        }

        // For non-char inputs (or after flushing), handle normally.
        // Special handling for backspace on placeholders
        if let KeyEvent {
            code: KeyCode::Backspace,
            ..
        } = input
            && self.try_remove_any_placeholder_at_cursor()
        {
            return (InputResult::None, true);
        }

        // Normal input handling
        self.textarea.input(input);
        let text_after = self.textarea.text();

        // Update paste-burst heuristic for plain Char (no Ctrl/Alt) events.
        let crossterm::event::KeyEvent {
            code, modifiers, ..
        } = input;
        match code {
            KeyCode::Char(_) => {
                let has_ctrl_or_alt = has_ctrl_or_alt(modifiers);
                if has_ctrl_or_alt {
                    self.paste_burst.clear_window_after_non_char();
                }
            }
            KeyCode::Enter => {
                // Keep burst window alive (supports blank lines in paste).
            }
            _ => {
                // Other keys: clear burst window (buffer should have been flushed above if needed).
                self.paste_burst.clear_window_after_non_char();
            }
        }

        // Check if any placeholders were removed and remove their corresponding pending pastes
        self.pending_pastes
            .retain(|(placeholder, _)| text_after.contains(placeholder));

        // Keep attached images in proportion to how many matching placeholders exist in the text.
        // This handles duplicate placeholders that share the same visible label.
        if !self.attached_images.is_empty() {
            let mut needed: HashMap<String, usize> = HashMap::new();
            for img in &self.attached_images {
                needed
                    .entry(img.placeholder.clone())
                    .or_insert_with(|| text_after.matches(&img.placeholder).count());
            }

            let mut used: HashMap<String, usize> = HashMap::new();
            let mut kept: Vec<AttachedImage> = Vec::with_capacity(self.attached_images.len());
            for img in self.attached_images.drain(..) {
                let total_needed = *needed.get(&img.placeholder).unwrap_or(&0);
                let used_count = used.entry(img.placeholder.clone()).or_insert(0);
                if *used_count < total_needed {
                    kept.push(img);
                    *used_count += 1;
                }
            }
            self.attached_images = kept;
        }

        (InputResult::None, true)
    }

    /// Attempts to remove an image or paste placeholder if the cursor is at the end of one.
    /// Returns true if a placeholder was removed.
    fn try_remove_any_placeholder_at_cursor(&mut self) -> bool {
        // Clamp the cursor to a valid char boundary to avoid panics when slicing.
        let text = self.textarea.text();
        let p = Self::clamp_to_char_boundary(text, self.textarea.cursor());

        // Try image placeholders first
        let mut out: Option<(usize, String)> = None;
        // Detect if the cursor is at the end of any image placeholder.
        // If duplicates exist, remove the specific occurrence's mapping.
        for (i, img) in self.attached_images.iter().enumerate() {
            let ph = &img.placeholder;
            if p < ph.len() {
                continue;
            }
            let start = p - ph.len();
            if text.get(start..p) != Some(ph.as_str()) {
                continue;
            }

            // Count the number of occurrences of `ph` before `start`.
            let mut occ_before = 0usize;
            let mut search_pos = 0usize;
            while search_pos < start {
                let segment = match text.get(search_pos..start) {
                    Some(s) => s,
                    None => break,
                };
                if let Some(found) = segment.find(ph) {
                    occ_before += 1;
                    search_pos += found + ph.len();
                } else {
                    break;
                }
            }

            // Remove the occ_before-th attached image that shares this placeholder label.
            out = if let Some((remove_idx, _)) = self
                .attached_images
                .iter()
                .enumerate()
                .filter(|(_, img2)| img2.placeholder == *ph)
                .nth(occ_before)
            {
                Some((remove_idx, ph.clone()))
            } else {
                Some((i, ph.clone()))
            };
            break;
        }
        if let Some((idx, placeholder)) = out {
            self.textarea.replace_range(p - placeholder.len()..p, "");
            self.attached_images.remove(idx);
            return true;
        }

        // Also handle when the cursor is at the START of an image placeholder.
        // let result = 'out: {
        let out: Option<(usize, String)> = 'out: {
            for (i, img) in self.attached_images.iter().enumerate() {
                let ph = &img.placeholder;
                if p + ph.len() > text.len() {
                    continue;
                }
                if text.get(p..p + ph.len()) != Some(ph.as_str()) {
                    continue;
                }

                // Count occurrences of `ph` before `p`.
                let mut occ_before = 0usize;
                let mut search_pos = 0usize;
                while search_pos < p {
                    let segment = match text.get(search_pos..p) {
                        Some(s) => s,
                        None => break 'out None,
                    };
                    if let Some(found) = segment.find(ph) {
                        occ_before += 1;
                        search_pos += found + ph.len();
                    } else {
                        break 'out None;
                    }
                }

                if let Some((remove_idx, _)) = self
                    .attached_images
                    .iter()
                    .enumerate()
                    .filter(|(_, img2)| img2.placeholder == *ph)
                    .nth(occ_before)
                {
                    break 'out Some((remove_idx, ph.clone()));
                } else {
                    break 'out Some((i, ph.clone()));
                }
            }
            None
        };

        if let Some((idx, placeholder)) = out {
            self.textarea.replace_range(p..p + placeholder.len(), "");
            self.attached_images.remove(idx);
            return true;
        }

        // Then try pasted-content placeholders
        if let Some(placeholder) = self.pending_pastes.iter().find_map(|(ph, _)| {
            if p < ph.len() {
                return None;
            }
            let start = p - ph.len();
            if text.get(start..p) == Some(ph.as_str()) {
                Some(ph.clone())
            } else {
                None
            }
        }) {
            self.textarea.replace_range(p - placeholder.len()..p, "");
            self.pending_pastes.retain(|(ph, _)| ph != &placeholder);
            return true;
        }

        // Also handle when the cursor is at the START of a pasted-content placeholder.
        if let Some(placeholder) = self.pending_pastes.iter().find_map(|(ph, _)| {
            if p + ph.len() > text.len() {
                return None;
            }
            if text.get(p..p + ph.len()) == Some(ph.as_str()) {
                Some(ph.clone())
            } else {
                None
            }
        }) {
            self.textarea.replace_range(p..p + placeholder.len(), "");
            self.pending_pastes.retain(|(ph, _)| ph != &placeholder);
            return true;
        }

        false
    }

    fn handle_shortcut_overlay_key(&mut self, key_event: &KeyEvent) -> bool {
        if key_event.kind != KeyEventKind::Press {
            return false;
        }

        let toggles = matches!(
            key_event,
            KeyEvent {
                code: KeyCode::Char('?'),
                modifiers: KeyModifiers::NONE,
                ..
            } if self.is_empty()
        );

        if !toggles {
            return false;
        }

        let next = toggle_shortcut_mode(self.footer_mode, self.ctrl_c_quit_hint);
        let changed = next != self.footer_mode;
        self.footer_mode = next;
        changed
    }

    fn footer_props(&self) -> FooterProps {
        let (
            git_branch,
            nori_profile,
            nori_version,
            nori_version_source,
            git_lines_added,
            git_lines_removed,
        ) = if let Some(ref info) = self.system_info {
            (
                info.git_branch.clone(),
                info.nori_profile.clone(),
                info.nori_version.clone(),
                info.nori_version_source,
                info.git_lines_added,
                info.git_lines_removed,
            )
        } else {
            (None, None, None, None, None, None)
        };

        // Extract token breakdown and agent kind from transcript location
        let transcript_location = self
            .system_info
            .as_ref()
            .and_then(|s| s.transcript_location.as_ref());
        let token_breakdown = transcript_location.and_then(|loc| loc.token_breakdown.as_ref());
        let context_window_size =
            transcript_location.map(|loc| loc.agent_kind.context_window_size());
        let context_tokens = token_breakdown.map(codex_acp::TranscriptTokenUsage::total);
        let context_window_percent = self.context_window_percent.or_else(|| {
            context_window_size.and_then(|window_size| {
                context_tokens.map(|tokens| {
                    if window_size > 0 {
                        ((tokens as f64 / window_size as f64) * 100.0).round() as i64
                    } else {
                        0
                    }
                })
            })
        });

        FooterProps {
            mode: self.footer_mode(),
            esc_backtrack_hint: self.esc_backtrack_hint,
            use_shift_enter_hint: self.use_shift_enter_hint,
            is_task_running: self.is_task_running,
            vertical_footer: self.vertical_footer,
            context_window_percent,
            context_tokens,
            git_branch,
            approval_mode_label: self.approval_mode_label.clone(),
            nori_profile,
            nori_version,
            nori_version_source,
            git_lines_added,
            git_lines_removed,
            is_worktree: self
                .system_info
                .as_ref()
                .map(|s| s.is_worktree)
                .unwrap_or(false),
            input_tokens: token_breakdown.map(|t| t.input_tokens),
            output_tokens: token_breakdown.map(|t| t.output_tokens),
            cached_tokens: token_breakdown.map(|t| t.cached_tokens),
            vim_mode_state: self.textarea.vim_mode_state_if_enabled(),
            prompt_summary: self.prompt_summary.clone(),
            worktree_name: self
                .system_info
                .as_ref()
                .and_then(|s| s.worktree_name.clone()),
            footer_segment_config: self.footer_segment_config.clone(),
        }
    }

    fn footer_mode(&self) -> FooterMode {
        match self.footer_mode {
            FooterMode::EscHint => FooterMode::EscHint,
            FooterMode::ShortcutOverlay => FooterMode::ShortcutOverlay,
            FooterMode::CtrlCReminder => FooterMode::CtrlCReminder,
            FooterMode::ShortcutSummary if self.ctrl_c_quit_hint => FooterMode::CtrlCReminder,
            FooterMode::ShortcutSummary if !self.is_empty() => FooterMode::ContextOnly,
            other => other,
        }
    }

    fn custom_footer_height(&self) -> Option<u16> {
        self.footer_hint_override
            .as_ref()
            .map(|items| if items.is_empty() { 0 } else { 1 })
    }

    /// Synchronize `self.command_popup` with the current text in the
    /// textarea. This must be called after every modification that can change
    /// the text so the popup is shown/updated/hidden as appropriate.
    fn sync_command_popup(&mut self) {
        // Determine whether the caret is inside the initial '/name' token on the first line.
        let text = self.textarea.text();
        let first_line_end = text.find('\n').unwrap_or(text.len());
        let first_line = &text[..first_line_end];
        let cursor = self.textarea.cursor();
        let caret_on_first_line = cursor <= first_line_end;

        let is_editing_slash_command_name = if first_line.starts_with('/') && caret_on_first_line {
            // Compute the end of the initial '/name' token (name may be empty yet).
            let token_end = first_line
                .char_indices()
                .find(|(_, c)| c.is_whitespace())
                .map(|(i, _)| i)
                .unwrap_or(first_line.len());
            cursor <= token_end
        } else {
            false
        };
        // If the cursor is currently positioned within an `@token`, prefer the
        // file-search popup over the slash popup so users can insert a file path
        // as an argument to the command (e.g., "/compact @docs/...").
        if Self::current_at_token(&self.textarea).is_some() {
            if matches!(self.active_popup, ActivePopup::Command(_)) {
                self.active_popup = ActivePopup::None;
            }
            return;
        }
        match &mut self.active_popup {
            ActivePopup::Command(popup) => {
                if is_editing_slash_command_name {
                    popup.on_composer_text_change(first_line.to_string());
                } else {
                    self.active_popup = ActivePopup::None;
                }
            }
            _ => {
                if is_editing_slash_command_name {
                    let mut command_popup = CommandPopup::new(self.custom_prompts.clone());
                    command_popup.on_composer_text_change(first_line.to_string());
                    self.active_popup = ActivePopup::Command(command_popup);
                }
            }
        }
    }

    pub(crate) fn set_custom_prompts(&mut self, prompts: Vec<CustomPrompt>) {
        self.custom_prompts = prompts.clone();
        if let ActivePopup::Command(popup) = &mut self.active_popup {
            popup.set_prompts(prompts);
        }
    }

    /// Synchronize `self.file_search_popup` with the current text in the textarea.
    /// Note this is only called when self.active_popup is NOT Command.
    fn sync_file_search_popup(&mut self) {
        // Determine if there is an @token underneath the cursor.
        let query = match Self::current_at_token(&self.textarea) {
            Some(token) => token,
            None => {
                self.active_popup = ActivePopup::None;
                self.dismissed_file_popup_token = None;
                return;
            }
        };

        // If user dismissed popup for this exact query, don't reopen until text changes.
        if self.dismissed_file_popup_token.as_ref() == Some(&query) {
            return;
        }

        if !query.is_empty() {
            self.app_event_tx
                .send(AppEvent::StartFileSearch(query.clone()));
        }

        match &mut self.active_popup {
            ActivePopup::File(popup) => {
                if query.is_empty() {
                    popup.set_empty_prompt();
                } else {
                    popup.set_query(&query);
                }
            }
            _ => {
                let mut popup = FileSearchPopup::new();
                if query.is_empty() {
                    popup.set_empty_prompt();
                } else {
                    popup.set_query(&query);
                }
                self.active_popup = ActivePopup::File(popup);
            }
        }

        self.current_file_query = Some(query);
        self.dismissed_file_popup_token = None;
    }

    fn set_has_focus(&mut self, has_focus: bool) {
        self.has_focus = has_focus;
    }

    pub fn set_task_running(&mut self, running: bool) {
        self.is_task_running = running;
    }

    pub(crate) fn set_context_window_percent(&mut self, percent: Option<i64>) {
        if self.context_window_percent != percent {
            self.context_window_percent = percent;
        }
    }

    pub(crate) fn set_system_info(&mut self, info: crate::system_info::SystemInfo) {
        self.system_info = Some(info);
    }

    pub(crate) fn set_approval_mode_label(&mut self, label: Option<String>) {
        self.approval_mode_label = label;
    }

    pub(crate) fn set_prompt_summary(&mut self, summary: Option<String>) {
        self.prompt_summary = summary;
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
    pub(crate) fn transcript_token_breakdown(&self) -> Option<codex_acp::TranscriptTokenUsage> {
        self.system_info
            .as_ref()
            .and_then(|s| s.transcript_location.as_ref())
            .and_then(|loc| loc.token_breakdown.clone())
    }
}

impl Renderable for ChatComposer {
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let [_, textarea_rect, _] = self.layout_areas(area);
        let state = *self.textarea_state.borrow();
        self.textarea.cursor_pos_with_state(textarea_rect, state)
    }

    fn desired_height(&self, width: u16) -> u16 {
        let footer_props = self.footer_props();
        let footer_hint_height = self
            .custom_footer_height()
            .unwrap_or_else(|| footer_height(&footer_props));
        let footer_spacing = Self::footer_spacing(footer_hint_height);
        let footer_total_height = footer_hint_height + footer_spacing;
        const COLS_WITH_MARGIN: u16 = LIVE_PREFIX_COLS + 1;
        self.textarea
            .desired_height(width.saturating_sub(COLS_WITH_MARGIN))
            + 2
            + match &self.active_popup {
                ActivePopup::None => footer_total_height,
                ActivePopup::Command(c) => c.calculate_required_height(width),
                ActivePopup::File(c) => c.calculate_required_height(),
                ActivePopup::HistorySearch(c) => c.calculate_required_height(),
            }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let [composer_rect, textarea_rect, popup_rect] = self.layout_areas(area);
        match &self.active_popup {
            ActivePopup::Command(popup) => {
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
                        Line::from(spans).render_ref(custom_rect, buf);
                    }
                } else {
                    render_footer(hint_rect, buf, &footer_props);
                }
            }
        }
        let style = user_message_style();
        Block::default().style(style).render_ref(composer_rect, buf);
        if !textarea_rect.is_empty() {
            buf.set_span(
                textarea_rect.x - LIVE_PREFIX_COLS,
                textarea_rect.y,
                &"›".bold(),
                textarea_rect.width,
            );
        }

        let mut state = self.textarea_state.borrow_mut();
        StatefulWidgetRef::render_ref(&(&self.textarea), textarea_rect, buf, &mut state);
        if self.textarea.text().is_empty() {
            let placeholder = Span::from(self.placeholder_text.as_str()).dim();
            Line::from(vec![placeholder]).render_ref(textarea_rect.inner(Margin::new(0, 0)), buf);
        }
    }
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
