mod events;
mod lines;

use self::events::motion_for_event;
use self::events::plain_char;
use self::events::text_object_for_event;
use self::events::text_object_scope_for_event;
use super::TextArea;
use super::split_word_pieces;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KillBufferKind {
    Characterwise,
    Linewise,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VimOperator {
    Delete,
    Yank,
    Change,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VimPending {
    None,
    Goto,
    Operator(VimOperator),
    TextObject {
        operator: VimOperator,
        scope: VimTextObjectScope,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VimMotion {
    Left,
    Right,
    Up,
    Down,
    WordForward,
    WordBackward,
    WordEnd,
    LineStart,
    LineEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VimTextObjectScope {
    Inner,
    Around,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VimTextObject {
    Word,
    BigWord,
    Parentheses,
    Brackets,
    Braces,
    DoubleQuote,
    SingleQuote,
    Backtick,
}

impl TextArea {
    pub(super) fn handle_vim_pending(&mut self, event: KeyEvent) -> bool {
        let pending = std::mem::replace(&mut self.vim_pending, VimPending::None);
        match pending {
            VimPending::None => false,
            VimPending::Goto => {
                if plain_char(event, 'g') {
                    self.set_cursor(0);
                }
                true
            }
            VimPending::Operator(operator) => {
                self.handle_vim_operator(operator, event);
                true
            }
            VimPending::TextObject { operator, scope } => {
                self.handle_vim_text_object(operator, scope, event);
                true
            }
        }
    }

    fn handle_vim_operator(&mut self, operator: VimOperator, event: KeyEvent) {
        if operator == VimOperator::Delete && plain_char(event, 'd') {
            self.kill_current_line();
            return;
        }
        if operator == VimOperator::Yank && plain_char(event, 'y') {
            self.yank_current_line();
            return;
        }
        if event.code == KeyCode::Esc && event.modifiers == KeyModifiers::NONE {
            return;
        }
        if let Some(scope) = text_object_scope_for_event(event) {
            self.vim_pending = VimPending::TextObject { operator, scope };
            return;
        }
        if operator != VimOperator::Change
            && let Some(motion) = motion_for_event(event)
        {
            self.apply_vim_operator(operator, motion);
        }
    }

    fn handle_vim_text_object(
        &mut self,
        operator: VimOperator,
        scope: VimTextObjectScope,
        event: KeyEvent,
    ) {
        if event.code == KeyCode::Esc && event.modifiers == KeyModifiers::NONE {
            return;
        }
        let Some(object) = text_object_for_event(event) else {
            return;
        };
        if let Some(range) = self.text_object_range(object, scope) {
            self.apply_vim_operator_to_range(operator, range);
        }
    }

    fn apply_vim_operator(&mut self, operator: VimOperator, motion: VimMotion) {
        let Some((range, kind)) = self.range_for_motion(motion) else {
            return;
        };
        match operator {
            VimOperator::Delete => self.kill_range_with_kind(range, kind),
            VimOperator::Yank => self.yank_range_with_kind(range, kind),
            VimOperator::Change => {}
        }
    }

    fn apply_vim_operator_to_range(&mut self, operator: VimOperator, range: Range<usize>) {
        match operator {
            VimOperator::Delete => self.kill_range(range),
            VimOperator::Yank => self.yank_range(range),
            VimOperator::Change => {
                self.begin_undo_group();
                self.kill_range(range);
                self.vim_mode_state = super::VimModeState::Insert;
            }
        }
    }

    fn range_for_motion(&mut self, motion: VimMotion) -> Option<(Range<usize>, KillBufferKind)> {
        if matches!(motion, VimMotion::Up | VimMotion::Down) {
            return self
                .linewise_range_for_vertical_motion(motion)
                .map(|range| (range, KillBufferKind::Linewise));
        }
        let start = self.cursor_pos;
        let target = self.target_for_motion(motion);
        if start == target {
            return None;
        }
        let range = if target < start {
            target..start
        } else {
            start..target
        };
        Some((range, KillBufferKind::Characterwise))
    }

    fn linewise_range_for_vertical_motion(&self, motion: VimMotion) -> Option<Range<usize>> {
        let current = self.current_line_range_with_newline();
        let range = match motion {
            VimMotion::Up => {
                let start = if current.start == 0 {
                    current.start
                } else {
                    self.beginning_of_line(current.start.saturating_sub(1))
                };
                start..current.end
            }
            VimMotion::Down => {
                let end = if current.end >= self.text.len() {
                    current.end
                } else {
                    let next_eol = self.end_of_line(current.end);
                    if next_eol < self.text.len() {
                        next_eol + 1
                    } else {
                        next_eol
                    }
                };
                current.start..end
            }
            _ => return None,
        };
        (range.start < range.end).then_some(range)
    }

    fn target_for_motion(&mut self, motion: VimMotion) -> usize {
        let original_cursor = self.cursor_pos;
        let original_preferred = self.preferred_col;
        match motion {
            VimMotion::Left => self.move_cursor_left(),
            VimMotion::Right => self.move_cursor_right(),
            VimMotion::Up => self.move_cursor_up(),
            VimMotion::Down => self.move_cursor_down(),
            VimMotion::WordForward => self.set_cursor(self.beginning_of_next_word()),
            VimMotion::WordBackward => self.set_cursor(self.beginning_of_previous_word()),
            VimMotion::WordEnd => self.set_cursor(self.end_of_next_word()),
            VimMotion::LineStart => self.set_cursor(self.beginning_of_current_line()),
            VimMotion::LineEnd => self.set_cursor(self.end_of_current_line()),
        }
        let target = self.cursor_pos;
        self.cursor_pos = original_cursor;
        self.preferred_col = original_preferred;
        target
    }

    fn text_object_range(
        &self,
        object: VimTextObject,
        scope: VimTextObjectScope,
    ) -> Option<Range<usize>> {
        match object {
            VimTextObject::Word => self.word_text_object_range(scope, false),
            VimTextObject::BigWord => self.word_text_object_range(scope, true),
            VimTextObject::Parentheses => self.paired_text_object_range(scope, '(', ')'),
            VimTextObject::Brackets => self.paired_text_object_range(scope, '[', ']'),
            VimTextObject::Braces => self.paired_text_object_range(scope, '{', '}'),
            VimTextObject::DoubleQuote => self.quoted_text_object_range(scope, '"'),
            VimTextObject::SingleQuote => self.quoted_text_object_range(scope, '\''),
            VimTextObject::Backtick => self.quoted_text_object_range(scope, '`'),
        }
    }

    fn word_text_object_range(
        &self,
        scope: VimTextObjectScope,
        big_word: bool,
    ) -> Option<Range<usize>> {
        let inner = if big_word {
            self.big_word_range_at_cursor()?
        } else {
            self.small_word_range_at_cursor()?
        };
        Some(match scope {
            VimTextObjectScope::Inner => inner,
            VimTextObjectScope::Around => self.expand_word_around(inner),
        })
    }

    fn big_word_range_at_cursor(&self) -> Option<Range<usize>> {
        self.non_ws_runs()
            .into_iter()
            .find(|range| self.cursor_overlaps_range(range) || self.cursor_is_at_range_end(range))
    }

    fn small_word_range_at_cursor(&self) -> Option<Range<usize>> {
        for run in self.non_ws_runs() {
            if !self.cursor_overlaps_range(&run) && !self.cursor_is_at_range_end(&run) {
                continue;
            }
            let mut last_piece = None;
            for (piece_start, piece) in split_word_pieces(&self.text[run.clone()]) {
                let piece = run.start + piece_start..run.start + piece_start + piece.len();
                if self.cursor_overlaps_range(&piece) {
                    return Some(piece);
                }
                last_piece = Some(piece);
            }
            if self.cursor_is_at_range_end(&run) {
                return last_piece.or(Some(run));
            }
            return Some(run);
        }
        None
    }

    fn non_ws_runs(&self) -> Vec<Range<usize>> {
        let mut runs = Vec::new();
        let mut start = None;
        for (idx, ch) in self.text.char_indices() {
            if ch.is_whitespace() {
                if let Some(run_start) = start.take() {
                    runs.push(run_start..idx);
                }
            } else if start.is_none() {
                start = Some(idx);
            }
        }
        if let Some(run_start) = start {
            runs.push(run_start..self.text.len());
        }
        runs
    }

    fn cursor_overlaps_range(&self, range: &Range<usize>) -> bool {
        range.start <= self.cursor_pos && self.cursor_pos < range.end
    }

    fn cursor_is_at_range_end(&self, range: &Range<usize>) -> bool {
        range.start < range.end && self.cursor_pos == range.end
    }

    fn expand_word_around(&self, inner: Range<usize>) -> Range<usize> {
        let following = self.following_whitespace_end(inner.end);
        if following > inner.end {
            return inner.start..following;
        }
        self.preceding_whitespace_start(inner.start)..inner.end
    }

    fn following_whitespace_end(&self, start: usize) -> usize {
        let mut end = start;
        for (offset, ch) in self.text[start..].char_indices() {
            if !ch.is_whitespace() {
                break;
            }
            end = start + offset + ch.len_utf8();
        }
        end
    }

    fn preceding_whitespace_start(&self, end: usize) -> usize {
        let mut start = end;
        for (idx, ch) in self.text[..end].char_indices().rev() {
            if !ch.is_whitespace() {
                break;
            }
            start = idx;
        }
        start
    }

    fn paired_text_object_range(
        &self,
        scope: VimTextObjectScope,
        open: char,
        close: char,
    ) -> Option<Range<usize>> {
        let mut stack = Vec::new();
        let mut best: Option<Range<usize>> = None;
        for (idx, ch) in self.text.char_indices() {
            if self.is_inside_element(idx) {
                continue;
            }
            if ch == open {
                stack.push(idx);
            } else if ch == close {
                let Some(open_idx) = stack.pop() else {
                    continue;
                };
                let close_end = idx + ch.len_utf8();
                if open_idx <= self.cursor_pos && self.cursor_pos <= idx {
                    let candidate = match scope {
                        VimTextObjectScope::Inner => open_idx + open.len_utf8()..idx,
                        VimTextObjectScope::Around => open_idx..close_end,
                    };
                    if best
                        .as_ref()
                        .is_none_or(|current| candidate.len() < current.len())
                    {
                        best = Some(candidate);
                    }
                }
            }
        }
        best
    }

    fn quoted_text_object_range(
        &self,
        scope: VimTextObjectScope,
        quote: char,
    ) -> Option<Range<usize>> {
        let line = self.beginning_of_current_line()..self.end_of_current_line();
        let mut open = None;
        let mut best: Option<Range<usize>> = None;
        for (offset, ch) in self.text[line.clone()].char_indices() {
            let idx = line.start + offset;
            if self.is_inside_element(idx) || ch != quote || self.is_escaped(idx) {
                continue;
            }
            if let Some(open_idx) = open.take() {
                if open_idx <= self.cursor_pos && self.cursor_pos <= idx {
                    let candidate = match scope {
                        VimTextObjectScope::Inner => open_idx + quote.len_utf8()..idx,
                        VimTextObjectScope::Around => open_idx..idx + quote.len_utf8(),
                    };
                    if best
                        .as_ref()
                        .is_none_or(|current| candidate.len() < current.len())
                    {
                        best = Some(candidate);
                    }
                }
            } else {
                open = Some(idx);
            }
        }
        best
    }

    fn is_inside_element(&self, pos: usize) -> bool {
        self.elements
            .iter()
            .any(|element| pos >= element.range.start && pos < element.range.end)
    }

    fn is_escaped(&self, pos: usize) -> bool {
        self.text[..pos]
            .chars()
            .rev()
            .take_while(|ch| *ch == '\\')
            .count()
            % 2
            == 1
    }

    pub(super) fn kill_range_with_kind(&mut self, range: Range<usize>, kind: KillBufferKind) {
        let range = self.expand_range_to_element_boundaries(range);
        if range.start >= range.end {
            return;
        }
        let removed = self.text[range.clone()].to_string();
        if removed.is_empty() {
            return;
        }
        self.kill_buffer = removed;
        self.kill_buffer_kind = kind;
        self.replace_range_raw(range, "");
    }

    fn yank_range(&mut self, range: Range<usize>) {
        self.yank_range_with_kind(range, KillBufferKind::Characterwise);
    }

    fn yank_range_with_kind(&mut self, range: Range<usize>, kind: KillBufferKind) {
        let range = self.expand_range_to_element_boundaries(range);
        if range.start >= range.end {
            return;
        }
        let text = self.text[range].to_string();
        if !text.is_empty() {
            self.kill_buffer = text;
            self.kill_buffer_kind = kind;
        }
    }

    pub(super) fn paste_after_cursor(&mut self) {
        if self.kill_buffer.is_empty() {
            return;
        }
        if self.kill_buffer_kind == KillBufferKind::Linewise {
            self.paste_line_after_current_line();
            return;
        }
        let insert_at = self.next_atomic_boundary(self.cursor_pos);
        self.set_cursor(insert_at);
        let text = self.kill_buffer.clone();
        self.insert_str(&text);
    }

    fn paste_line_after_current_line(&mut self) {
        let eol = self.end_of_current_line();
        let insert_at = if eol < self.text.len() { eol + 1 } else { eol };
        let cursor = if eol < self.text.len() {
            insert_at
        } else {
            insert_at + 1
        };
        let text = if eol < self.text.len() {
            if self.kill_buffer.ends_with('\n') {
                self.kill_buffer.clone()
            } else {
                format!("{}\n", self.kill_buffer)
            }
        } else {
            format!("\n{}", self.kill_buffer.trim_end_matches('\n'))
        };
        self.insert_str_at(insert_at, &text);
        self.set_cursor(cursor.min(self.text.len()));
    }
}
