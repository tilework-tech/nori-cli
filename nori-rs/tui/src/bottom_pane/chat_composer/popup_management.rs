use super::*;

pub(super) struct DollarToken {
    pub(super) query: String,
    pub(super) range: std::ops::Range<usize>,
    pub(super) slash_command_position: bool,
}

impl ChatComposer {
    /// Return true if a composer popup is active.
    pub(crate) fn popup_active(&self) -> bool {
        !matches!(self.active_popup, ActivePopup::None)
    }

    pub(super) fn is_image_path(path: &str) -> bool {
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
    pub(super) fn current_at_token(textarea: &TextArea) -> Option<String> {
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
    pub(super) fn insert_selected_path(&mut self, path: &str) {
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

    pub(super) fn current_dollar_token(textarea: &TextArea) -> Option<DollarToken> {
        let cursor_offset = textarea.cursor();
        let text = textarea.text();
        let safe_cursor = Self::clamp_to_char_boundary(text, cursor_offset);

        let before_cursor = &text[..safe_cursor];
        let after_cursor = &text[safe_cursor..];
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

        if start_idx >= end_idx {
            return None;
        }

        let token = &text[start_idx..end_idx];
        let query = token.strip_prefix('$')?.to_string();
        let slash_command_position = start_idx == 0;

        Some(DollarToken {
            query,
            range: start_idx..end_idx,
            slash_command_position,
        })
    }

    pub(super) fn insert_selected_skill(&mut self, insert_text: &str) {
        let Some(token) = Self::current_dollar_token(&self.textarea) else {
            return;
        };

        let cursor = token.range.start + insert_text.len();
        self.textarea.replace_range(token.range, insert_text);
        self.textarea.set_cursor(cursor);
    }

    pub(super) fn sync_selection_popups(&mut self) {
        if matches!(self.active_popup, ActivePopup::HistorySearch(_)) {
            return;
        }

        if self.is_shell_mode {
            self.active_popup = ActivePopup::None;
            return;
        }

        // Slash commands remain navigable in Vim Normal mode, but token-based
        // file and skill lookup is intentionally Insert-only. This keeps `$`
        // available as Vim's end-of-line motion and prevents Normal-mode edits
        // from mutating picker queries behind the popup.
        if self.textarea.is_in_vim_normal_mode() {
            if matches!(
                self.active_popup,
                ActivePopup::File(_) | ActivePopup::Skill(_)
            ) {
                self.active_popup = ActivePopup::None;
            }
            self.sync_command_popup();
            return;
        }

        if Self::current_at_token(&self.textarea).is_some() {
            self.sync_file_search_popup();
            return;
        }

        self.sync_skill_popup();
        if matches!(self.active_popup, ActivePopup::Skill(_)) {
            self.dismissed_file_popup_token = None;
            return;
        }

        self.sync_command_popup();
        if matches!(self.active_popup, ActivePopup::Command(_)) {
            self.dismissed_file_popup_token = None;
        } else {
            self.sync_file_search_popup();
        }
    }

    pub(super) fn sync_skill_popup(&mut self) {
        let Some(token) = Self::current_dollar_token(&self.textarea) else {
            if matches!(self.active_popup, ActivePopup::Skill(_)) {
                self.active_popup = ActivePopup::None;
            }
            self.dismissed_skill_popup_token = None;
            return;
        };

        if self.dismissed_skill_popup_token.as_ref() == Some(&token.query) {
            return;
        }

        let query = token.query.clone();
        let items = self.skill_picker_items(&token);
        if items.is_empty() {
            if matches!(self.active_popup, ActivePopup::Skill(_)) {
                self.active_popup = ActivePopup::None;
            }
            return;
        }

        match &mut self.active_popup {
            ActivePopup::Skill(popup) => {
                popup.set_items(items);
                popup.on_query_change(query);
                if !popup.has_matches() {
                    self.active_popup = ActivePopup::None;
                }
            }
            _ => {
                let mut popup = SkillPopup::new(items);
                popup.on_query_change(query);
                if popup.has_matches() {
                    self.active_popup = ActivePopup::Skill(popup);
                }
            }
        }

        self.dismissed_skill_popup_token = None;
    }

    fn skill_picker_items(&self, token: &DollarToken) -> Vec<SkillPickerItem> {
        let builtin_names: std::collections::HashSet<&str> = built_in_slash_commands()
            .iter()
            .map(|(name, _)| *name)
            .collect();
        let mut items = Vec::new();

        for command in &self.agent_commands {
            if let Some(display_name) = command.name.strip_prefix('$') {
                items.push(SkillPickerItem {
                    display_name: display_name.to_string(),
                    insert_text: command.name.clone(),
                    description: command.description.clone(),
                });
            }
        }

        if token.slash_command_position
            && matches!(self.agent_command_prefix.as_str(), "claude" | "claude-code")
        {
            for command in &self.agent_commands {
                if command.name.starts_with('$') || builtin_names.contains(command.name.as_str()) {
                    continue;
                }
                items.push(SkillPickerItem {
                    display_name: command.name.clone(),
                    insert_text: {
                        let prefix = &self.agent_command_prefix;
                        format!("/{prefix}:{} ", command.name)
                    },
                    description: command.description.clone(),
                });
            }
        }

        items
    }

    /// Synchronize `self.command_popup` with the current text in the
    /// textarea. This must be called after every modification that can change
    /// the text so the popup is shown/updated/hidden as appropriate.
    pub(super) fn sync_command_popup(&mut self) {
        let text = self.textarea.text();
        let first_line_end = text.find('\n').unwrap_or(text.len());
        let first_line = &text[..first_line_end];
        let is_editing_slash_command_name = self.is_editing_slash_command_name();
        if self.dismissed_command_popup_text.as_deref() == Some(first_line) {
            if matches!(self.active_popup, ActivePopup::Command(_)) {
                self.active_popup = ActivePopup::None;
            }
            return;
        }
        self.dismissed_command_popup_text = None;
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
                    let mut command_popup = CommandPopup::new_full(
                        self.custom_prompts.clone(),
                        self.agent_commands.clone(),
                        self.agent_command_prefix.clone(),
                        self.command_description_overrides.clone(),
                        self.disabled_builtin_commands.clone(),
                    );
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

    pub(crate) fn set_agent_commands(
        &mut self,
        commands: Vec<crate::presentation::AgentCommandInfo>,
        prefix: String,
    ) {
        self.agent_commands = commands.clone();
        self.agent_command_prefix = prefix.clone();
        if let ActivePopup::Command(popup) = &mut self.active_popup {
            popup.set_agent_commands(commands, prefix);
        } else if matches!(self.active_popup, ActivePopup::Skill(_)) {
            self.sync_skill_popup();
        }
    }

    #[cfg(test)]
    pub(crate) fn update_agent_command_prefix(&mut self, prefix: String) {
        self.agent_command_prefix = prefix.clone();
        if let ActivePopup::Command(popup) = &mut self.active_popup {
            popup.set_agent_commands(self.agent_commands.clone(), prefix);
        } else if matches!(self.active_popup, ActivePopup::Skill(_)) {
            self.sync_skill_popup();
        }
    }

    /// Return the filtered command items from the active popup, if any.
    #[cfg(test)]
    pub(crate) fn command_popup_items(
        &self,
    ) -> Vec<crate::bottom_pane::command_popup::CommandItem> {
        if let ActivePopup::Command(popup) = &self.active_popup {
            popup.filtered_items()
        } else {
            Vec::new()
        }
    }

    /// Return the name of an agent command by index, if available.
    #[cfg(test)]
    pub(crate) fn agent_command_name(&self, idx: usize) -> Option<String> {
        if let ActivePopup::Command(popup) = &self.active_popup {
            popup.agent_command(idx).map(|c| c.name.clone())
        } else {
            None
        }
    }

    /// Synchronize `self.file_search_popup` with the current text in the textarea.
    /// Note this is only called when self.active_popup is NOT Command.
    pub(super) fn sync_file_search_popup(&mut self) {
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
}
