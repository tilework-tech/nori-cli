use super::*;
use crate::bottom_pane::textarea::VimModeState;
use codex_acp::config::VimEnterBehavior;

impl ChatComposer {
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

    /// Handle key event when the slash-command popup is visible.
    pub(super) fn handle_key_event_with_slash_popup(
        &mut self,
        key_event: KeyEvent,
    ) -> (InputResult, bool) {
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
                        CommandItem::AgentCommand(idx) => {
                            if let Some(cmd) = popup.agent_command(idx) {
                                let display_name = if self.agent_command_prefix.is_empty() {
                                    cmd.name.clone()
                                } else {
                                    format!("{}:{}", self.agent_command_prefix, cmd.name)
                                };
                                self.textarea.set_text(&format!("/{display_name} "));
                                cursor_target = Some(self.textarea.text().len());
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
                        CommandItem::AgentCommand(idx) => {
                            if let Some(cmd) = popup.agent_command(idx) {
                                let text = format!("/{}", cmd.name);
                                self.textarea.set_text("");
                                return (InputResult::Submitted(text), true);
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
    pub(super) fn clamp_to_char_boundary(text: &str, pos: usize) -> usize {
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
    pub(super) fn handle_non_ascii_char(&mut self, input: KeyEvent) -> (InputResult, bool) {
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
    pub(super) fn handle_key_event_with_file_popup(
        &mut self,
        key_event: KeyEvent,
    ) -> (InputResult, bool) {
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
    pub(super) fn handle_key_event_with_history_popup(
        &mut self,
        key_event: KeyEvent,
    ) -> (InputResult, bool) {
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

    /// Handle key event when no popup is visible.
    pub(super) fn handle_key_event_without_popup(
        &mut self,
        key_event: KeyEvent,
    ) -> (InputResult, bool) {
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
            key_event
                if key_event.kind == KeyEventKind::Press
                    && self.textarea.vim_mode_state_if_enabled() != Some(VimModeState::Normal)
                    && matches_binding(
                        self.textarea
                            .hotkey_config()
                            .binding_for(codex_acp::config::HotkeyAction::HistorySearch),
                        &key_event,
                    ) =>
            {
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
                // Vim enter behavior: route based on mode and configured behavior.
                if let Some(vim_state) = self.textarea.vim_mode_state_if_enabled() {
                    match (self.vim_enter_behavior, vim_state) {
                        // "Enter is Newline": INSERT mode Enter inserts a newline.
                        (VimEnterBehavior::Newline, VimModeState::Insert) => {
                            return self.handle_input_basic(key_event);
                        }
                        // "Enter is Submit": NORMAL mode Enter inserts a newline.
                        (VimEnterBehavior::Submit, VimModeState::Normal) => {
                            self.textarea.insert_str("\n");
                            return (InputResult::None, true);
                        }
                        _ => {} // fall through to submit
                    }
                }

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
                        let agent_prefix = if self.agent_command_prefix.is_empty() {
                            String::new()
                        } else {
                            format!("{}:", self.agent_command_prefix)
                        };
                        let is_agent_command = !agent_prefix.is_empty()
                            && name.strip_prefix(&agent_prefix).is_some_and(|cmd_name| {
                                self.agent_commands.iter().any(|cmd| cmd.name == cmd_name)
                            });
                        if !is_builtin && !is_known_prompt && !is_agent_command {
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

                // Strip the agent command prefix so the ACP agent receives the
                // bare command name (e.g. "/loop 5m" instead of "/claude-code:loop 5m").
                // Save the pre-stripped text for history so recall + resubmit works
                // (the prefixed form passes validation and gets stripped again).
                let history_text = text.clone();
                if !self.agent_command_prefix.is_empty() {
                    let slash_prefix = format!("/{}:", self.agent_command_prefix);
                    if let Some(rest) = text.strip_prefix(&slash_prefix) {
                        text = format!("/{rest}");
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
                    self.history.record_local_submission(&history_text);
                }
                // Do not clear attached_images here; ChatWidget drains them via take_recent_submission_images().
                (InputResult::Submitted(text), true)
            }
            input => self.handle_input_basic(input),
        }
    }

    /// Handle generic Input events that modify the textarea content.
    pub(super) fn handle_input_basic(&mut self, input: KeyEvent) -> (InputResult, bool) {
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
    pub(super) fn try_remove_any_placeholder_at_cursor(&mut self) -> bool {
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

    pub(super) fn handle_shortcut_overlay_key(&mut self, key_event: &KeyEvent) -> bool {
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
}
