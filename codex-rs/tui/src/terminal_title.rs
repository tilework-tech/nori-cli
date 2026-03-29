//! Terminal title management via OSC escape sequences.
//!
//! Sets the terminal window/tab title using OSC 0, which sets both the icon
//! name and window title. This allows users to see Nori's activity status
//! (idle/working) in their terminal tab even when the tab is not focused.
//!
//! This module does **not** attempt to read or restore the terminal's previous
//! title because that is not portable across terminals. On exit the title is
//! cleared (set to an empty string).

use std::fmt;
use std::io;
use std::io::IsTerminal;
use std::io::stdout;
use std::time::Duration;
use std::time::Instant;

use crossterm::Command;
use crossterm::execute;

/// Maximum number of characters allowed in the terminal title.
const MAX_TERMINAL_TITLE_CHARS: usize = 240;

/// Braille-pattern dot-spinner frames for the terminal title animation.
pub(crate) const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Time between spinner frame advances in the terminal title.
pub(crate) const SPINNER_INTERVAL: Duration = Duration::from_millis(100);

/// Write a sanitized OSC 0 window title to stdout.
///
/// Returns `Ok(())` even when stdout is not a terminal (the write is simply
/// skipped).
pub(crate) fn set_terminal_title(title: &str) -> io::Result<()> {
    if !stdout().is_terminal() {
        return Ok(());
    }
    let title = sanitize_terminal_title(title);
    if title.is_empty() {
        return Ok(());
    }
    execute!(stdout(), SetWindowTitle(title))
}

/// Clear the terminal title by writing an empty OSC 0 payload.
pub(crate) fn clear_terminal_title() -> io::Result<()> {
    if !stdout().is_terminal() {
        return Ok(());
    }
    execute!(stdout(), SetWindowTitle(String::new()))
}

/// Custom crossterm command that writes an OSC 0 sequence to set both the
/// icon name and window title.
#[derive(Debug, Clone)]
struct SetWindowTitle(String);

impl Command for SetWindowTitle {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b]0;{}\x1b\\", self.0)
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> Result<()> {
        Err(io::Error::other("SetWindowTitle: use ANSI, not WinAPI"))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

/// Sanitize a string for use as a terminal title.
///
/// - Strips control characters and disallowed invisible/bidi codepoints.
/// - Collapses runs of whitespace to a single space.
/// - Truncates to [`MAX_TERMINAL_TITLE_CHARS`].
pub(crate) fn sanitize_terminal_title(input: &str) -> String {
    let mut result = String::with_capacity(input.len().min(MAX_TERMINAL_TITLE_CHARS + 1));
    let mut prev_was_space = true; // trim leading whitespace

    for ch in input.chars() {
        if ch.is_control() || is_disallowed_terminal_title_char(ch) {
            continue;
        }
        if ch.is_whitespace() {
            if !prev_was_space {
                result.push(' ');
                prev_was_space = true;
            }
            continue;
        }
        prev_was_space = false;
        result.push(ch);

        if result.chars().count() >= MAX_TERMINAL_TITLE_CHARS {
            break;
        }
    }

    // Trim trailing space
    if result.ends_with(' ') {
        result.pop();
    }

    result
}

/// Returns the spinner frame for the given elapsed time since animation start.
pub(crate) fn spinner_frame_at(animation_origin: Instant, now: Instant) -> &'static str {
    let elapsed = now.saturating_duration_since(animation_origin);
    let frame_index = (elapsed.as_millis() / SPINNER_INTERVAL.as_millis()) as usize;
    SPINNER_FRAMES[frame_index % SPINNER_FRAMES.len()]
}

/// Compose the terminal title string.
///
/// When `spinner_frame` is `Some`, the title is `"{spinner} {project}"`.
/// When `None`, the title is just `"{project}"`.
pub(crate) fn compose_title(project_name: &str, spinner_frame: Option<&str>) -> String {
    match spinner_frame {
        Some(frame) => format!("{frame} {project_name}"),
        None => project_name.to_string(),
    }
}

/// Returns `true` for Unicode codepoints that should not appear in terminal
/// titles (bidi overrides, zero-width characters, BOM, etc.).
fn is_disallowed_terminal_title_char(ch: char) -> bool {
    matches!(
        ch,
        // Bidi overrides and embedding
        '\u{202A}'..='\u{202E}'
        // Zero-width and directional formatting
        | '\u{200B}'..='\u{200F}'
        // BOM / zero-width no-break space
        | '\u{FEFF}'
        // Bidi isolates
        | '\u{2066}'..='\u{2069}'
        // Interlinear annotation anchors
        | '\u{FFF9}'..='\u{FFFB}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn sanitize_strips_control_characters() {
        let input = "hello\x00world\x07foo\x1b[31mbar";
        let result = sanitize_terminal_title(input);
        // \x00, \x07, and \x1b are control chars and get stripped.
        // The `[`, `3`, `1`, `m` are regular printable chars and remain.
        assert_eq!(result, "helloworldfoo[31mbar");
    }

    #[test]
    fn sanitize_collapses_whitespace() {
        let input = "  hello   world  ";
        let result = sanitize_terminal_title(input);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn sanitize_strips_bidi_characters() {
        let input = "hello\u{202A}world\u{200B}foo";
        let result = sanitize_terminal_title(input);
        assert_eq!(result, "helloworldfoo");
    }

    #[test]
    fn sanitize_truncates_long_strings() {
        let input = "a".repeat(300);
        let result = sanitize_terminal_title(&input);
        assert_eq!(result.chars().count(), MAX_TERMINAL_TITLE_CHARS);
    }

    #[test]
    fn sanitize_empty_input_returns_empty() {
        assert_eq!(sanitize_terminal_title(""), "");
        assert_eq!(sanitize_terminal_title("   "), "");
        assert_eq!(sanitize_terminal_title("\x00\x07"), "");
    }

    #[test]
    fn sanitize_preserves_unicode_text() {
        let input = "Nori — 日本語プロジェクト";
        let result = sanitize_terminal_title(input);
        assert_eq!(result, "Nori — 日本語プロジェクト");
    }

    #[test]
    fn spinner_frame_cycles_through_all_frames() {
        let origin = Instant::now();
        for (i, expected_frame) in SPINNER_FRAMES.iter().enumerate() {
            let now = origin + SPINNER_INTERVAL * i as u32;
            assert_eq!(
                spinner_frame_at(origin, now),
                *expected_frame,
                "frame at index {i}"
            );
        }
    }

    #[test]
    fn spinner_frame_wraps_around() {
        let origin = Instant::now();
        // After 10 frames (one full cycle), should be back to frame 0
        let now = origin + SPINNER_INTERVAL * SPINNER_FRAMES.len() as u32;
        assert_eq!(spinner_frame_at(origin, now), SPINNER_FRAMES[0]);
    }

    #[test]
    fn compose_title_with_spinner() {
        let title = compose_title("my-project", Some("⠋"));
        assert_eq!(title, "⠋ my-project");
    }

    #[test]
    fn compose_title_without_spinner() {
        let title = compose_title("my-project", None);
        assert_eq!(title, "my-project");
    }
}
