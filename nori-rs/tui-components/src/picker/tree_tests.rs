use super::*;
use crate::DetailTone;
use crate::Theme;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn family_picker() -> PickerState<String> {
    let columns = [
        PickerColumn::flexible("title", "Session"),
        PickerColumn::fixed("status", "Status", 16),
    ];
    let row = |key: &str, depth, status: &str, preview: &str| {
        PickerItem::new(key.to_string(), "title", key)
            .depth(depth)
            .cell("status", status)
            .description(preview)
    };
    let items = [
        row(
            "Fix flaky login test",
            0,
            "waiting on you",
            "approve: rm -rf target/debug",
        ),
        row("Port picker", 0, "working", "running: cargo build"),
        row(
            "Write snapshot tests",
            1,
            "working",
            "running: cargo insta test",
        ),
        row("Check fixtures", 2, "ready", "Fixtures match"),
        row(
            "Update docs.md",
            1,
            "ready · new",
            "Updated the tree row API",
        ),
        row(
            "Spike remote transport",
            0,
            "failed",
            "agent exited (code 1)",
        ),
    ];
    PickerState::new("Sessions", columns, items)
}

fn render(state: &PickerState<String>, width: u16, height: u16, density: PickerDensity) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|frame| {
            frame.render_widget(
                Picker::new(state)
                    .density(density)
                    .show_title(false)
                    .compact_descriptions(true),
                frame.area(),
            )
        })
        .expect("draw");
    terminal.backend().to_string()
}

#[test]
fn tree_rows_draw_guides_at_every_depth_with_previews_in_compact_density() {
    assert_snapshot!(render(&family_picker(), 72, 18, PickerDensity::Compact));
}

#[test]
fn tree_rows_at_normal_density_snapshot() {
    assert_snapshot!(render(&family_picker(), 72, 18, PickerDensity::Normal));
}

#[test]
fn compact_rows_without_compact_descriptions_stay_one_line() {
    let mut terminal = Terminal::new(TestBackend::new(72, 14)).expect("terminal");
    let state = family_picker();
    terminal
        .draw(|frame| {
            frame.render_widget(
                Picker::new(&state)
                    .density(PickerDensity::Compact)
                    .show_title(false),
                frame.area(),
            )
        })
        .expect("draw");
    let screen = terminal.backend().to_string();
    assert!(!screen.contains("approve: rm -rf"), "{screen}");
    assert!(screen.contains("Spike remote transport"), "{screen}");
}

#[test]
fn replacing_rows_keeps_the_selection_on_the_same_key() {
    let mut state = family_picker();
    state.handle(PickerAction::MoveDown);
    state.handle(PickerAction::MoveDown);
    let selected = state.selected_item().map(|item| item.key.clone());

    let mut reordered = family_picker().items;
    reordered.reverse();
    state.replace_items(reordered);
    assert_eq!(state.selected_item().map(|item| item.key.clone()), selected);

    state.replace_items(
        family_picker()
            .items
            .into_iter()
            .filter(|item| Some(&item.key) != selected.as_ref()),
    );
    assert_eq!(
        state.selected_item().map(|item| item.key.as_str()),
        Some("Fix flaky login test"),
        "a removed selection falls back to the first row"
    );
}

#[test]
#[allow(clippy::disallowed_methods)]
fn a_cell_tone_colours_that_cell_through_the_theme() {
    let theme = Theme::for_terminal_background(Some((20, 20, 20)));
    let columns = [
        PickerColumn::flexible("title", "Session"),
        PickerColumn::fixed("status", "Status", 10),
    ];
    let items = [
        PickerItem::new("a".to_string(), "title", "first"),
        PickerItem::new("b".to_string(), "title", "second")
            .cell("status", "failed")
            .cell_detail_tone("status", DetailTone::Error),
    ];
    let state = PickerState::new("Sessions", columns, items);
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).expect("terminal");
    terminal
        .draw(|frame| {
            frame.render_widget(
                Picker::new(&state)
                    .theme(theme)
                    .density(PickerDensity::Compact)
                    .show_title(false),
                frame.area(),
            )
        })
        .expect("draw");
    let buffer = terminal.backend().buffer();
    let (x, y) = (0..buffer.area.height)
        .find_map(|y| {
            (0..buffer.area.width.saturating_sub(5)).find_map(|x| {
                let word: String = (x..x + 6).map(|x| buffer[(x, y)].symbol()).collect();
                (word == "failed").then_some((x, y))
            })
        })
        .unwrap_or_else(|| panic!("status cell drawn:\n{}", terminal.backend()));
    assert_eq!(buffer[(x, y)].fg, theme.error.fg.expect("error colour"));
}
