use super::KillBufferKind;
use super::TextArea;
use std::ops::Range;

impl TextArea {
    pub(in crate::bottom_pane::textarea) fn yank_current_line(&mut self) {
        let range = self.current_line_range_with_newline();
        self.yank_range_with_kind(range, KillBufferKind::Linewise);
    }

    pub(super) fn kill_current_line(&mut self) {
        let bol = self.beginning_of_current_line();
        let eol = self.end_of_current_line();
        if eol < self.text.len() {
            self.kill_range_with_kind(bol..eol + 1, KillBufferKind::Linewise);
        } else if bol > 0 {
            let removed = self.text[bol..eol].to_string();
            if !removed.is_empty() {
                self.kill_buffer = removed;
                self.kill_buffer_kind = KillBufferKind::Linewise;
            }
            self.replace_range_raw(bol - 1..eol, "");
        } else {
            self.kill_range_with_kind(0..eol, KillBufferKind::Linewise);
        }
    }

    pub(super) fn current_line_range_with_newline(&self) -> Range<usize> {
        let bol = self.beginning_of_current_line();
        let eol = self.end_of_current_line();
        let end = if eol < self.text.len() { eol + 1 } else { eol };
        bol..end
    }
}
