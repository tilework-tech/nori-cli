//! Composer input helpers for the PTY end-to-end tests.

use crate::Key;
use crate::TIMEOUT;
use crate::TuiSession;

/// `text` with every run of whitespace removed.
///
/// Screen matching has to be whitespace-insensitive because the composer does
/// not echo input back byte for byte: a leading `/` or `!` renders as a mode
/// sigil followed by a space (`/remote-control status` becomes
/// `/ remote-control status`), and long input wraps across lines.
fn without_whitespace(text: &str) -> String {
    text.split_whitespace().collect()
}

impl TuiSession {
    /// Type `input` into the composer and wait until the composer has rendered
    /// it.
    ///
    /// Prefer this over a bare [`TuiSession::send_str`] followed by a sleep.
    /// `send_str` writes the whole string into the PTY in one burst, so the
    /// composer classifies it as a paste: the characters are buffered and held
    /// out of the textarea until a flush tick fires, and while that buffer is
    /// live Enter is appended to it as a newline instead of submitting. No
    /// fixed sleep settles that — a busy event loop (spawning or activating an
    /// agent child) pushes the flush past any constant, and the swallowed
    /// Enter then leaves the prompt sitting unsent in the composer. The
    /// rendered text is the flush's observable effect, so it is the point at
    /// which the next key is safe to send.
    ///
    /// Use [`TuiSession::submit_input`] to type and submit in one step.
    pub fn type_input(&mut self, input: &str) -> Result<(), String> {
        // A multi-line input arrives in one burst, so the last non-blank line
        // is the last thing to render.
        let needle = input
            .rsplit('\n')
            .find(|line| !line.trim().is_empty())
            .map(without_whitespace)
            .unwrap_or_default();
        if needle.is_empty() {
            return self
                .send_str(input)
                .map_err(|error| format!("type {input:?}: {error}"));
        }
        // Count what is already on screen rather than waiting for the text
        // outright: a prompt repeated within a session is still in the
        // transcript, and an input that filters a popup also appears in the
        // popup's own rows.
        self.poll().map_err(|error| error.to_string())?;
        let visible = without_whitespace(&self.screen_contents());
        let before = visible.matches(needle.as_str()).count();
        self.send_str(input)
            .map_err(|error| format!("type {input:?}: {error}"))?;
        let expected = needle.clone();
        let rendered = move |screen: &str| {
            let visible = without_whitespace(screen);
            visible.matches(expected.as_str()).count() > before
        };
        self.wait_for(rendered, TIMEOUT)
            .map_err(|error| format!("the composer never rendered {needle:?}: {error}"))
    }

    /// Type `input` and submit it once the composer has rendered it.
    pub fn submit_input(&mut self, input: &str) -> Result<(), String> {
        self.type_input(input)?;
        self.send_key(Key::Enter)
            .map_err(|error| format!("submit {input:?}: {error}"))
    }
}
