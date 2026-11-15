//! Host session listing with terminal-cell budgets; piped output retains
//! complete values.
//!
//! Children follow their parent. Tree glyphs sit inside TITLE so the ID,
//! AGENT, STATUS, and AGE columns never move.

use owo_colors::OwoColorize;
use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Cells for ID, AGENT, STATUS, and AGE. TITLE takes what is left.
#[derive(Clone, Copy)]
struct Columns {
    id: usize,
    agent: usize,
    status: usize,
    age: usize,
}

impl Columns {
    /// The two spaces after every column, TITLE's start included.
    fn prefix(self) -> usize {
        self.id + self.agent + self.status + self.age + 8
    }
}

/// Full IDs when piped or when TITLE still gets [`MIN_TITLE`] cells.
const WIDE: Columns = Columns {
    id: 36,
    agent: 16,
    status: 16,
    age: 6,
};
/// Short IDs, as in the `nori attach` list.
const COMPACT: Columns = Columns {
    id: 8,
    agent: 14,
    status: 16,
    age: 5,
};
/// The narrowest layout; `waiting on you` and ages such as `59m` still fit.
const TIGHT: Columns = Columns {
    id: 8,
    agent: 10,
    status: 14,
    age: 4,
};
const MIN_TITLE: usize = 24;

fn columns_for(width: Option<usize>) -> Columns {
    let Some(width) = width else {
        return WIDE;
    };
    [WIDE, COMPACT]
        .into_iter()
        .find(|columns| width >= columns.prefix() + MIN_TITLE)
        .unwrap_or(TIGHT)
}

pub(super) fn format_table(listing: &Value, width: Option<usize>, color: bool) -> String {
    let columns = columns_for(width);
    let Columns {
        id: id_cells,
        agent: agent_cells,
        status: status_cells,
        age: age_cells,
    } = columns;
    let header = format!(
        "{:<id_cells$}  {:<agent_cells$}  {:<status_cells$}  {:>age_cells$}  TITLE",
        "ID", "AGENT", "STATUS", "AGE"
    );
    let mut output = format!(
        "{}\n",
        width.map_or_else(|| header.clone(), |width| truncate(&header, width))
    );
    let rows: Vec<&Value> = listing["sessions"]
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    let links: Vec<(&str, Option<&str>)> = rows
        .iter()
        .map(|row| {
            (
                row["session_id"].as_str().unwrap_or("-"),
                row["parent"].as_str(),
            )
        })
        .collect();
    let order = nori_tui_components::picker::tree_order(&links);
    let depths: Vec<i32> = order.iter().map(|(_, depth)| *depth).collect();
    let guides = nori_tui_components::picker::tree_guides(&depths);
    for ((index, depth), (guide, _)) in order.into_iter().zip(guides) {
        let row = rows[index];
        let clean = |key: &str| {
            row[key]
                .as_str()
                .unwrap_or("-")
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect::<String>()
        };
        let id = clean("session_id");
        let agent = clean("agent");
        // A top-level session speaks for its family: its row shows the
        // status of the member that most needs the user.
        let status_key = row["family_status"]
            .as_str()
            .filter(|_| depth == 0)
            .unwrap_or_else(|| row["status"].as_str().unwrap_or("-"));
        let status = status_key.replace('_', " ");
        let name = clean("title");
        let title = if row["parent"].is_string() && width.is_none() {
            format!("{guide}{name}  (parent {})", clean("parent"))
        } else {
            format!("{guide}{name}")
        };
        let seconds = row["age_seconds"].as_i64().unwrap_or(0).max(0);
        let age = if seconds < 60 {
            format!("{seconds}s")
        } else if seconds < 3600 {
            format!("{}m", seconds / 60)
        } else {
            format!("{}h", seconds / 3600)
        };
        let (id, agent, status) = if width.is_some() {
            (
                id.chars().take(id_cells).collect(),
                truncate(&agent, agent_cells),
                truncate(&status, status_cells),
            )
        } else {
            (id, agent, status)
        };
        let prefix = format!(
            "{id:<id_cells$}  {agent:<agent_cells$}  {status:<status_cells$}  {age:>age_cells$}  "
        );
        let prefix_width = prefix.width();
        // The tree guide stays whole; the name is cut in the middle so titles
        // that share a long prefix still read apart.
        let title = width.map_or_else(
            || title.clone(),
            |width| {
                let room = width.saturating_sub(prefix_width);
                if room > guide.width() {
                    format!("{guide}{}", truncate_middle(&name, room - guide.width()))
                } else {
                    truncate(&title, room)
                }
            },
        );
        let status = format!("{status:<status_cells$}");
        let status = if color {
            match status_key {
                "failed" => status.red().to_string(),
                "waiting_on_you" => status.yellow().to_string(),
                "working" => status.cyan().to_string(),
                "ready" => status.green().to_string(),
                // A child that finished its task reads apart from one the
                // user closed.
                "done" => status,
                _ => status.dimmed().to_string(),
            }
        } else {
            status
        };
        let line = match width {
            // Below the tight layout, metadata alone overflows: clip plainly.
            Some(width) if width < prefix_width => truncate(&prefix, width),
            _ => format!(
                "{id:<id_cells$}  {agent:<agent_cells$}  {status}  {age:>age_cells$}  {title}"
            ),
        };
        output.push_str(line.trim_end());
        output.push('\n');
    }
    output
}

