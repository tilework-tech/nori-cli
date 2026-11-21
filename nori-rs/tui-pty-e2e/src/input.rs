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

/// How many times `screen` shows the composer echoing `needle`, a
/// whitespace-free input line.
///
/// A leading `/` or `!` only counts when it renders as the composer's mode
/// sigil, followed by whitespace. The slash popup lists commands as `/name`
/// rows, which would otherwise match as soon as the popup opens on a prefix,
/// before the rest of the input has reached the composer. The input must
/// therefore start the composer line; typing a slash command after existing
/// draft text will not match.
fn composer_echoes(screen: &str, needle: &str) -> usize {
    let Some(rest) = needle.strip_prefix(['/', '!']) else {
        return without_whitespace(screen).matches(needle).count();
    };
    let sigil = &needle[..1];
    screen
        .match_indices(sigil)
        .filter(|(index, _)| {
            let after = &screen[index + 1..];
            after.starts_with(char::is_whitespace) && without_whitespace(after).starts_with(rest)
        })
        .count()
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
        // transcript. Popup rows are excluded by `composer_echoes`.
        self.poll().map_err(|error| error.to_string())?;
        let before = composer_echoes(&self.screen_contents(), &needle);
        self.send_str(input)
            .map_err(|error| format!("type {input:?}: {error}"))?;
        let expected = needle.clone();
        let rendered = move |screen: &str| composer_echoes(screen, &expected) > before;
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

#[cfg(test)]
mod tests {
    use super::composer_echoes;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_slash_popup_row_is_not_a_composer_echo() {
        let screen = "/ co\n\n  /compact          summarize conversation\n  /context          visualize context usage\n";

        assert_eq!(composer_echoes(screen, "/context"), 0);
    }

    #[test]
    fn a_slash_command_counts_once_the_composer_renders_it() {
        let screen = "/ context\n\n  /context          visualize context usage\n";

        assert_eq!(composer_echoes(screen, "/context"), 1);
    }

    #[test]
    fn a_shell_command_counts_once_the_composer_renders_its_sigil() {
        let screen = "  run !ls in the shell\n! ls -la\n";

        assert_eq!(composer_echoes(screen, "!ls-la"), 1);
    }

    #[test]
    fn plain_input_matches_across_a_wrap() {
        let screen = "› write a long\n  sentence here\n";

        assert_eq!(composer_echoes(screen, "writealongsentencehere"), 1);
    }
}
