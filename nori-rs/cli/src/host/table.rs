//! Flat host listing with terminal-cell budgets; piped output retains complete values.

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
    for row in listing["sessions"].as_array().into_iter().flatten() {
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
        let title = clean("title");
        let title = match row["parent"].as_str() {
            Some(_) if row["source"] == "session" => {
                let parent = clean("parent");
                let parent = if width.is_some() {
                    parent.chars().take(8).collect::<String>()
                } else {
                    parent
                };
                format!("↳ (parent {parent}) {title}")
            }
            _ => title,
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
                "waiting_on_you" | "working" | "starting" => status.yellow().to_string(),
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
    fn delegated_child_identity_stays_inside_title_column() {
        let parent = "11111111-1111-1111-1111-111111111111";
        let child = "22222222-2222-2222-2222-222222222222";
        let listing = json!({"sessions":[
            {"session_id":parent,"agent":"mock-model","status":"ready","title":"Parent"},
            {"session_id":child,"agent":"mock-model-alt","status":"waiting_on_you","title":"Child task","parent":parent,"source":"session","depth":1}
        ]});
        let piped = format_table(&listing, None, false);
        let rows = piped.lines().collect::<Vec<_>>();
        assert_eq!(&rows[2][..36], child);
        assert_eq!(&rows[2][38..54], "mock-model-alt  ");
        assert_eq!(&rows[2][56..72], "waiting on you  ");
        let title = &rows[2][82..];
        assert!(title.contains("Child task"));
        assert!(
            title.contains(parent),
            "child row must identify its parent: {title}"
        );
        let tty = format_table(&listing, Some(110), false);
        assert!(tty.lines().all(|line| line.width() <= 110));
        assert!(
            tty.lines().nth(2).unwrap()[82..].contains("parent 11111111"),
            "normal terminal width must retain parent identity"
        );
        assert_eq!(&tty.lines().nth(2).unwrap()[..82], &rows[2][..82]);
    }
}
