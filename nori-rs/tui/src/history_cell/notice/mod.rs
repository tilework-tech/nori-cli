//! One inset notice cell for information that is not part of the conversation.
//!
//! CLI updates, ACP adapter updates, adapter stderr, invitations, and tips
//! share this cell: a dim rail two columns in, a bold source with a dim kind,
//! a short message, and while the notice is active one optional action.
//!
//! The cell uses no hues, only bold and dim, so it reads the same on any
//! terminal palette and with `NO_COLOR`.
//!
//! [`NoticeSlot`] decides which keys the active notice may take. The prompt
//! always comes first: a notice reacts to Tab (its action) or Esc (dismiss)
//! only while the prompt is empty, and sending a prompt settles every active
//! notice into history before the prompt itself.
//!
//! Adapter stderr is inert. Escape sequences and control characters are
//! removed, the text is shown as the adapter wrote it, and it never carries an
//! action, so nothing an adapter prints can run something or claim a sign-in
//! state Nori has not checked.

use std::collections::VecDeque;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use unicode_width::UnicodeWidthStr;

use super::HistoryCell;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;

pub mod fixtures;

/// Stderr rows shown before the rest is summarized.
const STDERR_ROWS: usize = 4;
const TAB_WIDTH: usize = 4;
/// Characters kept from one stderr line.
const STDERR_LINE_CHARS: usize = 200;
const RAIL_WIDTH: usize = 4;
const HINT_GAP: &str = "  ";

/// The one thing an active notice can do for the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoticeAction {
    id: &'static str,
    label: String,
    done: String,
}

impl NoticeAction {
    /// `label` follows the key in the hint; `done` replaces the hint once
    /// the action has run.
    #[must_use]
    pub fn new(id: &'static str, label: &str, done: &str) -> Self {
        Self {
            id,
            label: label.to_string(),
            done: done.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NoticeState {
    Active,
    Dismissed,
    Invoked,
}

/// One notice, active below the transcript or settled into it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    source: String,
    kind: String,
    body: Vec<String>,
    hidden_rows: i64,
    inert: bool,
    action: Option<NoticeAction>,
    later: Option<String>,
    state: NoticeState,
}

impl Notice {
    #[must_use]
    pub fn new(source: &str, kind: &str, message: &str) -> Self {
        Self {
            source: source.to_string(),
            kind: kind.to_string(),
            body: message.lines().map(str::to_string).collect(),
            hidden_rows: 0,
            inert: false,
            action: None,
            later: None,
            state: NoticeState::Active,
        }
    }

    /// Adapter stderr, cleaned of terminal control and shown as inert text.
    /// Returns `None` when nothing visible is left.
    #[must_use]
    pub fn from_stderr(adapter: &str, output: &str) -> Option<Self> {
        let text = inert_text(output);
        let mut rows: Vec<&str> = text.lines().map(str::trim_end).collect();
        while rows.last().is_some_and(|row| row.is_empty()) {
            rows.pop();
        }
        let first = rows.iter().position(|row| !row.is_empty())?;
        rows.drain(..first);
        let mut hidden_rows = 0;
        if rows.len() > STDERR_ROWS + 1 {
            hidden_rows = i64::try_from(rows.len() - STDERR_ROWS).unwrap_or(i64::MAX);
            rows.truncate(STDERR_ROWS);
        }
        let mut body = Vec::new();
        for row in rows {
            let length = row.chars().count();
            if length <= STDERR_LINE_CHARS {
                body.push(row.to_string());
                continue;
            }
            let kept: String = row.chars().take(STDERR_LINE_CHARS).collect();
            body.push(format!("{kept}…"));
            body.push(format!("+{} more characters", length - STDERR_LINE_CHARS));
        }
        Some(Self {
            body,
            hidden_rows,
            inert: true,
            ..Self::new(adapter, "stderr", "")
        })
    }

    /// Offer an action. Stderr notices ignore it.
    #[must_use]
    pub fn with_action(mut self, action: NoticeAction) -> Self {
        if !self.inert {
            self.action = Some(action);
        }
        self
    }

    /// A line kept in history when the notice is dismissed, usually the
    /// command that does the same thing later.
    #[must_use]
    pub fn with_later(mut self, later: &str) -> Self {
        self.later = Some(later.to_string());
        self
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The rows the notice takes at `width`, rail included.
    #[must_use]
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let rail = || Line::from(vec!["  ".into(), "▎".dim(), " ".into()]);
        let mut rows: Vec<Line<'static>> = vec![
            vec![
                self.source.clone().bold(),
                " · ".dim(),
                self.kind.clone().dim(),
            ]
            .into(),
        ];
        rows.extend(self.body.iter().map(|row| Line::from(row.clone())));
        if self.hidden_rows > 0 {
            let more = format!("+{} more lines", self.hidden_rows);
            rows.push(Line::from(more.dim()));
        }
        match (self.state, &self.action) {
            (NoticeState::Active, action) => {
                let mut hints = Vec::new();
                if let Some(action) = action {
                    hints.push(("Tab", action.label.as_str()));
                }
                hints.push(("Esc", "dismiss"));
                rows.extend(hint_rows(
                    &hints,
                    usize::from(width).saturating_sub(RAIL_WIDTH),
                ));
            }
            (NoticeState::Invoked, Some(action)) => {
                rows.push(vec!["✓ ".bold(), Span::from(action.done.clone())].into());
            }
            (NoticeState::Dismissed | NoticeState::Invoked, _) => {
                if let Some(later) = &self.later {
                    rows.push(Line::from(later.clone().dim()));
                }
            }
        }
        let mut lines = word_wrap_lines(
            &rows,
            RtOptions::new(usize::from(width))
                .initial_indent(rail())
                .subsequent_indent(rail())
                .word_splitter(textwrap::WordSplitter::NoHyphenation),
        );
        for line in &mut lines {
            if line
                .spans
                .iter()
                .skip(3)
                .all(|span| span.content.is_empty())
            {
                line.spans.truncate(2);
            }
        }
        lines
    }

    fn settle(mut self, state: NoticeState) -> Self {
        self.state = state;
        self
    }
}

impl HistoryCell for Notice {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.lines(width)
    }

