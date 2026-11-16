//! One layout for every list of sessions: the local session list, the
//! `nori attach` list, and `nori host ls` on a terminal.
//!
//! Each session takes two rows. Row 1 holds the status icon and title, the
//! agent, the status word and a header-less context column; row 2 holds the
//! start of the agent's last reply, the model and the time since the last
//! activity. Columns are separated by two spaces. A 2-cell marker and the
//! 2-cell status icon come first, then:
//!
//! | Column  | Width                          |
//! |---------|--------------------------------|
//! | Session | at least 24, takes extra width |
//! | Agent   | 12, hidden below 58 columns    |
//! | Status  | 14                             |
//! | Context | 11, shown from 87 columns      |
//!
//! From 58 to 86 columns every extra cell goes to Session; at 87 Session has
//! 40 and Context appears; beyond that Session takes the rest again.

use crate::PickerColumn;
use crate::PickerColumnWidth;

pub const SESSION: &str = "session";
pub const AGENT: &str = "agent";
pub const STATUS: &str = "status";
pub const CONTEXT: &str = "context";

/// Second-row text for a session the agent has not answered yet.
pub const NO_REPLY: &str = "no reply yet";

/// Cells for the marker and the status icon in front of the title.
pub const LEAD_CELLS: i32 = 4;
pub const MIN_SESSION_CELLS: i32 = 24;
pub const AGENT_CELLS: i32 = 12;
pub const STATUS_CELLS: i32 = 14;
pub const CONTEXT_CELLS: i32 = 11;
/// Narrowest terminal that still shows the Agent column.
pub const AGENT_FROM: i32 = 58;
/// Narrowest terminal that shows the Context column.
pub const CONTEXT_FROM: i32 = 87;
const GAP: i32 = 2;

/// Column widths for one terminal width. `session` excludes the status icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionWidths {
    pub session: i32,
    pub agent: Option<i32>,
    pub status: i32,
    pub context: Option<i32>,
}

pub fn session_widths(width: i32) -> SessionWidths {
    let agent = (width >= AGENT_FROM).then_some(AGENT_CELLS);
    let context = (width >= CONTEXT_FROM).then_some(CONTEXT_CELLS);
    let fixed = [agent, Some(STATUS_CELLS), context]
        .into_iter()
        .flatten()
        .map(|cells| cells + GAP)
        .sum::<i32>();
    SessionWidths {
        session: (width - LEAD_CELLS - fixed).max(MIN_SESSION_CELLS),
        agent,
        status: STATUS_CELLS,
        context,
    }
}

/// Picker columns for this layout. The Session cell starts with the status
/// icon and a space, so its column is two cells wider than the title room.
pub fn session_columns() -> Vec<PickerColumn> {
    let cells = |value: i32| u16::try_from(value).unwrap_or(0);
    let icon = LEAD_CELLS - 2;
    vec![
        PickerColumn::flexible(SESSION, "Session")
            .width(PickerColumnWidth::Flexible {
                min: cells(MIN_SESSION_CELLS + icon),
                max: u16::MAX,
                weight: 1,
            })
            .cut_middle(),
        PickerColumn::fixed(AGENT, "Agent", cells(AGENT_CELLS))
            .hide_below(cells(AGENT_FROM))
            .second_row_cut_middle(),
        PickerColumn::fixed(STATUS, "Status", cells(STATUS_CELLS)),
        PickerColumn::fixed(CONTEXT, "", cells(CONTEXT_CELLS)).hide_below(cells(CONTEXT_FROM)),
    ]
}

/// Longest home-relative form a list shows before falling back to the
/// folder's own name.
const SHORT_FOLDER_CELLS: usize = 20;

