use crate::MenuItem;
use crate::MenuState;
use crate::OverlayMenu;
use crate::Picker;
use crate::PickerAction;
use crate::PickerColumn;
use crate::PickerItem;
use crate::PickerMode;
use crate::PickerState;
use crate::Theme;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::widgets::StatefulWidget;
use ratatui::widgets::Widget;

const DARK_TERMINAL: Option<(u8, u8, u8)> = Some((20, 20, 24));

fn model_picker() -> PickerState<&'static str> {
    let items = [
        PickerItem::new("fast", "title", "Quick draft").description("Short replies"),
        PickerItem::new("deep", "title", "Careful review").description("Longer reasoning"),
        PickerItem::new("plain", "title", "Plain text").description("No formatting"),
    ];
    PickerState::new(
        "Pick a model",
        [PickerColumn::flexible("title", "Model")],
        items,
    )
}

fn render_picker(state: &PickerState<&'static str>, theme: Theme, height: u16) -> Buffer {
    let area = Rect::new(0, 0, 40, height);
    let mut buffer = Buffer::empty(area);
    Picker::new(state)
        .theme(theme)
        .show_details(false)
        .render(area, &mut buffer);
    buffer
}

fn render_menu(theme: Theme) -> Buffer {
    let area = Rect::new(0, 0, 40, 10);
    let mut buffer = Buffer::empty(area);
    let mut state = MenuState::try_new([
        MenuItem::new("yes", "Trust this folder").number_shortcut(1),
        MenuItem::new("no", "Quit").number_shortcut(2),
    ])
    .expect("valid menu");
    OverlayMenu::new("Trust folder?")
        .theme(theme)
        .backdrop(false)
        .render(area, &mut buffer, &mut state);
    buffer
}

fn row_of(buffer: &Buffer, text: &str) -> u16 {
    (0..buffer.area.height)
        .find(|y| line(buffer, *y).contains(text))
        .unwrap_or_else(|| panic!("no row contains {text:?}"))
}