    fn desired_height(&self, width: u16) -> u16 {
        u16::try_from(self.lines(width).len()).unwrap_or(u16::MAX)
    }
}

/// Whether the prompt may give up Tab and Esc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PromptState {
    pub is_empty: bool,
    /// A turn is running, a popup is open, the composer is in vim or shell
    /// mode, or a paste burst is buffering keys: Tab and Esc already mean
    /// something there.
    pub is_busy: bool,
}

/// What a key did to the active notice.
#[derive(Debug, PartialEq, Eq)]
pub enum NoticeKey {
    /// The key belongs to the prompt.
    PassThrough,
    /// The active notice settled. `invoked` names the action the caller
    /// must run, if any; `notice` goes into history.
    Settled {
        invoked: Option<&'static str>,
        notice: Notice,
    },
}

/// Notices waiting below the transcript, first one active.
#[derive(Debug, Default)]
pub struct NoticeSlot {
    waiting: VecDeque<Notice>,
}

impl NoticeSlot {
    pub fn offer(&mut self, notice: Notice) {
        self.waiting.push_back(notice);
    }

    #[must_use]
    pub fn active(&self) -> Option<&Notice> {
        self.waiting.front()
    }

    /// Route a key before the prompt sees it. Tab and Esc reach the notice
    /// only as plain presses on an empty, idle prompt.
    pub fn handle_key(&mut self, key: KeyEvent, prompt: PromptState) -> NoticeKey {
        if !prompt.is_empty
            || prompt.is_busy
            || key.kind != KeyEventKind::Press
            || key.modifiers != KeyModifiers::NONE
        {
            return NoticeKey::PassThrough;
        }
        let (invoked, state) = match (key.code, self.waiting.front()) {
            (
                KeyCode::Tab,
                Some(Notice {
                    action: Some(action),
                    ..
                }),
            ) => (Some(action.id), NoticeState::Invoked),
            (KeyCode::Esc, Some(_)) => (None, NoticeState::Dismissed),
            _ => return NoticeKey::PassThrough,
        };
        match self.waiting.pop_front() {
            Some(notice) => NoticeKey::Settled {
                invoked,
                notice: notice.settle(state),
            },
            None => NoticeKey::PassThrough,
        }
    }

    /// The user sent a prompt: every notice settles, in order, ahead of it.
    pub fn on_submit(&mut self) -> Vec<Notice> {
        self.waiting
            .drain(..)
            .map(|notice| notice.settle(NoticeState::Dismissed))
            .collect()
    }
}

/// Key hints packed onto as few rows as fit, never splitting a key from its
/// label.
fn hint_rows(hints: &[(&'static str, &str)], width: usize) -> Vec<Line<'static>> {
    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut used = 0;
    for (key, label) in hints {
        let hint_width = key.width() + 1 + label.width();
        match rows.last_mut() {
            Some(row) if used + HINT_GAP.width() + hint_width <= width => {
                row.spans.push(HINT_GAP.into());
                used += HINT_GAP.width();
            }
            _ => {
                rows.push(Line::default());
                used = 0;
            }
        }
        if let Some(row) = rows.last_mut() {
            row.spans.push(key.bold().dim());
            row.spans.push(format!(" {label}").dim());
        }
        used += hint_width;
    }
    rows
}

/// Drop terminal escape sequences, control and format characters (which
/// could reorder or hide text), and expand tabs. A carriage return that
/// rewrites a line in a terminal starts a new row instead.
fn inert_text(output: &str) -> String {
    let mut text = String::with_capacity(output.len());
    let mut chars = output.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                Some('[') => skip_csi(&mut chars),
                Some(']' | 'P' | 'X' | '_' | '^') => {
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' || (c == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                            break;
                        }
                    }
                }
                Some('N' | 'O') => {
                    chars.next();
                }
                Some(' '..='/') => {
                    while chars.next_if(|c| (' '..='/').contains(c)).is_some() {}
                    chars.next();
                }
                Some('\n') => text.push('\n'),
                Some(_) | None => {}
            },
            '\u{9b}' => skip_csi(&mut chars),
            '\r' => {
                if chars.peek() != Some(&'\n') {
                    text.push('\n');
                }
            }
            '\n' => text.push('\n'),
            '\t' => text.push_str(&" ".repeat(TAB_WIDTH)),
            '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2069}'
            | '\u{feff}' => {}
            c if c.is_control() => {}
            c => text.push(c),
        }
    }
    text
}

fn skip_csi(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while chars
        .next()
        .is_some_and(|c| !('\u{40}'..='\u{7e}').contains(&c))
    {}
}

#[cfg(test)]
mod tests;
