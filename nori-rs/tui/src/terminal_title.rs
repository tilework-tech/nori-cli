//! Terminal title management via OSC escape sequences.
//!
//! Sets the terminal window/tab title using OSC 0, which sets both the icon
//! name and window title. The title reads `agent · project · session`, led by
//! a spinner while a turn is working, so users can tell their tabs apart and
//! see which ones are busy without focusing them. Agent and project names are
//! cut short first because the session title is usually what tells tabs apart.
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
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Maximum number of characters allowed in the terminal title.
const MAX_TERMINAL_TITLE_CHARS: usize = 240;

/// Widest the agent name may be in the title, in terminal columns.
const MAX_AGENT_COLUMNS: usize = 12;

/// Widest the project name may be in the title, in terminal columns.
const MAX_PROJECT_COLUMNS: usize = 16;

/// Widest the session title may be in the title, in terminal columns.
const MAX_SESSION_COLUMNS: usize = 60;

/// Separator between the agent, project and session parts.
const PART_SEPARATOR: &str = " · ";

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
/// - Turns whitespace, including newlines and tabs, into single spaces.
/// - Strips other control characters and disallowed invisible/bidi codepoints.
/// - Truncates to [`MAX_TERMINAL_TITLE_CHARS`].
pub(crate) fn sanitize_terminal_title(input: &str) -> String {
    let mut result = String::with_capacity(input.len().min(MAX_TERMINAL_TITLE_CHARS + 1));
    let mut prev_was_space = true; // trim leading whitespace

    for ch in input.chars() {
        if ch.is_whitespace() {
            if !prev_was_space {
                result.push(' ');
                prev_was_space = true;
            }
            continue;
        }
        if ch.is_control() || is_disallowed_terminal_title_char(ch) {
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

/// What the terminal title names. Empty parts are left out.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TitleParts<'a> {
    /// The agent's display name, such as `Claude Code` or a custom agent's name.
    pub(crate) agent: &'a str,
    /// The project, named after the session's working directory.
    pub(crate) project: &'a str,
    /// The session title, if the session has one yet.
    pub(crate) session: Option<&'a str>,
}

/// Compose the terminal title string.
///
/// A titled session reads `agent · project · session`. Until the session has
/// a title, only the project is shown (the agent when there is no project).
/// Each part is sanitized and shortened to its own column limit, and
/// `spinner_frame`, when present, leads the title.
pub(crate) fn compose_title(parts: &TitleParts<'_>, spinner_frame: Option<&str>) -> String {
    let agent = shorten(&sanitize_terminal_title(parts.agent), MAX_AGENT_COLUMNS);
    let project = shorten(&sanitize_terminal_title(parts.project), MAX_PROJECT_COLUMNS);
    let session = parts
        .session
        .map(|session| shorten(&sanitize_terminal_title(session), MAX_SESSION_COLUMNS))
        .filter(|session| !session.is_empty());

    let named: Vec<&str> = match &session {
        Some(session) => vec![&agent, &project, session],
        None if project.is_empty() => vec![&agent],
        None => vec![&project],
    };
    let body = named
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(PART_SEPARATOR);
    match spinner_frame {
        Some(frame) if !body.is_empty() => format!("{frame} {body}"),
        Some(_) | None => body,
    }
}

/// Cut `text` to at most `max_columns` terminal columns, ending in `…` when
/// cut. Splits only between grapheme clusters.
fn shorten(text: &str, max_columns: usize) -> String {
    if text.width() <= max_columns {
        return text.to_string();
    }
    let budget = max_columns.saturating_sub(1);
    let mut used = 0;
    let mut end = 0;
    for (index, grapheme) in text.grapheme_indices(true) {
        let width = grapheme.width();
        if used + width > budget {
            break;
        }
        used += width;
        end = index + grapheme.len();
    }
    format!("{}…", text[..end].trim_end())
}

/// Returns `true` for Unicode codepoints that should not appear in terminal
/// titles (bidi overrides, zero-width characters, BOM, etc.).
fn is_disallowed_terminal_title_char(ch: char) -> bool {
    matches!(
        ch,
        // Bidi overrides and embedding
        '\u{202A}'..='\u{202E}'
        // Zero-width space and directional marks; the zero-width joiner and
        // non-joiner (U+200C, U+200D) stay because emoji and scripts need them
        | '\u{200B}'
        | '\u{200E}'..='\u{200F}'
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
    fn sanitize_keeps_joiners_inside_emoji_sequences() {
        assert_eq!(
            sanitize_terminal_title("👩\u{200D}💻 ship"),
            "👩\u{200D}💻 ship"
        );
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

    fn parts<'a>(agent: &'a str, project: &'a str, session: Option<&'a str>) -> TitleParts<'a> {
        TitleParts {
            agent,
            project,
            session,
        }
    }

    #[test]
    fn title_reads_agent_project_then_session() {
        let title = compose_title(
            &parts("Claude Code", "harbor-ledger", Some("Fix login flakes")),
            None,
        );
        assert_eq!(title, "Claude Code · harbor-ledger · Fix login flakes");
    }

    #[test]
    fn working_spinner_leads_the_title() {
        let title = compose_title(
            &parts("Codex", "harbor-ledger", Some("Fix login flakes")),
            Some("⠋"),
        );
        assert_eq!(title, "⠋ Codex · harbor-ledger · Fix login flakes");
    }

    #[test]
    fn untitled_session_falls_back_to_the_project_alone() {
        assert_eq!(
            compose_title(&parts("Pi", "harbor-ledger", None), None),
            "harbor-ledger"
        );
        assert_eq!(
            compose_title(&parts("Pi", "harbor-ledger", Some(" \n\t ")), Some("⠙")),
            "⠙ harbor-ledger"
        );
    }

    #[test]
    fn missing_parts_are_left_out_without_stray_separators() {
        assert_eq!(
            compose_title(&parts("", "harbor-ledger", Some("Fix login flakes")), None),
            "harbor-ledger · Fix login flakes"
        );
        assert_eq!(
            compose_title(&parts("Pi", "", Some("Fix login flakes")), None),
            "Pi · Fix login flakes"
        );
        assert_eq!(compose_title(&parts("Pi", "", None), None), "Pi");
        assert_eq!(compose_title(&parts("", "", None), Some("⠋")), "");
    }

    #[test]
    fn agent_and_project_shorten_before_the_session_title() {
        let title = compose_title(
            &parts(
                "Synthetic Review Specialist",
                "an-exceptionally-long-project-directory",
                Some("Fix login flakes"),
            ),
            None,
        );
        assert_eq!(title, "Synthetic R… · an-exceptionall… · Fix login flakes");
    }

    #[test]
    fn long_session_title_is_cut_at_its_own_limit() {
        let session = "word ".repeat(40);
        let title = compose_title(&parts("Pi", "shop", Some(&session)), None);
        let shown_session = title.strip_prefix("Pi · shop · ").expect("prefix");
        assert_eq!(shown_session.width(), MAX_SESSION_COLUMNS);
        assert!(shown_session.ends_with('…'), "{shown_session}");
    }

    #[test]
    fn wide_characters_are_shortened_by_columns_and_never_split() {
        let title = compose_title(
            &parts(
                "合成エージェント名前",
                "日本語のとても長いプロジェクト",
                Some("👩‍💻 ログインの不具合を直す"),
            ),
            None,
        );
        assert_eq!(
            title,
            "合成エージ… · 日本語のとても… · 👩‍💻 ログインの不具合を直す"
        );
        let mut pieces = title.split(" · ");
        assert!(pieces.next().expect("agent").width() <= MAX_AGENT_COLUMNS);
        assert!(pieces.next().expect("project").width() <= MAX_PROJECT_COLUMNS);
    }

    #[test]
    fn control_sequences_in_any_part_cannot_reach_the_terminal() {
        let title = compose_title(
            &parts(
                "Pi\x1b]0;spoof\x07",
                "shop\u{202E}",
                Some("Fix\x1b[2J bug\nsecond line\u{9b}"),
            ),
            Some("⠋"),
        );
        assert!(!title.chars().any(char::is_control), "{title:?}");
        assert!(!title.contains('\u{202E}'), "{title:?}");
        assert_eq!(title, "⠋ Pi]0;spoof · shop · Fix[2J bug second line");
    }

    #[test]
    fn title_scenarios_snapshot() {
        let long_session =
            "Investigate the intermittent checkout failure seen after the cache warmup";
        let scenarios: [(&str, TitleParts<'_>, Option<&str>); 7] = [
            (
                "fresh session",
                parts("Claude Code", "harbor-ledger", None),
                None,
            ),
            (
                "titled, idle",
                parts("Claude Code", "harbor-ledger", Some("Fix login flakes")),
                None,
            ),
            (
                "titled, working",
                parts("Codex", "harbor-ledger", Some("Fix login flakes")),
                Some("⠼"),
            ),
            (
                "custom agent",
                parts(
                    "night-shift-reviewer",
                    "harbor-ledger",
                    Some("Review open diffs"),
                ),
                None,
            ),
            (
                "long labels",
                parts(
                    "Synthetic Review Specialist",
                    "an-exceptionally-long-project-directory",
                    Some(long_session),
                ),
                None,
            ),
            (
                "wide labels",
                parts(
                    "合成エージェント名前",
                    "日本語のとても長いプロジェクト",
                    Some("ログインの不具合を直す"),
                ),
                None,
            ),
            ("no project", parts("Pi", "", Some("Tidy the README")), None),
        ];
        let rendered = scenarios
            .iter()
            .map(|(name, parts, spinner)| format!("{name:<16}| {}", compose_title(parts, *spinner)))
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!(rendered);
    }
}
