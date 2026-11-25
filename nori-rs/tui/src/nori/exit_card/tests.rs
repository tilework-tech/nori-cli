use pretty_assertions::assert_eq;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::Line;

use super::layout::VALUE_OFFSET;
use super::*;
use crate::insert_history::insert_history_lines;
use crate::test_backend::VT100Backend;

const STRESS: (ExitToolsFixture, ExitFieldsFixture) =
    (ExitToolsFixture::Many, ExitFieldsFixture::Full);

fn card(tools: ExitToolsFixture, fields: ExitFieldsFixture, width: u16) -> Vec<Line<'static>> {
    exit_card_lines(
        &exit_card_fixture(tools, fields),
        width,
        ExitCardPaint::plain(),
    )
}

/// Print lines into a VT100 terminal through the same history path the
/// session uses, and return every terminal row they occupy.
fn terminal_rows(lines: Vec<Line<'static>>, width: u16) -> Vec<String> {
    let height = u16::try_from(lines.len() * 3 + 4).expect("rows fit");
    let backend = VT100Backend::new(width, height);
    let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");
    term.set_viewport_area(Rect::new(0, height - 1, width, 1));
    insert_history_lines(&mut term, lines).expect("insert card");
    let rows: Vec<String> = term
        .backend()
        .vt100()
        .screen()
        .rows(0, width)
        .map(|row| row.trim_end().to_string())
        .collect();
    let first = rows.iter().position(|row| !row.is_empty()).unwrap_or(0);
    rows[first..rows.len() - 1].to_vec()
}

