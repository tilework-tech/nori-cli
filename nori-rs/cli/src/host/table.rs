//! Host session listing with terminal-cell budgets; piped output retains
//! complete values.
//!
//! Children follow their parent. Tree glyphs sit inside TITLE so the ID,
//! AGENT, STATUS, and AGE columns never move.

use owo_colors::OwoColorize;
use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(super) fn format_table(listing: &Value, width: Option<usize>, color: bool) -> String {
    let header = format!(
        "{:<36}  {:<16}  {:<16}  {:>6}  TITLE",
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
    for ((index, _), (guide, _)) in order.into_iter().zip(guides) {
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
        let status = clean("status").replace('_', " ");
        let title = format!("{guide}{}", clean("title"));
        let title = if row["parent"].is_string() && width.is_none() {
            format!("{title}  (parent {})", clean("parent"))
        } else {
            title
        };
        let seconds = row["age_seconds"].as_i64().unwrap_or(0).max(0);
        let age = if seconds < 60 {
            format!("{seconds}s")
        } else if seconds < 3600 {
            format!("{}m", seconds / 60)
        } else {
            format!("{}h", seconds / 3600)
        };
        let prefix = format!("{id:<36}  {agent:<16}  {status:<16}  {age:>6}  ");
        let prefix_width = prefix.width();
        if let Some(width) = width
            && width < prefix_width
        {
            output.push_str(&truncate(&prefix, width));
            output.push('\n');
            continue;
        }
        let title = width.map_or_else(
            || title.clone(),
            |width| truncate(&title, width.saturating_sub(prefix_width)),
        );
        let status = format!("{status:<16}");
        let status = if color {
            match row["status"].as_str().unwrap_or_default() {
                "failed" => status.red().to_string(),
                "waiting_on_you" => status.yellow().to_string(),
                "working" => status.cyan().to_string(),
                "ready" => status.green().to_string(),
                _ => status.dimmed().to_string(),
            }
        } else {
            status
        };
        output.push_str(&format!(
            "{id:<36}  {agent:<16}  {status}  {age:>6}  {title}\n"
        ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn tty_titles_respect_display_columns_and_keep_graphemes_intact() {
        for (title, budget, expected) in [
            ("abcdef", 4, "abc…"),
            ("abcd", 4, "abcd"),
            ("界界界", 4, "界…"),
            ("e\u{301}e\u{301}e\u{301}", 2, "e\u{301}…"),
            ("👩‍💻👩‍💻👩‍💻", 4, "👩‍💻…"),
            ("abcdef", 1, "…"),
            ("abcdef", 0, ""),
        ] {
            let listing = json!({"sessions":[{"session_id":"id", "agent":"agent", "status":"ready", "title":title}]});
            let rendered = format_table(&listing, Some(82 + budget), false);
            let row = rendered.lines().nth(1).unwrap();
            assert_eq!(row.get(82..).unwrap_or(""), expected, "title {title}");
        }
    }

    #[test]
    fn tty_title_budget_accounts_for_expanded_metadata_columns() {
        let listing = json!({"sessions":[{"session_id":"id", "agent":"abcdefghijklmnopqrst", "status":"ready", "title":"long title"}]});
        let rendered = format_table(&listing, Some(90), false);
        assert!(rendered.lines().nth(1).unwrap().ends_with("lon…"));
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
        let rows = tty.lines().skip(1).collect::<Vec<_>>();
        assert_eq!(
            rows.iter().map(|row| &row[..8]).collect::<Vec<_>>(),
            ["11111111", "22222222", "44444444", "55555555", "33333333"],
            "children follow their parent: {tty}"
        );
        assert_eq!(
            rows.iter().map(|row| &row[82..]).collect::<Vec<_>>(),
            [
                "Ship tree picker",
                "├─ Port picker",
                "│  └─ Write snapshot tests",
                "└─ Update docs",
                "Spike transport",
            ],
            "{tty}"
        );
        assert_eq!(&rows[2][38..54], "mock-model      ");
        assert_eq!(&rows[1][56..72], "waiting on you  ");
        let narrow = format_table(&listing, Some(92), false);
        assert!(narrow.lines().all(|line| line.width() <= 92), "{narrow}");
        assert!(
            narrow.lines().nth(3).unwrap()[82..].starts_with("│  └─ "),
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
            child_row[82..].starts_with("├─ Port picker") && child_row.contains(root),
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
}
