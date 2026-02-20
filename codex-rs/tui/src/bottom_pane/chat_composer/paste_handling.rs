use super::*;

impl ChatComposer {
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

    pub(crate) fn flush_paste_burst_if_due(&mut self) -> bool {
        self.handle_paste_burst_flush(Instant::now())
    }

    pub(crate) fn is_in_paste_burst(&self) -> bool {
        self.paste_burst.is_active()
    }

    pub(crate) fn recommended_paste_flush_delay() -> Duration {
        PasteBurst::recommended_flush_delay()
    }

    pub(super) fn handle_paste_burst_flush(&mut self, now: Instant) -> bool {
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
}