fn line(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

/// The focus marks on a row, left to right: rails, pointer, and dots.
fn marks(buffer: &Buffer, y: u16) -> Vec<(&'static str, Color, bool)> {
    let mut found = Vec::new();
    for x in 0..buffer.area.width {
        let cell = &buffer[(x, y)];
        for mark in ["▏", "›", "●", "○", "▕"] {
            if cell.symbol() == mark {
                found.push((mark, cell.fg, cell.modifier.contains(Modifier::BOLD)));
            }
        }
    }
    found
}

fn column_of(buffer: &Buffer, y: u16, text: &str) -> u16 {
    (0..buffer.area.width)
        .find(|x| {
            text.chars().enumerate().all(|(offset, ch)| {
                let x = x + offset as u16;
                x < buffer.area.width && buffer[(x, y)].symbol() == ch.to_string()
            })
        })
        .unwrap_or_else(|| panic!("{text:?} missing from row {y}"))
}

#[test]
fn focused_picker_row_has_rails_and_a_green_pointer_beside_its_number() {
    let buffer = render_picker(
        &model_picker(),
        Theme::for_terminal_background(DARK_TERMINAL),
        12,
    );
    let focused = row_of(&buffer, "Quick draft");
    let other = row_of(&buffer, "Careful review");

    assert_eq!(
        marks(&buffer, focused),
        vec![
            ("▏", Color::Green, false),
            ("›", Color::Green, false),
            ("▕", Color::Green, false),
        ]
    );
    assert!(line(&buffer, focused).contains("› 1. Quick draft"));
    assert_eq!(marks(&buffer, other), Vec::new());
    assert!(line(&buffer, other).contains("  2. Careful review"));
}

#[test]
fn focused_picker_description_line_keeps_the_rails_but_not_the_pointer() {
    let buffer = render_picker(&model_picker(), Theme::default(), 12);
    let description = row_of(&buffer, "Short replies");

    assert_eq!(
        marks(&buffer, description),
        vec![("▏", Color::Green, true), ("▕", Color::Green, true)]
    );
}

#[test]
fn shaded_picker_focus_fills_the_row_background() {
    let theme = Theme::for_terminal_background(DARK_TERMINAL);
    let buffer = render_picker(&model_picker(), theme, 12);
    let focused = row_of(&buffer, "Quick draft");
    let label = column_of(&buffer, focused, "Quick");

    assert_eq!(
        buffer[(label, focused)].bg,
        theme.selected.bg.expect("shade")
    );
    assert!(!buffer[(label, focused)].modifier.contains(Modifier::BOLD));
}

#[test]
fn unshaded_picker_focus_uses_bold_green_pointer_and_rails() {
    let buffer = render_picker(&model_picker(), Theme::default(), 12);
    let focused = row_of(&buffer, "Quick draft");

    assert_eq!(
        marks(&buffer, focused),
        vec![
            ("▏", Color::Green, true),
            ("›", Color::Green, true),
            ("▕", Color::Green, true),
        ]
    );
}

#[test]
fn moving_focus_moves_the_pointer_and_rails_together() {
    let mut state = model_picker();
    state.handle(PickerAction::MoveDown);
    let buffer = render_picker(&state, Theme::default(), 12);

    assert_eq!(marks(&buffer, row_of(&buffer, "Quick draft")), Vec::new());
    assert_eq!(
        marks(&buffer, row_of(&buffer, "Careful review"))
            .into_iter()
            .map(|(mark, _, _)| mark)
            .collect::<Vec<_>>(),
        vec!["▏", "›", "▕"]
    );
}

#[test]
fn rows_past_nine_keep_the_label_column_of_numbered_rows() {
    const LABELS: [&str; 10] = [
        "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India", "Juliet",
    ];
    let items = LABELS.map(|label| PickerItem::new(label, "title", label));
    let mut state = PickerState::new("Pick one", [PickerColumn::flexible("title", "Name")], items);
    for _ in 0..9 {
        state.handle(PickerAction::MoveDown);
    }
    let buffer = render_picker(&state, Theme::default(), 14);
    let ninth = row_of(&buffer, "India");
    let tenth = row_of(&buffer, "Juliet");

    assert_eq!(
        column_of(&buffer, tenth, "Juliet"),
        column_of(&buffer, ninth, "India")
    );
    assert!(line(&buffer, tenth).contains("›    Juliet"));
}

#[test]
fn multi_choice_rows_show_dots_beside_the_pointer_column() {
    let mut state = model_picker().mode(PickerMode::Multi);
    state.handle(PickerAction::Toggle);
    state.handle(PickerAction::MoveDown);
    let buffer = render_picker(&state, Theme::default(), 12);

    let checked = row_of(&buffer, "Quick draft");
    let focused = row_of(&buffer, "Careful review");
    assert_eq!(marks(&buffer, checked), vec![("●", Color::Green, false)]);
    assert!(line(&buffer, checked).contains("  ● Quick draft"));
    assert_eq!(
        marks(&buffer, focused)
            .into_iter()
            .map(|(mark, _, _)| mark)
            .collect::<Vec<_>>(),
        vec!["▏", "›", "○", "▕"]
    );
    assert!(line(&buffer, focused).contains("› ○ Careful review"));
}

#[test]
fn focused_menu_item_has_rails_a_pointer_and_its_number() {
    let buffer = render_menu(Theme::default());
    let focused = row_of(&buffer, "Trust this folder");
    let other = row_of(&buffer, "Quit");

    assert_eq!(
        marks(&buffer, focused),
        vec![
            ("▏", Color::Green, true),
            ("›", Color::Green, true),
            ("▕", Color::Green, true),
        ]
    );
    assert!(line(&buffer, focused).contains("▏› 1  Trust this folder"));
    assert_eq!(marks(&buffer, other), Vec::new());
    assert_eq!(
        column_of(&buffer, other, "Quit"),
        column_of(&buffer, focused, "Trust")
    );
}

#[test]
fn shaded_menu_focus_keeps_a_regular_weight_pointer() {
    let buffer = render_menu(Theme::for_terminal_background(DARK_TERMINAL));
    let focused = row_of(&buffer, "Trust this folder");

    assert_eq!(
        marks(&buffer, focused),
        vec![
            ("▏", Color::Green, false),
            ("›", Color::Green, false),
            ("▕", Color::Green, false),
        ]
    );
}

fn snapshot_picker(state: &PickerState<&'static str>, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|frame| frame.render_widget(Picker::new(state).show_details(false), frame.area()))
        .expect("draw");
    terminal.backend().to_string()
}

#[test]
fn numbered_picker_focus_snapshot() {
    assert_snapshot!(snapshot_picker(&model_picker(), 40, 12));
}

#[test]
fn focused_menu_descriptions_that_fit_two_lines_are_never_cut() {
    // Labels here have room for two of these words per line.
    for words in 1..=4 {
        let description = vec!["abcde"; words].join(" ");
        let mut state = MenuState::try_new([
            MenuItem::new("first", "First").description(description.clone()),
            MenuItem::new("second", "Second"),
        ])
        .expect("valid menu");
        let area = Rect::new(0, 0, 25, 14);
        let mut buffer = Buffer::empty(area);
        OverlayMenu::new("Pick")
            .backdrop(false)
            .render(area, &mut buffer, &mut state);
        let text = (0..area.height)
            .map(|y| line(&buffer, y))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!text.contains('…'), "{words} words were cut:\n{text}");
    }
}

#[test]
fn a_menus_current_item_says_current_in_parentheses_even_when_narrow() {
    for width in [40, 22] {
        let mut state = MenuState::try_new([
            MenuItem::new("keep", "Keep the synthetic session").current(true),
            MenuItem::new("drop", "Drop it"),
        ])
        .expect("valid menu");
        let area = Rect::new(0, 0, width, 10);
        let mut buffer = Buffer::empty(area);
        OverlayMenu::new("Pick")
            .backdrop(false)
            .render(area, &mut buffer, &mut state);
        let row = line(&buffer, row_of(&buffer, "(current)"));
        assert!(row.contains("› K"), "width {width}: {row}");
    }
}

#[test]
fn an_on_screen_picker_row_says_on_screen_not_current() {
    let state = PickerState::new(
        "Sessions",
        [PickerColumn::flexible("title", "Session")],
        [
            PickerItem::new("a", "title", "Synthetic parser fix").on_screen(true),
            PickerItem::new("b", "title", "Synthetic docs pass"),
        ],
    );
    let buffer = render_picker(&state, Theme::default(), 10);
    let row = line(&buffer, row_of(&buffer, "Synthetic parser fix"));

    assert!(row.contains("(on screen)"), "{row}");
    assert!(!row.contains("current"), "{row}");
}