fn plain(lines: &[Line<'static>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

/// The rows of one labelled block: the label row and its continuation rows.
fn block(rows: &[String], label: &str) -> Vec<String> {
    let start = rows
        .iter()
        .position(|row| row.trim_start().starts_with(label))
        .unwrap_or_else(|| panic!("no {label} row in {rows:#?}"));
    let mut block = vec![rows[start].clone()];
    block.extend(
        rows[start + 1..]
            .iter()
            .take_while(|row| row.starts_with(&" ".repeat(VALUE_OFFSET)))
            .cloned(),
    );
    block
}

/// Column just past the first number on a row.
fn number_end(row: &str) -> usize {
    let start = row
        .find(|ch: char| ch.is_ascii_digit())
        .expect("row has a count");
    start
        + row[start..]
            .find(|ch: char| !ch.is_ascii_digit() && ch != ',')
            .unwrap_or(row.len() - start)
}

#[test]
fn every_card_line_occupies_exactly_one_terminal_row_at_every_width() {
    for tools in ExitToolsFixture::ALL {
        for fields in ExitFieldsFixture::ALL {
            for width in 1..=120 {
                let lines = card(tools, fields, width);
                let expected = plain(&lines);
                assert_eq!(
                    terminal_rows(lines, width),
                    expected,
                    "{tools:?}/{fields:?} soft-wrapped at width {width}"
                );
            }
        }
    }
}

#[test]
fn the_card_never_contains_the_resume_command() {
    let text = plain(&card(STRESS.0, STRESS.1, 80));

    assert!(
        !text.iter().any(|row| row.contains("nori resume")),
        "{text:#?}"
    );
}

#[test]
fn the_title_is_green_and_the_agent_takes_its_color() {
    let lines = card(ExitToolsFixture::Few, ExitFieldsFixture::Full, 80);
    let style_of = |text: &str| {
        lines
            .iter()
            .flat_map(|line| &line.spans)
            .find(|span| span.content == text)
            .map(|span| span.style.fg)
            .unwrap_or_else(|| panic!("no span {text:?}"))
    };

    assert_eq!(style_of("Redesign the exit card"), Some(Color::Green));
    assert_eq!(
        style_of("Claude Code"),
        agent_style(Some(nori_harness::AgentKind::ClaudeCode)).fg
    );
}

#[test]
fn agents_without_a_brand_color_keep_the_terminal_foreground() {
    use nori_harness::AgentKind;

    assert_eq!(agent_style(Some(AgentKind::Pi)).fg, None);
    assert_eq!(agent_style(None).fg, None);
    assert_eq!(agent_style(Some(AgentKind::Codex)).fg, Some(Color::Gray));
}

#[test]
fn a_session_with_only_an_id_shows_the_heading_and_id() {
    let text = plain(&card(
        ExitToolsFixture::None,
        ExitFieldsFixture::NotResumable,
        80,
    ));

    assert_eq!(
        text,
        vec![
            "  › Nori CLI  session ended".to_string(),
            String::new(),
            "  Session      019a7c3e-5b21-7f4d-9e08-6c2a1d4b8f37".to_string(),
        ]
    );
}

#[test]
fn a_session_without_an_id_shows_only_the_heading() {
    let text = plain(&card(
        ExitToolsFixture::None,
        ExitFieldsFixture::NoSession,
        80,
    ));

    assert_eq!(text, vec!["  › Nori CLI  session ended".to_string()]);
}

#[test]
fn the_session_block_lists_title_and_agent_under_the_id() {
    let text = plain(&card(ExitToolsFixture::Few, ExitFieldsFixture::Full, 80));

    assert_eq!(
        block(&text, "Session"),
        vec![
            "  Session      019a7c3e-5b21-7f4d-9e08-6c2a1d4b8f37".to_string(),
            "               Title  Redesign the exit card".to_string(),
            "               Agent  Claude Code".to_string(),
        ]
    );
}

#[test]
fn tool_counts_are_right_justified_and_names_left_justified() {
    let text = plain(&card(STRESS.0, STRESS.1, 80));
    let tools = block(&text, "Tools");

    assert_eq!(tools.len(), 14, "one tool per row: {tools:#?}");
    assert!(
        tools
            .iter()
            .all(|row| number_end(row) == number_end(&tools[0])),
        "{tools:#?}"
    );
    assert!(
        tools
            .iter()
            .all(|row| row[number_end(row)..].starts_with("  ")
                && !row[number_end(row) + 2..].starts_with(' '))
    );
    assert_eq!(tools[0], "  Tools        37  Read");
    assert_eq!(tools[13], "                1  NotebookEdit");
}

#[test]
fn tools_keep_descending_call_order() {
    let text = plain(&card(STRESS.0, STRESS.1, 80));
    let counts: Vec<i64> = block(&text, "Tools")
        .iter()
        .map(|row| {
            let end = number_end(row);
            let start = row[..end].rfind(' ').map_or(0, |space| space + 1);
            row[start..end].parse().expect("count")
        })
        .collect();

    let mut sorted = counts.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(counts, sorted);
}

#[test]
fn wide_terminals_lay_tools_out_in_columns() {
    let text = plain(&card(ExitToolsFixture::Few, ExitFieldsFixture::Full, 120));

    assert_eq!(
        block(&text, "Tools"),
        vec![
            "  Tools        37  Read   18  Edit    6  Glob".to_string(),
            "               29  Bash   12  Grep    3  Write".to_string(),
        ]
    );
}

#[test]
fn one_long_tool_name_keeps_a_wide_terminal_in_one_column() {
    let text = plain(&card(STRESS.0, STRESS.1, 120));

    assert_eq!(block(&text, "Tools").len(), 14);
}

#[test]
fn a_long_tool_name_wraps_under_its_own_name_column() {
    let text = plain(&card(STRESS.0, STRESS.1, 40));
    let tools = block(&text, "Tools");
    let start = tools
        .iter()
        .position(|row| row.contains("mcp__github__"))
        .expect("long name shown");
    let name_column = tools[start].find("mcp__").expect("name");

    assert_eq!(
        tools[start + 1].find(|ch: char| ch != ' '),
        Some(name_column)
    );
    assert!(
        tools[start].ends_with('_'),
        "breaks after a separator: {:?}",
        tools[start]
    );
}

#[test]
fn a_name_without_separators_is_cut_at_the_column_edge() {
    let text = plain(&card(
        ExitToolsFixture::LongNames,
        ExitFieldsFixture::Minimal,
        40,
    ));
    let tools = block(&text, "Tools");
    let start = tools
        .iter()
        .position(|row| row.contains("Exampletool"))
        .expect("unbreakable name shown");

    assert_eq!(tools[start].len(), 40);
}

#[test]
fn skills_and_subagents_take_one_line_each() {
    let text = plain(&card(STRESS.0, STRESS.1, 80));

    assert_eq!(
        block(&text, "Skills"),
        vec![
            "  Skills       test-driven-development".to_string(),
            "               writing-plans".to_string()
        ]
    );
}

#[test]
fn a_wide_terminal_fits_a_few_skills_on_one_line() {
    let text = plain(&card(STRESS.0, STRESS.1, 120));

    assert_eq!(
        block(&text, "Skills"),
        vec!["  Skills       test-driven-development · writing-plans".to_string()]
    );
}

#[test]
fn usage_closes_the_card_with_context_then_tokens_then_cost() {
    let text = plain(&card(ExitToolsFixture::Few, ExitFieldsFixture::Full, 80));
    let usage_start = text
        .iter()
        .position(|row| row.trim_start().starts_with("Context"))
        .expect("context row");

    assert_eq!(text[usage_start - 1], "");
    assert_eq!(
        text[usage_start..],
        [
            "  Context      164,208 of 1,000,000 · 16% full",
            "  Tokens       2,204,330  total",
            "                  18,420  input",
            "               2,071,850  cached read",
            "                  72,100  cached write",
            "                  41,960  output",
            "  Cost         $3.42 · reported by the agent",
        ]
        .map(String::from)
    );
}

#[test]
fn a_session_without_usage_reports_has_no_usage_section() {
    let text = plain(&card(ExitToolsFixture::Few, ExitFieldsFixture::Minimal, 80));

    assert!(!text.iter().any(|row| {
        ["Context", "Tokens", "Cost"]
            .iter()
            .any(|label| row.trim_start().starts_with(label))
    }));
}

#[test]
fn narrow_terminals_stack_each_label_above_its_values() {
    let text = plain(&card(STRESS.0, STRESS.1, 24));
    let tools = text
        .iter()
        .position(|row| row == "  Tools")
        .expect("Tools label alone on its row");

    assert_eq!(text[tools + 1], "    37  Read");
}

#[test]
fn no_color_keeps_emphasis_but_drops_every_color() {
    let model = exit_card_fixture(STRESS.0, STRESS.1);
    let paint = ExitCardPaint {
        shade_from: None,
        color: false,
    };
    let lines = exit_card_lines(&model, 80, paint);

    assert!(
        lines
            .iter()
            .flat_map(|line| &line.spans)
            .all(|span| span.style.fg.is_none() && span.style.bg.is_none())
    );
    assert_eq!(
        plain(&lines),
        plain(&exit_card_lines(&model, 80, ExitCardPaint::plain()))
    );
}

#[test]
fn an_unknown_terminal_background_gets_no_band() {
    let lines = card(STRESS.0, STRESS.1, 80);

    assert!(
        lines
            .iter()
            .flat_map(|line| &line.spans)
            .all(|span| span.style.bg.is_none())
    );
}

#[test]
fn shading_paints_one_even_band() {
    let model = exit_card_fixture(STRESS.0, STRESS.1);
    let paint = ExitCardPaint {
        shade_from: Some((24, 26, 27)),
        color: true,
    };
    let band = crate::style::user_message_bg((24, 26, 27));
    for width in [24_u16, 40, 80, 120] {
        let lines = exit_card_lines(&model, width, paint);
        let shaded = &lines;
        let band_width = shaded[0].width();

        assert!(
            shaded.iter().all(|line| line.width() == band_width),
            "uneven band at {width}"
        );
        assert!(usize::from(width) >= band_width);
        assert!(
            shaded
                .iter()
                .flat_map(|line| &line.spans)
                .all(|span| span.style.bg == Some(band))
        );
        assert_eq!(plain(&shaded[..1]), vec![String::new()]);
    }
}

#[test]
fn zero_width_renders_nothing() {
    assert_eq!(card(STRESS.0, STRESS.1, 0), Vec::<Line<'static>>::new());
}

#[test]
fn terminal_snapshots() {
    for (name, tools, fields, width) in [
        (
            "claude_80",
            ExitToolsFixture::Few,
            ExitFieldsFixture::Full,
            80,
        ),
        ("codex_120", STRESS.0, ExitFieldsFixture::Codex, 120),
        ("stress_40", STRESS.0, STRESS.1, 40),
        ("stress_24", STRESS.0, STRESS.1, 24),
        (
            "long_names_48",
            ExitToolsFixture::LongNames,
            ExitFieldsFixture::Full,
            48,
        ),
        (
            "tiny_10",
            ExitToolsFixture::Few,
            ExitFieldsFixture::Full,
            10,
        ),
    ] {
        let rows = terminal_rows(card(tools, fields, width), width);
        let framed: Vec<String> = rows
            .iter()
            .map(|row| format!("{row:<w$}|", w = usize::from(width)))
            .collect();
        insta::assert_snapshot!(name, framed.join("\n"));
    }
}
