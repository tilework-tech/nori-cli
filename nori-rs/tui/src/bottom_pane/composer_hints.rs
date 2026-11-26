//! The keys that work in the composer right now, shown under the prompt.

use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

/// What the prompt holds, as far as Enter is concerned.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum HintDraft {
    #[default]
    Empty,
    /// Text that Enter sends to the agent.
    Message,
    /// A slash command, `exit` or `quit`, which Enter runs here.
    Command,
    /// A `!` shell command; `empty` until a command is typed.
    Shell { empty: bool },
}

/// Inputs for the hint row. The chat widget knows the session's state; the
/// composer fills in `draft`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ComposerHints {
    pub(crate) draft: HintDraft,
    /// Enter may send. False for watchers and while reconnecting.
    pub(crate) can_submit: bool,
    /// A turn this terminal started is running.
    pub(crate) own_turn: bool,
    /// The agent accepts a prompt into its running turn.
    pub(crate) steering: bool,
    /// Enter on an empty prompt resumes a paused queue.
    pub(crate) queue_paused: bool,
    /// `!` runs a shell command for this session.
    pub(crate) shell: bool,
    /// `@` finds files where the session runs.
    pub(crate) files: bool,
}

impl Default for ComposerHints {
    fn default() -> Self {
        Self {
            draft: HintDraft::Empty,
            can_submit: true,
            own_turn: false,
            steering: false,
            queue_paused: false,
            shell: true,
            files: true,
        }
    }
}

impl ComposerHints {
    /// One line per hint, most useful first, so a narrow row can drop
    /// trailing ones whole.
    pub(crate) fn items(self, use_shift_enter_hint: bool) -> Vec<Line<'static>> {
        let newline = if use_shift_enter_hint {
            "Shift+Enter"
        } else {
            "Ctrl+J"
        };
        let mut items = Vec::new();
        match self.draft {
            HintDraft::Empty => {
                if self.can_submit && self.queue_paused {
                    items.push(hint("Enter", "resume queue"));
                }
                items.push(hint("/", "commands"));
                if self.can_submit && self.shell {
                    items.push(hint("!", "shell"));
                }
                if self.files {
                    items.push(hint("@", "files"));
                }
                items.push(hint("?", "help"));
            }
            HintDraft::Message => {
                if self.can_submit {
                    match (self.own_turn, self.steering) {
                        (true, true) => {
                            items.push(hint("Enter", "steer"));
                            items.push(hint("Tab", "queue"));
                        }
                        // Without steering, Enter queues for the next turn
                        // exactly as Tab would.
                        (true, false) => items.push(hint("Enter", "queue")),
                        (false, _) => items.push(hint("Enter", "send")),
                    }
                }
                items.push(hint(newline, "newline"));
            }
            // Commands run here, so a watcher may run them too.
            HintDraft::Command => items.push(hint("Enter", "run")),
            HintDraft::Shell { empty } => {
                if !empty && self.can_submit && self.shell {
                    items.push(hint("Enter", "run"));
                }
            }
        }
        items
    }
}

/// Single-glyph keys stay at full color so they stand out; word keys such as
/// `Enter` are dimmed too, so they do not outweigh their labels.
fn hint(key: &'static str, label: &'static str) -> Line<'static> {
    let key = if key.chars().count() == 1 {
        Span::from(key).bold()
    } else {
        Span::from(key).bold().dim()
    };
    Line::from(vec![key, format!(" {label}").dim()])
}
