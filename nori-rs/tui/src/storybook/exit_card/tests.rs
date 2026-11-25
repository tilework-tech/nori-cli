use pretty_assertions::assert_eq;
use ratatui::layout::Rect;
use ratatui::text::Line;

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

/// Print the card into a VT100 terminal through the same history path the
/// session uses, and return every terminal row the card occupies.
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

fn block_after(rows: &[String], label: &str) -> Vec<String> {
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
fn tool_continuation_rows_start_at_the_value_column() {
    let rows = terminal_rows(card(STRESS.0, STRESS.1, 40), 40);
    let tools = block_after(&rows, "Tools");

    assert!(tools.len() > 3, "the stress case must wrap: {tools:#?}");
    for row in &tools[1..] {
        assert_eq!(
            row.chars().take_while(|ch| *ch == ' ').count(),
            VALUE_OFFSET,
            "continuation row is misaligned: {row:?}"
        );
    }
}

#[test]
fn tools_that_fit_are_never_split_across_rows() {
    let rows = terminal_rows(card(STRESS.0, STRESS.1, 40), 40);
    for item in ["Read 37", "TodoWrite 7", "WebFetch 5", "NotebookEdit 1"] {
        assert!(
            rows.iter().any(|row| row.contains(item)),
            "{item:?} was split: {rows:#?}"
        );
    }
}

#[test]
fn a_tool_name_wider_than_the_column_breaks_after_its_separators() {
    let rows = terminal_rows(card(STRESS.0, STRESS.1, 40), 40);
    let tools = block_after(&rows, "Tools");
    let pieces: Vec<&str> = tools
        .iter()
        .map(|row| row.trim_start())
        .skip_while(|row| !row.starts_with("mcp__github"))
        .take(3)
        .collect();

    assert_eq!(
        pieces,
        vec![
            "mcp__github__create_pull_",
            "request_review_comment 4",
            "Write 3"
        ]
    );
}

#[test]
fn a_name_without_separators_is_cut_at_the_column_edge() {
    let rows = terminal_rows(
        card(ExitToolsFixture::LongNames, ExitFieldsFixture::Minimal, 40),
        40,
    );
    let tools = block_after(&rows, "Tools");
    let start = tools
        .iter()
        .position(|row| row.contains("Exampletool"))
        .expect("unbreakable name shown");

    assert_eq!(tools[start].trim_start().len(), 40 - VALUE_OFFSET);
    assert_eq!(tools[start + 1].trim_start(), "separatorsatall 2");
}

#[test]
fn narrow_terminals_stack_each_label_above_its_values() {
    let rows = terminal_rows(card(STRESS.0, STRESS.1, 24), 24);
    let tools = rows
        .iter()
        .position(|row| row == "  Tools")
        .expect("Tools label alone on its row");

    assert!(
        rows[tools + 1].starts_with("    Read 37"),
        "values indent under the label: {rows:#?}"
    );
}

#[test]
fn rows_without_values_are_left_out() {
    let rows = plain(&card(
        ExitToolsFixture::None,
        ExitFieldsFixture::NotResumable,
        80,
    ));

    assert_eq!(
        rows,
        vec![
            "  › Nori CLI  session ended".to_string(),
            String::new(),
            "  Session      019a7c3e-5b21-7f4d-9e08-6c2a1d4b8f37".to_string(),
        ]
    );
}

#[test]
fn a_session_without_an_id_shows_only_the_heading() {
    let rows = plain(&card(
        ExitToolsFixture::None,
        ExitFieldsFixture::NoSession,
        80,
    ));

    assert_eq!(rows, vec!["  › Nori CLI  session ended".to_string()]);
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
fn shading_paints_one_even_band_with_a_blank_row_above_and_below() {
    let model = exit_card_fixture(STRESS.0, STRESS.1);
    let paint = ExitCardPaint {
        shade_from: Some((24, 26, 27)),
        color: true,
    };
    for width in [24_u16, 40, 80, 120] {
        let lines = exit_card_lines(&model, width, paint);
        let band = crate::style::user_message_bg((24, 26, 27));
        let first_width = lines[0].width();

        assert!(
            lines.iter().all(|line| line.width() == first_width),
            "uneven band at {width}"
        );
        assert!(usize::from(width) >= first_width);
        assert!(
            lines
                .iter()
                .flat_map(|line| &line.spans)
                .all(|span| span.style.bg == Some(band))
        );
        assert_eq!(plain(&lines[..1]), vec![String::new()]);
        assert_eq!(plain(&lines[lines.len() - 1..]), vec![String::new()]);
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
            "typical_80",
            ExitToolsFixture::Few,
            ExitFieldsFixture::Full,
            80,
        ),
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
