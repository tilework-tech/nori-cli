use super::*;

fn sanitize_pasted_text(text: &str) -> String {
    let mut sanitized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.next_if_eq(&'[').is_some() {
            let _ = chars.find(|ch| ('@'..='~').contains(ch));
        } else if matches!(ch, '\n' | '\t') || !ch.is_control() {
            sanitized.push(ch);
        }
    }
    sanitized
}

impl ChatComposer {
    pub fn handle_paste(&mut self, pasted: String) -> bool {
        if !self.input_enabled {
            return false;
        }
        let pasted = pasted.replace("\r\n", "\n").replace('\r', "\n");
        let pasted = sanitize_pasted_text(&pasted);
        self.handle_paste_with_shell_detection(pasted, true)
    }

    pub(super) fn handle_paste_with_shell_detection(
        &mut self,
        mut pasted: String,
        detect_shell_mode: bool,
    ) -> bool {
        if !self.input_enabled {
            return false;
        }
        if detect_shell_mode
            && !self.is_shell_mode
            && self.textarea.text().is_empty()
            && self.textarea.cursor() == 0
            && let Some(shell_body) = pasted.strip_prefix('!')
        {
            self.is_shell_mode = true;
            pasted = shell_body.to_string();
        }

        let char_count = pasted.chars().count();
        if char_count > LARGE_PASTE_CHAR_THRESHOLD {
            let placeholder = self.next_large_paste_placeholder(char_count);
            self.textarea.insert_element(&placeholder);
            self.pending_pastes.push((placeholder, pasted));
        } else if char_count > 1 && self.handle_paste_image_path(pasted.clone()) {
            self.textarea.insert_str(" ");
        } else {
            self.textarea.insert_str(&pasted);
        }
        // Explicit paste events should not trigger Enter suppression.
        self.paste_burst.clear_after_explicit_paste();
        self.sync_selection_popups();
        true
    }

    fn next_large_paste_placeholder(&self, char_count: usize) -> String {
        let base = format!("[Pasted Content {char_count} chars]");
        let prefix = format!("{base} #");
        let mut max_suffix = 0;

        for (placeholder, _) in &self.pending_pastes {
            if placeholder == &base {
                max_suffix = max_suffix.max(1);
            } else if let Some(suffix) = placeholder.strip_prefix(&prefix)
                && let Ok(value) = suffix.parse::<usize>()
            {
                max_suffix = max_suffix.max(value);
            }
        }

        if max_suffix == 0 {
            base
        } else {
            format!("{base} #{}", max_suffix + 1)
        }
    }

    pub fn handle_paste_image_path(&mut self, pasted: String) -> bool {
        if !self.input_enabled {
            return false;
        }
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

    pub(crate) fn flush_paste_burst_if_due(&mut self) -> bool {
        if !self.input_enabled {
            return false;
        }
        self.handle_paste_burst_flush(Instant::now())
    }

    pub(crate) fn is_in_paste_burst(&self) -> bool {
        self.paste_burst.is_active()
    }

    pub(crate) fn recommended_paste_flush_delay() -> Duration {
        PasteBurst::recommended_flush_delay()
    }

    pub(super) fn handle_paste_burst_flush(&mut self, now: Instant) -> bool {
        if !self.input_enabled {
            return false;
        }
        match self.paste_burst.flush_if_due(now) {
            FlushResult::Paste(pasted) => {
                self.handle_paste_with_shell_detection(pasted, false);
                true
            }
            FlushResult::Typed(ch) => {
                // Mirror insert_str() behavior so popups stay in sync when a
                // pending fast char flushes as normal typed input.
                if ch == '!'
                    && !self.is_shell_mode
                    && self.textarea.text().is_empty()
                    && self.textarea.cursor() == 0
                {
                    self.is_shell_mode = true;
                } else {
                    self.textarea.insert_str(ch.to_string().as_str());
                }
                self.sync_selection_popups();
                true
            }
            FlushResult::None => false,
        }
    }
}
