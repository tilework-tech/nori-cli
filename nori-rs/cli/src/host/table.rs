//! Session-list formatting for host terminals and piped output.

use nori_tui_components::session_table;
use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Piped columns: complete IDs, then AGENT, STATUS, AGE, CWD and TITLE.
const ID_CELLS: usize = 36;
const AGENT_CELLS: usize = 16;
const STATUS_CELLS: usize = 16;
const AGE_CELLS: usize = 6;

/// The session table: two-row and coloured on a terminal of `width` cells,
/// one plain row per session with complete values when piped.
pub(super) fn format_table(
    listing: &Value,
    width: Option<usize>,
    color: bool,
    shade: Option<(u8, u8, u8)>,
) -> String {
    if let Some(width) = width {
        return terminal_table(listing, width, color, shade);
    }
    let rows: Vec<&Value> = listing["sessions"]
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    let cwd_cells = rows
        .iter()
        .map(|row| row["cwd"].as_str().map_or(1, UnicodeWidthStr::width))
        .max()
        .unwrap_or(0)
        .max("CWD".len());
    let mut output = format!(
        "{:<ID_CELLS$}  {:<AGENT_CELLS$}  {:<STATUS_CELLS$}  {:>AGE_CELLS$}  {:<cwd_cells$}  TITLE\n",
        "ID", "AGENT", "STATUS", "AGE", "CWD"
    );
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
        let status = row["family_status"]
            .as_str()
            .filter(|_| depth == 0)
            .unwrap_or_else(|| row["status"].as_str().unwrap_or("-"))
            .chars()
            .map(|c| if c == '_' || c.is_control() { ' ' } else { c })
            .collect::<String>();
        let name = clean("title");
        let title = if row["parent"].is_string() {
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
        let cwd = clean("cwd");
        let gap = " ".repeat(cwd_cells.saturating_sub(cwd.width()));
        let line = format!(
            "{id:<ID_CELLS$}  {agent:<AGENT_CELLS$}  {status:<STATUS_CELLS$}  {age:>AGE_CELLS$}  {cwd}{gap}  {title}"
        );
        output.push_str(line.trim_end());
        output.push('\n');
    }
    output
}

/// One styled run of text on a terminal row.
struct Segment {
    text: String,
    /// SGR codes, such as `33` or `2`, applied to this run only.
    codes: &'static str,
}

fn segment(text: impl Into<String>, codes: &'static str) -> Segment {
    Segment {
        text: text.into(),
        codes,
    }
}

/// `value` padded with spaces to `cells` display cells.
fn pad(value: &str, cells: usize) -> String {
    format!("{value}{}", " ".repeat(cells.saturating_sub(value.width())))
}

fn status_codes(status: &str) -> &'static str {
    match status {
        "failed" => "31",
        "waiting_on_you" => "33",
        "working" => "36",
        "ready" => "32",
        // A child that finished its task reads apart from one the user
        // closed.
        "done" => "",
        _ => "2",
    }
}

/// The segments of one row, clipped to `width` cells. With `color`, each
/// segment gets its codes, and `shade` paints the whole row's background.
fn paint(segments: &[Segment], width: usize, color: bool, shade: Option<(u8, u8, u8)>) -> String {
    let background = shade
        .filter(|_| color)
        .map(|(red, green, blue)| format!("\u{1b}[48;2;{red};{green};{blue}m"));
    let mut line = String::new();
    let mut used = 0;
    for Segment { text, codes } in segments {
        let room = width.saturating_sub(used);
        if room == 0 {
            break;
        }
        let text = if text.width() > room {
            truncate(text, room)
        } else {
            text.clone()
        };
        used += text.width();
        match (&background, color && !codes.is_empty()) {
            (Some(background), true) => {
                line.push_str(&format!("{background}\u{1b}[{codes}m{text}\u{1b}[0m"));
            }
            (Some(background), false) => line.push_str(&format!("{background}{text}\u{1b}[0m")),
            (None, true) => line.push_str(&format!("\u{1b}[{codes}m{text}\u{1b}[0m")),
            (None, false) => line.push_str(&text),
        }
    }
    match &background {
        // A shaded row reaches the right edge.
        Some(background) if used < width => {
            line.push_str(&format!(
                "{background}{}\u{1b}[0m",
                " ".repeat(width - used)
            ));
            line
        }
        _ => line.trim_end().to_string(),
    }
}