/// A session's folder as the lists show it: `~` or `~/code/app` when short,
/// otherwise the folder's own name.
pub fn short_folder(path: &std::path::Path, home: Option<&std::path::Path>) -> String {
    if let Some(relative) = home.and_then(|home| path.strip_prefix(home).ok()) {
        let short = if relative.as_os_str().is_empty() {
            "~".to_string()
        } else {
            format!("~/{}", relative.display())
        };
        if short.chars().count() <= SHORT_FOLDER_CELLS {
            return short;
        }
    }
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// A list's second row text: the session's short folder, when known, then
/// its reply, if any.
pub fn folder_and_reply(
    cwd: Option<&std::path::Path>,
    home: Option<&std::path::Path>,
    reply: &str,
) -> String {
    match cwd {
        Some(cwd) if reply.is_empty() => short_folder(cwd, home),
        Some(cwd) => format!("{} · {reply}", short_folder(cwd, home)),
        None => reply.to_string(),
    }
}

/// Time since a session's last activity, such as `2m ago`.
pub fn ago(seconds: i64) -> String {
    let seconds = seconds.max(0);
    match seconds {
        0..60 => format!("{seconds}s ago"),
        60..3_600 => format!("{}m ago", seconds / 60),
        3_600..86_400 => format!("{}h ago", seconds / 3_600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

/// Context use in the status bar's short form, such as `8% / 475k`.
pub fn context_text(
    percent: Option<i64>,
    window_tokens: Option<i64>,
    used_tokens: Option<i64>,
) -> Option<String> {
    context_with(percent, window_tokens, used_tokens, compact_tokens)
}

fn context_with(
    percent: Option<i64>,
    window_tokens: Option<i64>,
    used_tokens: Option<i64>,
    tokens: fn(i64) -> String,
) -> Option<String> {
    let window = window_tokens.filter(|&count| count > 0).map(tokens);
    let used = used_tokens.filter(|&count| count > 0).map(tokens);
    match (percent.map(|percent| percent.clamp(0, 100)), window, used) {
        (Some(percent), Some(window), _) => Some(format!("{percent}% / {window}")),
        (Some(percent), None, _) => Some(format!("{percent}%")),
        (None, _, Some(used)) => Some(used),
        (None, _, None) => None,
    }
}

/// [`context_text`] for the Context column, which it always fits: when the
/// status bar's form is too wide, token counts keep at most one decimal, so
/// `100% / 1.05M` reads `100% / 1.1M`.
pub fn context_cell(
    percent: Option<i64>,
    window_tokens: Option<i64>,
    used_tokens: Option<i64>,
) -> Option<String> {
    let full = context_text(percent, window_tokens, used_tokens)?;
    if unicode_width::UnicodeWidthStr::width(full.as_str()) <= CONTEXT_CELLS as usize {
        return Some(full);
    }
    context_with(percent, window_tokens, used_tokens, short_tokens)
}

/// Token counts with at most one decimal, such as `8.5k` or `1.1M`.
fn short_tokens(value: i64) -> String {
    let value = value.max(0);
    if value < 1_000 {
        return value.to_string();
    }
    for (scale, suffix) in [(1_000_i64, "k"), (1_000_000, "M"), (1_000_000_000, "G")] {
        let scaled = value as f64 / scale as f64;
        let digits = if scaled < 9.95 {
            format!("{scaled:.1}")
        } else if scaled < 999.5 {
            format!("{scaled:.0}")
        } else {
            continue;
        };
        return format!("{}{suffix}", digits.trim_end_matches(".0"));
    }
    format!("{}G", value / 1_000_000_000)
}

/// [`context_cell`] from an agent's usage report: tokens in context and the
/// context window size.
pub fn context_from_usage(used: i64, size: i64) -> Option<String> {
    (size > 0).then(|| {
        let percent = used.saturating_mul(100).saturating_div(size);
        context_cell(Some(percent), Some(size), Some(used))
    })?
}

/// Token counts with a `k`, `M` or `G` suffix and three significant digits,
/// such as `8.50k`, `475k` or `1.00M`.
pub fn compact_tokens(value: i64) -> String {
    let value = value.max(0);
    if value < 1_000 {
        return value.to_string();
    }
    for (scale, suffix) in [(1_000_i64, "k"), (1_000_000, "M"), (1_000_000_000, "G")] {
        let scaled = value as f64 / scale as f64;
        let rounded = if scaled < 10.0 {
            format!("{scaled:.2}")
        } else if scaled < 100.0 {
            format!("{scaled:.1}")
        } else if scaled < 999.5 {
            format!("{scaled:.0}")
        } else {
            continue;
        };
        return format!("{rounded}{suffix}");
    }
    format!("{}G", value / 1_000_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn widths(session: i32, agent: Option<i32>, context: Option<i32>) -> SessionWidths {
        SessionWidths {
            session,
            agent,
            status: STATUS_CELLS,
            context,
        }
    }

    #[test]
    fn short_folder_names_a_folder_by_home_relative_path_or_basename() {
        use std::path::Path;
        let home = Some(Path::new("/home/ada"));
        assert_eq!(short_folder(Path::new("/home/ada"), home), "~");
        assert_eq!(
            short_folder(Path::new("/home/ada/code/app"), home),
            "~/code/app"
        );
        assert_eq!(
            short_folder(Path::new("/home/ada/code/clients/acme-billing"), home),
            "acme-billing"
        );
        assert_eq!(short_folder(Path::new("/srv/checkouts/api"), home), "api");
        assert_eq!(short_folder(Path::new("/home/adam/api"), home), "api");
        assert_eq!(short_folder(Path::new("/"), home), "/");
        assert_eq!(short_folder(Path::new("/srv/api"), None), "api");
    }

    #[test]
    fn the_session_column_takes_every_extra_cell_until_context_fits() {
        assert_eq!(session_widths(58), widths(24, Some(12), None));
        assert_eq!(session_widths(80), widths(46, Some(12), None));
        assert_eq!(session_widths(86), widths(52, Some(12), None));
    }

    #[test]
    fn context_appears_once_session_has_forty_cells_then_session_grows_again() {
        assert_eq!(session_widths(87), widths(40, Some(12), Some(11)));
        assert_eq!(session_widths(120), widths(73, Some(12), Some(11)));
    }

    #[test]
    fn below_fifty_eight_columns_agent_is_hidden_and_status_stays() {
        assert_eq!(session_widths(57), widths(37, None, None));
        assert_eq!(session_widths(44), widths(24, None, None));
    }

    #[test]
    fn ages_read_in_the_largest_whole_unit() {
        assert_eq!(
            [0, 59, 60, 3599, 3600, 86_400 * 2].map(ago),
            ["0s ago", "59s ago", "1m ago", "59m ago", "1h ago", "2d ago"]
        );
    }

    #[test]
    fn context_always_fits_its_column() {
        assert_eq!(
            context_from_usage(1_050_000, 1_050_000).as_deref(),
            Some("100% / 1.1M")
        );
        assert_eq!(
            context_from_usage(52_500, 1_050_000).as_deref(),
            Some("5% / 1.05M"),
            "a value that fits keeps the status bar's digits"
        );
        for size in [
            999,
            8_500,
            99_950,
            475_000,
            999_600,
            9_960_000,
            1_000_000,
            1_050_000,
            12_340_000,
            999_000_000,
            5_000_000_000,
        ] {
            for used in [0, size / 20, size] {
                let text = context_from_usage(used, size).unwrap_or_default();
                assert!(
                    unicode_width::UnicodeWidthStr::width(text.as_str()) <= CONTEXT_CELLS as usize,
                    "{used} of {size}: {text}"
                );
            }
        }
    }

    #[test]
    fn context_reads_like_the_status_bar() {
        assert_eq!(
            context_from_usage(38_000, 475_000).as_deref(),
            Some("8% / 475k")
        );
        assert_eq!(
            context_text(Some(8), Some(475_000), Some(38_000)).as_deref(),
            Some("8% / 475k")
        );
        assert_eq!(
            context_text(Some(130), Some(1_000_000), None).as_deref(),
            Some("100% / 1.00M")
        );
        assert_eq!(
            context_text(None, None, Some(12_300)).as_deref(),
            Some("12.3k")
        );
        assert_eq!(context_text(None, None, None), None);
        assert_eq!(context_from_usage(0, 0), None);
    }
}