fn truncate(value: &str, width: usize) -> String {
    if value.width() <= width {
        return value.into();
    }
    if width == 0 {
        return String::new();
    }
    let mut end = 0;
    for (index, grapheme) in value.grapheme_indices(true) {
        let next = index + grapheme.len();
        if value[..next].width() > width - 1 {
            break;
        }
        end = next;
    }
    format!("{}…", &value[..end])
}

/// `value` in at most `width` cells, cut in the middle with `…` on grapheme
/// boundaries.
fn truncate_middle(value: &str, width: usize) -> String {
    if value.width() <= width {
        return value.into();
    }
    if width == 0 {
        return String::new();
    }
    let room = width - 1;
    let graphemes: Vec<&str> = value.graphemes(true).collect();
    let mut head = String::new();
    for grapheme in &graphemes {
        if head.width() + grapheme.width() > room.div_ceil(2) {
            break;
        }
        head.push_str(grapheme);
    }
    let mut tail: Vec<&str> = Vec::new();
    for grapheme in graphemes.iter().rev() {
        if head.width() + tail.concat().width() + grapheme.width() > room {
            break;
        }
        tail.push(grapheme);
    }
    tail.reverse();
    format!("{}…{}", head.trim_end(), tail.concat().trim_start())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    /// The display column where `name` starts in the header row.
    fn column(rendered: &str, name: &str) -> usize {
        let header = rendered.lines().next().unwrap_or_default();
        let padded = format!(" {header} ");
        let at = padded
            .find(&format!(" {name} "))
            .unwrap_or_else(|| panic!("{name} column in {rendered}"));
        padded[..at].width()
    }

    /// The text of `row` from display column `from` on.
    fn from_column(row: &str, from: usize) -> &str {
        let mut cells = 0;
        for (index, ch) in row.char_indices() {
            if cells >= from {
                return &row[index..];
            }
            cells += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        }
        ""
    }

    fn strip_ansi(text: &str) -> String {
        let mut plain = String::new();
        let mut chars = text.chars();
        while let Some(ch) = chars.next() {
            if ch == '\u{1b}' {
                for code in chars.by_ref() {
                    if code == 'm' {
                        break;
                    }
                }
            } else {
                plain.push(ch);
            }
        }
        plain
    }

    #[test]
    fn titles_are_cut_by_display_columns_and_keep_graphemes_intact() {
        for (title, budget, expected) in [
            ("abcdef", 4, "abc…"),
            ("abcd", 4, "abcd"),
            ("界界界", 4, "界…"),
            ("e\u{301}e\u{301}e\u{301}", 2, "e\u{301}…"),
            ("👩‍💻👩‍💻👩‍💻", 4, "👩‍💻…"),
            ("abcdef", 1, "…"),
            ("abcdef", 0, ""),
        ] {
            assert_eq!(truncate(title, budget), expected, "title {title}");
        }
    }

    #[test]
    fn a_long_agent_name_is_cut_so_later_columns_stay_put() {
        let listing = json!({"sessions":[
            {"session_id":"11111111-1111-1111-1111-111111111111","agent":"reviewer-long-agent-name-x","status":"ready","title":"Review the parser"},
            {"session_id":"22222222-2222-2222-2222-222222222222","agent":"mock-model","status":"ready","title":"Spike transport"}
        ]});
        for width in [120, 80] {
            let rendered = format_table(&listing, Some(width), false);
            let status = column(&rendered, "STATUS");
            let title = column(&rendered, "TITLE");
            let rows = rendered.lines().skip(1).collect::<Vec<_>>();
            assert!(rows[0].contains("reviewer-long"), "{rendered}");
            assert!(rows[0].contains('…'), "the agent is cut: {rendered}");
            assert!(
                !rows[0].contains("reviewer-long-agent-name-x"),
                "{rendered}"
            );
            for row in &rows {
                assert!(from_column(row, status).starts_with("ready "), "{rendered}");
            }
            assert_eq!(
                from_column(rows[0], title),
                "Review the parser",
                "{rendered}"
            );
            assert_eq!(from_column(rows[1], title), "Spike transport", "{rendered}");
        }
        let piped = format_table(&listing, None, false);
        assert!(piped.contains("reviewer-long-agent-name-x"), "{piped}");
    }

    #[test]
    fn an_80_column_terminal_keeps_titles_tree_glyphs_ages_and_colors() {
        let root = "11111111-1111-1111-1111-111111111111";
        let child = "22222222-2222-2222-2222-222222222222";
        let grandchild = "44444444-4444-4444-4444-444444444444";
        let listing = json!({"sessions":[
            {"session_id":root,"agent":"mock-model","status":"ready","family_status":"waiting_on_you","title":"Ship tree picker","age_seconds":754},
            {"session_id":child,"agent":"mock-model-alt","status":"ready","title":"Port picker","parent":root,"source":"session","depth":1,"age_seconds":75},
            {"session_id":grandchild,"agent":"mock-model","status":"waiting_on_you","title":"Write snapshot tests","parent":child,"source":"session","depth":2,"age_seconds":7}
        ]});
        let colored = format_table(&listing, Some(80), true);
        let plain = strip_ansi(&colored);
        assert!(plain.lines().all(|line| line.width() <= 80), "{plain}");
        let title = column(&plain, "TITLE");
        let age = column(&plain, "AGE");
        let rows = plain.lines().skip(1).collect::<Vec<_>>();
        assert_eq!(
            rows.iter()
                .map(|row| from_column(row, title))
                .collect::<Vec<_>>(),
            [
                "Ship tree picker",
                "└─ Port picker",
                "   └─ Write snapshot tests"
            ],
            "{plain}"
        );
        assert_eq!(
            rows.iter()
                .map(|row| from_column(row, age)
                    .split_whitespace()
                    .next()
                    .unwrap_or_default())
                .collect::<Vec<_>>(),
            ["12m", "1m", "7s"],
            "AGE is never cut: {plain}"
        );
        assert!(
            rows[0].starts_with("11111111 "),
            "IDs shorten to fit: {plain}"
        );
        let root_row = colored.lines().nth(1).unwrap_or_default();
        assert!(
            root_row.contains("\u{1b}[33mwaiting on you"),
            "the top-level row takes its family's color: {colored:?}"
        );
    }

    #[test]
    fn a_top_level_session_shows_when_anything_below_it_waits_on_you() {
        let root = "11111111-1111-1111-1111-111111111111";
        let listing = json!({"sessions":[
            {"session_id":root,"agent":"mock-model","status":"ready","family_status":"waiting_on_you","title":"Ship tree picker"},
            {"session_id":"22222222-2222-2222-2222-222222222222","agent":"mock-model","status":"working","family_status":"waiting_on_you","title":"Port picker","parent":root,"source":"session","depth":1},
            {"session_id":"33333333-3333-3333-3333-333333333333","agent":"mock-model","status":"ready","family_status":"ready","title":"Spike transport"}
        ]});
        for width in [Some(120), None] {
            let rendered = format_table(&listing, width, false);
            let status = column(&rendered, "STATUS");
            let rows = rendered.lines().skip(1).collect::<Vec<_>>();
            assert!(
                from_column(rows[0], status).starts_with("waiting on you"),
                "{rendered}"
            );
            assert!(
                from_column(rows[1], status).starts_with("working "),
                "{rendered}"
            );
            assert!(
                from_column(rows[2], status).starts_with("ready "),
                "{rendered}"
            );
        }
    }

    #[test]
    fn piped_table_keeps_complete_plain_titles() {
        let title = "A title much longer than the remaining terminal width 界 e\u{301} 👩‍💻";
        let listing = json!({"sessions":[{"session_id":"id", "agent":"agent", "status":"working", "title":title}]});
        let rendered = format_table(&listing, None, false);
        assert!(rendered.lines().nth(1).unwrap().ends_with(title));
        assert!(!rendered.contains('\u{1b}'));
    }

    #[test]
    fn children_print_under_their_parent_with_tree_glyphs_inside_title() {
        let root = "11111111-1111-1111-1111-111111111111";
        let other = "33333333-3333-3333-3333-333333333333";
        let child = "22222222-2222-2222-2222-222222222222";
        let grandchild = "44444444-4444-4444-4444-444444444444";
        let sibling = "55555555-5555-5555-5555-555555555555";
        let listing = json!({"sessions":[
            {"session_id":root,"agent":"mock-model","status":"working","title":"Ship tree picker"},
            {"session_id":other,"agent":"mock-model","status":"ready","title":"Spike transport"},
            {"session_id":child,"agent":"mock-model-alt","status":"waiting_on_you","title":"Port picker","parent":root,"source":"session","depth":1},
            {"session_id":grandchild,"agent":"mock-model","status":"failed","title":"Write snapshot tests","parent":child,"source":"session","depth":2},
            {"session_id":sibling,"agent":"mock-model","status":"ready","title":"Update docs","parent":root,"source":"session","depth":1}
        ]});
        let tty = format_table(&listing, Some(120), false);
        let (agent_at, status_at, title_at) = (
            column(&tty, "AGENT"),
            column(&tty, "STATUS"),
            column(&tty, "TITLE"),
        );
        let rows = tty.lines().skip(1).collect::<Vec<_>>();
        assert_eq!(
            rows.iter().map(|row| &row[..8]).collect::<Vec<_>>(),
            ["11111111", "22222222", "44444444", "55555555", "33333333"],
            "children follow their parent: {tty}"
        );
        assert_eq!(
            rows.iter()
                .map(|row| from_column(row, title_at))
                .collect::<Vec<_>>(),
            [
                "Ship tree picker",
                "├─ Port picker",
                "│  └─ Write snapshot tests",
                "└─ Update docs",
                "Spike transport",
            ],
            "{tty}"
        );
        assert!(from_column(rows[2], agent_at).starts_with("mock-model "));
        assert!(from_column(rows[1], status_at).starts_with("waiting on you "));
        let narrow = format_table(&listing, Some(92), false);
        assert!(narrow.lines().all(|line| line.width() <= 92), "{narrow}");
        let narrow_title = column(&narrow, "TITLE");
        assert!(
            from_column(narrow.lines().nth(3).unwrap(), narrow_title).starts_with("│  └─ "),
            "glyphs survive truncation: {narrow}"
        );
        let piped = format_table(&listing, None, false);
        let child_row = piped.lines().nth(2).unwrap();
        assert_eq!(&child_row[..36], child);
        let orphan = json!({"sessions":[
            {"session_id":child,"agent":"m","status":"ready","title":"Port picker","parent":root,"source":"session"}
        ]});
        assert!(
            format_table(&orphan, None, false).contains(root),
            "a child whose parent is gone still names it when piped"
        );
        assert!(
            from_column(child_row, column(&piped, "TITLE")).starts_with("├─ Port picker")
                && child_row.contains(root),
            "piped output keeps the complete parent ID: {child_row}"
        );
    }

    #[test]
    fn status_words_are_colored_by_what_the_session_needs() {
        let listing = json!({"sessions":[
            {"session_id":"a","agent":"m","status":"waiting_on_you","title":"a"},
            {"session_id":"b","agent":"m","status":"working","title":"b"},
            {"session_id":"c","agent":"m","status":"ready","title":"c"},
            {"session_id":"d","agent":"m","status":"failed","title":"d"}
        ]});
        let colored = format_table(&listing, Some(120), true);
        let rows = colored.lines().skip(1).collect::<Vec<_>>();
        let codes_before = |row: &str, word: &str| {
            let at = row
                .find(word)
                .unwrap_or_else(|| panic!("{word} in {row:?}"));
            let start = row[..at].rfind("\u{1b}[").expect("styled word");
            row[start + 2..at]
                .trim_end_matches('m')
                .split(';')
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        for (row, word, code) in [
            (rows[0], "waiting on you", "33"),
            (rows[1], "working", "36"),
            (rows[2], "ready", "32"),
            (rows[3], "failed", "31"),
        ] {
            assert!(
                codes_before(row, word).iter().any(|c| c == code),
                "{word} uses color {code}: {colored:?}"
            );
        }
    }

    #[test]
    fn done_reads_apart_from_closed_which_stays_dim() {
        let listing = json!({"sessions":[
            {"session_id":"a","agent":"m","status":"done","title":"a"},
            {"session_id":"b","agent":"m","status":"closed","title":"b"}
        ]});
        let colored = format_table(&listing, Some(120), true);
        let rows = colored.lines().skip(1).collect::<Vec<_>>();
        assert!(rows[0].contains("done"), "{colored:?}");
        assert!(
            !rows[0].contains("\u{1b}[2mdone"),
            "done is not dimmed: {colored:?}"
        );
        assert!(
            rows[1].contains("\u{1b}[2mclosed"),
            "closed stays dim: {colored:?}"
        );
    }

    #[test]
    fn titles_sharing_a_long_prefix_keep_their_ends() {
        let listing = json!({"sessions":[
            {"session_id":"11111111-1111-1111-1111-111111111111","agent":"mock-model","status":"ready",
             "title":"shared prefix for the parser review: parent side and a long tail of words"},
            {"session_id":"22222222-2222-2222-2222-222222222222","agent":"mock-model","status":"ready",
             "title":"shared prefix for the parser review: child side and a long tail of words z"}
        ]});
        let rendered = format_table(&listing, Some(80), false);
        let rows = rendered.lines().skip(1).collect::<Vec<_>>();
        assert!(rows[0].starts_with("11111111"), "{rendered}");
        assert!(rows[0].trim_end().ends_with("of words"), "{rendered}");
        assert!(rows[1].trim_end().ends_with("of words z"), "{rendered}");
        for row in &rows {
            assert!(row.width() <= 80, "{rendered}");
            assert!(row.contains("shared pre"), "{rendered}");
        }
    }

    #[test]
    fn rows_stay_within_a_very_narrow_terminal() {
        let listing = json!({"sessions":[
            {"session_id":"11111111-1111-1111-1111-111111111111","agent":"mock-model","status":"waiting_on_you","title":"Ship tree picker","age_seconds":754}
        ]});
        for width in [20, 40, 50] {
            let rendered = strip_ansi(&format_table(&listing, Some(width), true));
            assert!(
                rendered.lines().all(|line| line.width() <= width),
                "@{width}: {rendered}"
            );
        }
    }
}