fn terminal_table(
    listing: &Value,
    width: usize,
    color: bool,
    shade: Option<(u8, u8, u8)>,
) -> String {
    // Rows line up with the session lists, whose focus rails take a cell
    // at each side.
    let layout = session_table::session_widths(
        i32::try_from(width)
            .unwrap_or(i32::MAX)
            .saturating_sub(session_table::RAIL_CELLS),
    );
    let cells = |value: i32| usize::try_from(value).unwrap_or(0);
    // The Session column holds the status icon and a space before the title.
    let session_cells = cells(layout.session) + 2;
    let agent_cells = layout.agent.map(cells);
    let status_cells = cells(layout.status);
    let context_cells = layout.context.map(cells);
    let mut header = vec![
        segment("   ", ""),
        segment(pad("Session", session_cells), ""),
    ];
    if let Some(agent_cells) = agent_cells {
        header.push(segment(format!("  {}", pad("Agent", agent_cells)), ""));
    }
    header.push(segment(format!("  {}", pad("Status", status_cells)), ""));
    let mut output = paint(&header, width, false, None);
    output.push('\n');
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
    let home = dirs::home_dir();
    for ((index, depth), (guide, continuation)) in order.into_iter().zip(guides) {
        let row = rows[index];
        let clean = |key: &str| {
            row[key].as_str().map(|value| {
                value
                    .chars()
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect::<String>()
            })
        };
        // A top-level session speaks for its family: its row shows the
        // status of the member that most needs the user.
        let status_key: String = row["family_status"]
            .as_str()
            .filter(|_| depth == 0)
            .unwrap_or_else(|| row["status"].as_str().unwrap_or("-"))
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        let status_key = status_key.as_str();
        let status: nori_protocol::SessionStatus =
            serde_json::from_value(Value::String(status_key.to_string()))
                .unwrap_or(nori_protocol::SessionStatus::Starting);
        let title = clean("title").unwrap_or_else(|| nori_protocol::UNTITLED_SESSION_NAME.into());
        let icon = format!("{} ", nori_tui::session_status_glyph(status));
        // Cut the icon and title together, as the session lists' picker
        // does, so every list cuts a title at the same point.
        let room = session_cells.saturating_sub(guide.width());
        let cell = nori_tui_components::text::truncate_middle(&format!("{icon}{title}"), room);
        let (icon, title) = match cell.strip_prefix(&icon) {
            Some(title) => (icon.clone(), title.to_string()),
            None => (String::new(), cell),
        };
        let mut first = vec![
            segment("   ", ""),
            segment(guide.clone(), ""),
            segment(icon.clone(), status_codes(status_key)),
            segment(pad(&title, room.saturating_sub(icon.width())), ""),
        ];
        // A session waiting on the user shows what it asks for.
        let reply = clean("waiting_request")
            .filter(|_| row["status"] == "waiting_on_you")
            .or_else(|| clean("reply_preview"))
            .unwrap_or_else(|| session_table::NO_REPLY.into());
        let reply = session_table::folder_and_reply(
            clean("cwd").as_deref().map(std::path::Path::new),
            home.as_deref(),
            &reply,
        );
        let reply_room = session_cells.saturating_sub(continuation.width() + 2);
        let mut second = vec![
            segment("   ", ""),
            segment(continuation.clone(), ""),
            segment(
                format!("  {}", pad(&truncate(&reply, reply_room), reply_room)),
                "2",
            ),
        ];
        if let Some(agent_cells) = agent_cells {
            let agent = clean("agent").unwrap_or_default();
            first.push(segment(
                format!("  {}", pad(&truncate(&agent, agent_cells), agent_cells)),
                "",
            ));
            let model = clean("model").unwrap_or_default();
            second.push(segment(
                format!(
                    "  {}",
                    pad(
                        &nori_tui_components::text::truncate_middle(&model, agent_cells),
                        agent_cells
                    )
                ),
                "2",
            ));
        }
        first.push(segment("  ", ""));
        first.push(segment(
            pad(
                &truncate(&status_key.replace('_', " "), status_cells),
                status_cells,
            ),
            status_codes(status_key),
        ));
        let ago = row["idle_seconds"]
            .as_i64()
            .map(session_table::ago)
            .unwrap_or_default();
        second.push(segment(format!("  {}", pad(&ago, status_cells)), "2"));
        if let Some(context_cells) = context_cells {
            let context = row["context_used"]
                .as_i64()
                .zip(row["context_size"].as_i64())
                .and_then(|(used, size)| session_table::context_from_usage(used, size))
                .unwrap_or_default();
            first.push(segment(
                format!(
                    "  {}",
                    pad(&truncate(&context, context_cells), context_cells)
                ),
                "2",
            ));
        }
        // Lines alternate: each session's first line is shaded and its
        // second keeps the terminal's own background.
        output.push_str(&paint(&first, width, color, shade));
        output.push('\n');
        output.push_str(&paint(&second, width, color, None));
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
    fn piped_table_keeps_complete_plain_titles() {
        let title = "A title much longer than the remaining terminal width 界 e\u{301} 👩‍💻";
        let listing = json!({"sessions":[{"session_id":"id", "agent":"agent", "status":"working", "title":title}]});
        let rendered = format_table(&listing, None, false, None);
        assert!(rendered.lines().nth(1).unwrap().ends_with(title));
        assert!(!rendered.contains('\u{1b}'));
    }

    #[test]
    fn piped_children_follow_their_parent_with_full_ids() {
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
        let piped = format_table(&listing, None, false, None);
        let child_row = piped.lines().nth(2).unwrap();
        assert_eq!(&child_row[..36], child);
        let orphan = json!({"sessions":[
            {"session_id":child,"agent":"m","status":"ready","title":"Port picker","parent":root,"source":"session"}
        ]});
        assert!(
            format_table(&orphan, None, false, None).contains(root),
            "a child whose parent is gone still names it when piped"
        );
        assert!(
            from_column(child_row, column(&piped, "TITLE")).starts_with("├─ Port picker")
                && child_row.contains(root),
            "piped output keeps the complete parent ID: {child_row}"
        );
    }

    const BLACK: (u8, u8, u8) = (0, 0, 0);

    fn sessions() -> Value {
        let root = "11111111-1111-1111-1111-111111111111";
        let child = "22222222-2222-2222-2222-222222222222";
        json!({"sessions":[
            {"session_id":root,"agent":"claude-code","status":"working","family_status":"waiting_on_you",
             "title":"Greeting in three words","cwd":"/work/greeter","reply_preview":"Hi there, friend! Anything else you would like me to help with today?",
             "model":"opus 5.5","idle_seconds":125,"context_used":38000,"context_size":475000},
            {"session_id":child,"agent":"codex","status":"waiting_on_you","title":"inspect the parser recovery path",
             "parent":root,"source":"session","depth":1,"cwd":"/work/greeter","reply_preview":"Reading src/parser.rs","model":"gpt-5.5",
             "idle_seconds":4,"context_used":84320,"context_size":272000},
            {"session_id":"33333333-3333-3333-3333-333333333333","agent":"gemini","status":"starting",
             "title":"say hi in 3 words","idle_seconds":0}
        ]})
    }

    #[test]
    fn piped_rows_align_titles_after_full_folders_and_tolerate_older_hosts() {
        let listing = json!({"sessions":[
            {"session_id":"one","agent":"mock-model","status":"ready","title":"Fix the parser","cwd":"/work/parser-service"},
            {"session_id":"two","agent":"mock-model","status":"ready","title":"From an older host"}
        ]});
        let piped = format_table(&listing, None, false, None);
        let title = column(&piped, "TITLE");
        let rows: Vec<&str> = piped.lines().skip(1).collect();
        assert!(rows[0].contains("/work/parser-service"), "{piped}");
        assert_eq!(from_column(rows[0], title), "Fix the parser");
        assert_eq!(from_column(rows[1], title), "From an older host");
    }

    #[test]
    fn a_terminal_gets_two_rows_per_session_at_each_layout_width() {
        for width in [60, 80, 89, 120] {
            insta::assert_snapshot!(
                format!("host_ls_two_rows_{width}"),
                format_table(&sessions(), Some(width), false, None)
            );
        }
    }

    /// Display column where `needle` starts in `line`.
    fn cell_of(line: &str, needle: &str) -> Option<usize> {
        line.find(needle).map(|at| line[..at].width())
    }

    /// The Session cell of the first session as the shared picker draws it
    /// at `width`.
    fn picker_session_cell(glyph: &str, title: &str, width: u16) -> String {
        use nori_tui_components::picker::Picker;
        use nori_tui_components::picker::PickerItem;
        use nori_tui_components::picker::PickerState;
        use nori_tui_components::session_table;
        let item = PickerItem::new(
            "one".to_string(),
            session_table::SESSION,
            format!("{glyph} {title}"),
        )
        .cell(session_table::AGENT, "mock-model")
        .cell(session_table::STATUS, "ready");
        let state = PickerState::new("Sessions", session_table::session_columns(), [item]);
        let area = ratatui::layout::Rect::new(0, 0, width, 6);
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        ratatui::widgets::Widget::render(
            Picker::new(&state)
                .density(nori_tui_components::picker::PickerDensity::Compact)
                .show_title(false)
                .show_details(false)
                .horizontal_margin(0),
            area,
            &mut buffer,
        );
        let line = (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .find(|line| line.contains(glyph))
            .unwrap_or_default();
        session_cell(&line)
    }

    /// The Session cell of a table row: from the icon to the gap before the
    /// next column.
    fn session_cell(row: &str) -> String {
        let cell = from_column(row, 3);
        cell.split("  ").next().unwrap_or_default().to_string()
    }

    #[test]
    fn long_titles_are_cut_where_the_session_lists_cut_them() {
        let title = "why does the parser drop tokens after a missing brace? find the owners and propose a fix";
        let glyph = nori_tui::session_status_glyph(nori_protocol::SessionStatus::Ready);
        let listing = json!({"sessions":[
            {"session_id":"one","agent":"mock-model","status":"ready","title":title}
        ]});
        for width in [80, 120] {
            let rendered = format_table(&listing, Some(width), false, None);
            let row = rendered.lines().nth(1).unwrap_or_default();
            let table_cell = session_cell(row);
            assert!(
                table_cell.contains('…'),
                "the title is cut at {width}: {row}"
            );
            assert_eq!(
                table_cell,
                picker_session_cell(glyph, title, u16::try_from(width).unwrap_or(80)),
                "width {width}"
            );
        }
    }

    #[test]
    fn a_waiting_rows_second_line_says_what_it_asks_for() {
        let listing = json!({"sessions":[
            {"session_id":"asking","agent":"mock-model","status":"waiting_on_you","title":"Fix the parser",
             "reply_preview":"Let me run the tests first.","waiting_request":"approve: cargo test -p parser"},
            {"session_id":"answering","agent":"mock-model","status":"ready","title":"Spike transport",
             "reply_preview":"The transport spike is done.","waiting_request":"approve: stale"}
        ]});
        let rendered = format_table(&listing, Some(120), false, None);
        let below = |title: &str| {
            let lines: Vec<&str> = rendered.lines().collect();
            let at = lines
                .iter()
                .position(|line| line.contains(title))
                .unwrap_or_else(|| panic!("{title} in {rendered}"));
            lines[at + 1].to_string()
        };
        assert!(
            below("Fix the parser").contains("approve: cargo test -p parser"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("Let me run the tests first."),
            "{rendered}"
        );
        assert!(
            below("Spike transport").contains("The transport spike is done."),
            "{rendered}"
        );
        assert!(
            !rendered.contains("approve: stale"),
            "only a waiting row shows its request"
        );
    }

    #[test]
    fn context_text_is_never_cut() {
        let listing = json!({"sessions":[
            {"session_id":"one","agent":"mock-model","status":"ready","title":"Full",
             "context_used":1_050_000,"context_size":1_050_000}
        ]});
        for width in [89, 120] {
            let rendered = format_table(&listing, Some(width), false, None);
            let row = rendered.lines().nth(1).unwrap_or_default();
            assert!(row.trim_end().ends_with("100% / 1.1M"), "{rendered}");
        }
    }

    #[test]
    fn terminal_columns_sit_at_the_session_list_offsets() {
        // Terminal width, then where Agent, Status and the context value start.
        for (width, agent, status, context) in [
            (59, None, 44, None),
            (60, Some(31), 45, None),
            (80, Some(51), 65, None),
            (88, Some(59), 73, None),
            (89, Some(47), 61, Some(77)),
            (120, Some(78), 92, Some(108)),
        ] {
            let rendered = format_table(&sessions(), Some(width), false, None);
            let header = rendered.lines().next().unwrap_or_default();
            assert_eq!(cell_of(header, "Session"), Some(3), "{rendered}");
            assert_eq!(cell_of(header, "Agent"), agent, "{rendered}");
            assert_eq!(cell_of(header, "Status"), Some(status), "{rendered}");
            let root = rendered.lines().nth(1).unwrap_or_default();
            assert_eq!(cell_of(root, "8% / 475k"), context, "{rendered}");
            assert!(
                rendered.lines().all(|line| line.width() <= width),
                "{rendered}"
            );
        }
    }

    #[test]
    fn piped_rows_keep_full_agents_and_family_status() {
        let root = "11111111-1111-1111-1111-111111111111";
        let listing = json!({"sessions":[
            {"session_id":root,"agent":"reviewer-long-agent-name-x","status":"ready","family_status":"waiting_on_you","title":"Ship tree picker"},
            {"session_id":"22222222-2222-2222-2222-222222222222","agent":"mock-model","status":"working","title":"Port picker","parent":root,"source":"session","depth":1}
        ]});
        let piped = format_table(&listing, None, false, None);
        let rows = piped.lines().skip(1).collect::<Vec<_>>();
        assert!(rows[0].contains("reviewer-long-agent-name-x"), "{piped}");
        assert!(rows[0].contains("  waiting on you  "), "{piped}");
        assert!(rows[1].contains("  working  "), "{piped}");
    }

    #[test]
    fn the_second_row_holds_reply_model_and_last_activity() {
        let rendered = format_table(&sessions(), Some(120), false, None);
        let rows = rendered.lines().skip(1).collect::<Vec<_>>();
        assert!(rows[0].contains("● Greeting in three words"), "{rendered}");
        assert!(rows[1].contains("Hi there, friend!"), "{rendered}");
        assert!(
            rows[1].contains("opus 5.5") && rows[1].contains("2m ago"),
            "{rendered}"
        );
        assert!(
            rows[2].contains("└─ ● inspect the parser recovery path"),
            "{rendered}"
        );
        assert!(
            rows[5].contains("no reply yet") && rows[5].contains("0s ago"),
            "{rendered}"
        );
        let status_at = rendered
            .lines()
            .next()
            .unwrap_or_default()
            .find("Status")
            .unwrap_or(0);
        assert!(
            from_column(rows[0], status_at).starts_with("waiting on you"),
            "a top-level row speaks for its family: {rendered}"
        );
    }

    #[test]
    fn a_long_reply_and_title_stay_in_the_session_column() {
        let listing = json!({"sessions":[
            {"session_id":"a","agent":"reviewer-long-agent-name-x","status":"ready",
             "title":"shared prefix for the parser review: parent side and a long tail of words",
             "reply_preview":"The reply goes on for a very long time and should never reach the agent column at all",
             "model":"claude-opus-5-5-with-a-long-model-name"}
        ]});
        let rendered = format_table(&listing, Some(80), false, None);
        let agent_at =
            cell_of(rendered.lines().next().unwrap_or_default(), "Agent").expect("Agent column");
        let rows = rendered.lines().skip(1).collect::<Vec<_>>();
        let session_part = |row: &str| row.chars().take(agent_at).collect::<String>();
        assert!(
            session_part(rows[0]).trim_end().ends_with("of words"),
            "{rendered}"
        );
        assert!(session_part(rows[0]).contains("shared pre"), "{rendered}");
        assert!(
            session_part(rows[1]).contains("The reply goes on"),
            "{rendered}"
        );
        assert!(
            session_part(rows[1]).trim_end().ends_with('…'),
            "{rendered}"
        );
        assert!(
            from_column(rows[0], agent_at).starts_with("reviewer-lo…"),
            "{rendered}"
        );
        let model = from_column(rows[1], agent_at);
        assert!(
            model.starts_with("claude") && model.contains('…'),
            "{rendered}"
        );
    }

    /// The SGR codes of the last escape before `word` in `row`.
    fn codes_before(row: &str, word: &str) -> Vec<String> {
        let at = row
            .find(word)
            .unwrap_or_else(|| panic!("{word} in {row:?}"));
        let start = row[..at]
            .rfind("\u{1b}[")
            .unwrap_or_else(|| panic!("{word} is styled in {row:?}"));
        let codes = &row[start + 2..at];
        codes[..codes.find('m').unwrap_or(codes.len())]
            .split(';')
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn status_words_are_colored_by_what_the_session_needs() {
        let listing = json!({"sessions":[
            {"session_id":"a","agent":"m","status":"waiting_on_you","title":"a"},
            {"session_id":"b","agent":"m","status":"working","title":"b"},
            {"session_id":"c","agent":"m","status":"ready","title":"c"},
            {"session_id":"d","agent":"m","status":"failed","title":"d"},
            {"session_id":"e","agent":"m","status":"done","title":"e"},
            {"session_id":"f","agent":"m","status":"closed","title":"f"}
        ]});
        let colored = format_table(&listing, Some(120), true, None);
        let rows = colored.lines().skip(1).step_by(2).collect::<Vec<_>>();
        for (row, word, code) in [
            (rows[0], "waiting on you", "33"),
            (rows[1], "working", "36"),
            (rows[2], "ready", "32"),
            (rows[3], "failed", "31"),
            (rows[5], "closed", "2"),
        ] {
            assert!(
                codes_before(row, word).iter().any(|c| c == code),
                "{word} uses {code}: {colored:?}"
            );
        }
        let done_at = rows[4].find("done").expect("done row");
        assert!(
            rows[4][..done_at].ends_with("  "),
            "done is written plain, not dimmed: {colored:?}"
        );
    }

    #[test]
    fn second_rows_are_faint() {
        let colored = format_table(&sessions(), Some(120), true, None);
        let second = colored.lines().nth(2).unwrap_or_default();
        assert!(
            codes_before(second, "Hi there").iter().any(|c| c == "2"),
            "{colored:?}"
        );
        assert!(
            codes_before(second, "2m ago").iter().any(|c| c == "2"),
            "{colored:?}"
        );
        assert!(
            codes_before(second, "opus 5.5").iter().any(|c| c == "2"),
            "{colored:?}"
        );
    }

    #[test]
    fn session_lines_alternate_between_shaded_and_the_terminal_background() {
        let colored = format_table(&sessions(), Some(120), true, Some((9, 8, 7)));
        let rows = colored.lines().skip(1).collect::<Vec<_>>();
        let shaded = |row: &str| row.contains("\u{1b}[48;2;9;8;7m");
        assert_eq!(
            rows.iter().map(|row| shaded(row)).collect::<Vec<_>>(),
            [true, false, true, false, true, false],
            "{colored:?}"
        );
        assert!(
            rows.iter()
                .step_by(2)
                .all(|row| strip_ansi(row).width() == 120 && row.ends_with("\u{1b}[0m")),
            "shaded lines reach the right edge: {colored:?}"
        );
        let uncoloured = format_table(&sessions(), Some(120), false, Some((9, 8, 7)));
        assert!(
            !uncoloured.contains("\u{1b}[48;"),
            "no shade without colour: {uncoloured:?}"
        );
        assert!(
            rows.iter()
                .skip(1)
                .step_by(2)
                .all(|row| !row.contains("\u{1b}[48;")),
            "unshaded lines set no background: {colored:?}"
        );
        let plain = format_table(&sessions(), Some(120), true, None);
        assert!(
            !plain.contains("\u{1b}[48;"),
            "no shade without a known background"
        );
    }

    #[test]
    fn rows_stay_within_a_very_narrow_terminal() {
        for width in [20, 40, 50] {
            let rendered = strip_ansi(&format_table(&sessions(), Some(width), true, Some(BLACK)));
            assert!(
                rendered.lines().all(|line| line.width() <= width),
                "@{width}: {rendered}"
            );
        }
    }
}
