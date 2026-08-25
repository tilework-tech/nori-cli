use super::*;
use crate::KeyHint;
use crate::Theme;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::widgets::Block;
use ratatui::widgets::StatefulWidget;
use ratatui::widgets::Widget;

fn basic_item(key: &'static str, label: &'static str) -> MenuItem<&'static str> {
    MenuItem::new(key, label).description(format!("Supporting copy for {label}"))
}

fn action_state() -> MenuState<&'static str> {
    MenuState::try_new([
        MenuItem::new(
            "resume",
            "Resume the selected transcript without changing its history",
        )
        .description("Continue the selected transcript"),
        basic_item("new", "Start a new session"),
        basic_item("inspect", "Open read-only"),
        basic_item("share", "Share session").disabled(true),
    ])
    .expect("valid action menu")
}

fn shortcut_state() -> MenuState<&'static str> {
    MenuState::try_new([
        basic_item("resume", "Resume session")
            .mnemonic('r')
            .number_shortcut(1),
        basic_item("new", "Start a new session")
            .mnemonic('s')
            .number_shortcut(2),
        basic_item("inspect", "Inspect transcript")
            .mnemonic('i')
            .number_shortcut(3),
        basic_item("archive", "Archive session")
            .mnemonic('a')
            .number_shortcut(4),
        basic_item("share", "Share session")
            .number_shortcut(5)
            .disabled(true),
    ])
    .expect("valid shortcut menu")
}

fn consequence_state() -> MenuState<&'static str> {
    MenuState::try_new([
        basic_item("keep", "Keep session").current(true),
        basic_item("delete", "Delete local transcript").tone(MenuItemTone::Destructive),
        basic_item("archive", "Archive before deleting").tone(MenuItemTone::Warning),
        basic_item("disabled", "Delete remote history")
            .tone(MenuItemTone::Destructive)
            .disabled(true),
    ])
    .expect("valid consequence menu")
}

#[test]
fn construction_selects_the_first_enabled_item_and_handles_empty_menus() {
    let state = MenuState::try_new([
        basic_item("disabled", "Unavailable").disabled(true),
        basic_item("enabled", "Available"),
    ])
    .expect("valid menu");
    assert_eq!(state.selected_index(), Some(1));
    assert_eq!(state.selected_item().map(MenuItem::key), Some(&"enabled"));

    let empty = MenuState::<&str>::try_new([]).expect("empty menus are valid");
    assert_eq!(empty.selected_index(), None);

    let disabled = MenuState::try_new([basic_item("disabled", "Unavailable").disabled(true)])
        .expect("all-disabled menus are valid");
    assert_eq!(disabled.selected_index(), None);
}

#[test]
fn movement_wraps_and_skips_disabled_items() {
    let mut state = MenuState::try_new([
        basic_item("first", "First"),
        basic_item("disabled", "Disabled").disabled(true),
        basic_item("last", "Last"),
    ])
    .expect("valid menu");

    assert_eq!(
        state.handle(MenuAction::MoveUp),
        MenuOutcome::SelectionChanged(Some("last"))
    );
    assert_eq!(
        state.handle(MenuAction::MoveDown),
        MenuOutcome::SelectionChanged(Some("first"))
    );
    assert_eq!(
        state.handle(MenuAction::MoveDown),
        MenuOutcome::SelectionChanged(Some("last"))
    );
}

#[test]
fn edge_and_stable_key_selection_return_public_outcomes() {
    let mut state = MenuState::try_new([
        basic_item("one", "One"),
        basic_item("disabled", "Disabled").disabled(true),
        basic_item("three", "Three"),
        basic_item("four", "Four"),
        basic_item("five", "Five"),
    ])
    .expect("valid menu");
    assert_eq!(
        state.handle(MenuAction::Last),
        MenuOutcome::SelectionChanged(Some("five"))
    );
    assert_eq!(
        state.handle(MenuAction::First),
        MenuOutcome::SelectionChanged(Some("one"))
    );
    assert_eq!(
        state.select_key(&"three"),
        MenuOutcome::SelectionChanged(Some("three"))
    );
    assert_eq!(state.select_key(&"disabled"), MenuOutcome::Unchanged);
    assert_eq!(state.select_key(&"missing"), MenuOutcome::Unchanged);
}

#[test]
fn number_and_character_shortcuts_activate_the_same_dual_shortcut_item() {
    let mut number_state = shortcut_state();
    assert_eq!(
        number_state.handle(MenuAction::InvokeShortcut(MenuShortcut::Number(1))),
        MenuOutcome::Activated("resume")
    );

    let mut character_state = shortcut_state();
    assert_eq!(
        character_state.handle(MenuAction::InvokeShortcut(MenuShortcut::Character('R'))),
        MenuOutcome::Activated("resume")
    );
}

#[test]
fn shortcuts_are_case_insensitive_and_disabled_items_do_not_activate() {
    let mut state = shortcut_state();
    assert_eq!(
        state.handle(MenuAction::InvokeShortcut(MenuShortcut::Character('a'))),
        MenuOutcome::Activated("archive")
    );
    assert_eq!(
        state.handle(MenuAction::InvokeShortcut(MenuShortcut::Number(5))),
        MenuOutcome::Unchanged
    );
}

#[test]
fn selected_activation_and_cancellation_return_typed_outcomes() {
    let mut state = shortcut_state();
    assert_eq!(
        state.handle(MenuAction::ActivateSelected),
        MenuOutcome::Activated("resume")
    );
    assert_eq!(state.handle(MenuAction::Cancel), MenuOutcome::Cancelled);
}

#[test]
fn duplicate_keys_and_shortcuts_are_rejected() {
    assert_eq!(
        MenuState::try_new([basic_item("same", "One"), basic_item("same", "Two")]),
        Err(MenuModelError::DuplicateKey)
    );
    assert_eq!(
        MenuState::try_new([
            basic_item("one", "Resume").mnemonic('r'),
            basic_item("two", "Retry").mnemonic('R'),
        ]),
        Err(MenuModelError::DuplicateCharacterShortcut('r'))
    );
    assert_eq!(
        MenuState::try_new([
            basic_item("one", "One").number_shortcut(1),
            basic_item("two", "Two").number_shortcut(1),
        ]),
        Err(MenuModelError::DuplicateNumberShortcut(1))
    );
}

#[test]
fn shortcuts_are_ascii_alphabetic_single_digits_and_match_the_label_prefix() {
    assert_eq!(
        MenuState::try_new([basic_item("zero", "Zero").number_shortcut(0)]),
        Err(MenuModelError::InvalidNumberShortcut(0))
    );
    assert_eq!(
        MenuState::try_new([basic_item("ten", "Ten").number_shortcut(10)]),
        Err(MenuModelError::InvalidNumberShortcut(10))
    );
    assert_eq!(
        MenuState::try_new([basic_item("digit", "Digit").mnemonic('1')]),
        Err(MenuModelError::InvalidCharacterShortcut('1'))
    );
    assert_eq!(
        MenuState::try_new([basic_item("punctuation", "Punctuation").mnemonic('!')]),
        Err(MenuModelError::InvalidCharacterShortcut('!'))
    );
    assert_eq!(
        MenuState::try_new([basic_item("unicode", "Éditer").mnemonic('é')]),
        Err(MenuModelError::InvalidCharacterShortcut('é'))
    );
    assert_eq!(
        MenuState::try_new([basic_item("resume", "Resume").mnemonic('x')]),
        Err(MenuModelError::MnemonicDoesNotMatchLabel {
            mnemonic: 'x',
            label: "Resume".to_string(),
        })
    );
}

#[test]
fn unavailable_or_empty_menus_ignore_activation_and_navigation() {
    let mut empty = MenuState::<&str>::try_new([]).expect("empty menu");
    assert_eq!(empty.handle(MenuAction::MoveDown), MenuOutcome::Unchanged);
    assert_eq!(
        empty.handle(MenuAction::ActivateSelected),
        MenuOutcome::Unchanged
    );

    let mut disabled = MenuState::try_new([basic_item("disabled", "Disabled").disabled(true)])
        .expect("disabled menu");
    assert_eq!(disabled.handle(MenuAction::MoveUp), MenuOutcome::Unchanged);
    assert_eq!(
        disabled.handle(MenuAction::ActivateSelected),
        MenuOutcome::Unchanged
    );
}

#[test]
fn page_navigation_uses_the_rendered_viewport_and_keeps_selection_visible() {
    let mut state = MenuState::try_new([
        basic_item("one", "One"),
        basic_item("disabled", "Disabled").disabled(true),
        basic_item("three", "Three"),
        basic_item("four", "Four"),
        basic_item("five", "Five"),
    ])
    .expect("valid paged menu");
    let _ = snapshot(&mut state, 30, 7, "Choose", None);

    assert_eq!(
        state.handle(MenuAction::PageDown),
        MenuOutcome::SelectionChanged(Some("three"))
    );
    let page_down = snapshot(&mut state, 30, 7, "Choose", None);
    assert!(page_down.contains("▏ Three"));
    assert!(page_down.contains('↑'));
    assert!(page_down.contains('↓'));
    assert!(state.viewport_offset() > 0);

    assert_eq!(
        state.handle(MenuAction::PageUp),
        MenuOutcome::SelectionChanged(Some("one"))
    );
    let page_up = snapshot(&mut state, 30, 7, "Choose", None);
    assert!(page_up.contains("▏ One"));
}

#[test]
fn menu_30x12_snapshot_hides_supporting_subtitle_before_primary_labels() {
    let mut narrow = action_state();
    narrow.handle(MenuAction::MoveDown);
    narrow.handle(MenuAction::MoveDown);
    let rendered = snapshot(
        &mut narrow,
        30,
        12,
        "Continue",
        Some("Supporting copy disappears at this width"),
    );
    assert!(!rendered.contains("Supporting copy disappears"));
    assert_snapshot!("menu_30x12", rendered);
}

#[test]
fn menu_40x12_shortcut_snapshot() {
    let mut shortcut_narrow = shortcut_state();
    assert_snapshot!(
        "menu_40x12_shortcuts",
        snapshot(
            &mut shortcut_narrow,
            40,
            12,
            "Choose a session action",
            Some("Shortcuts activate immediately")
        )
    );
}

#[test]
fn menu_54x16_consequence_snapshot() {
    let mut consequences = consequence_state();
    consequences.select_key(&"delete");
    assert_snapshot!(
        "menu_54x16_consequences",
        snapshot(
            &mut consequences,
            54,
            16,
            "Remove local session?",
            Some("Remote history will remain available")
        )
    );
}

#[test]
fn menu_80x24_action_snapshot() {
    let mut action = action_state();
    assert_snapshot!(
        "menu_80x24_action",
        snapshot(
            &mut action,
            80,
            24,
            "Choose how to continue",
            Some("Select one action")
        )
    );
}

#[test]
fn menu_100x30_shortcut_snapshot() {
    let mut shortcut_wide = shortcut_state();
    assert_snapshot!(
        "menu_100x30_shortcuts",
        snapshot(
            &mut shortcut_wide,
            100,
            30,
            "Choose a session action",
            Some("Shortcuts activate immediately")
        )
    );
}

#[test]
#[allow(clippy::disallowed_methods)]
fn derived_surfaces_style_the_backdrop_menu_and_title() {
    let theme = Theme::for_terminal_background(Some((20, 20, 20)));
    let mut state = consequence_state();
    let buffer = rendered_buffer(
        &mut state,
        80,
        24,
        "Remove local session?",
        Some("Remote history will remain available"),
        theme,
        Color::Blue,
    );

    assert_eq!(buffer[(0, 0)].bg, Color::Rgb(29, 29, 29));
    let title = find_ascii_text(&buffer, "Remove local session?").expect("title");
    assert_eq!(buffer[title].bg, Color::Rgb(38, 38, 38));
    assert!(buffer[title].modifier.contains(Modifier::BOLD));
}

#[test]
#[allow(clippy::disallowed_methods)]
fn unselected_enabled_items_use_a_deeper_surface_than_disabled_items() {
    let theme = Theme::for_terminal_background(Some((20, 20, 20)));
    let mut state = MenuState::try_new([
        basic_item("selected", "Selected action"),
        basic_item("enabled", "Enabled action"),
        basic_item("disabled", "Disabled action").disabled(true),
    ])
    .expect("valid surface menu");
    let buffer = rendered_buffer(
        &mut state,
        80,
        24,
        "Surface depth",
        None,
        theme,
        Color::Blue,
    );

    let selected = find_ascii_text(&buffer, "Selected action").expect("selected item");
    let enabled = find_ascii_text(&buffer, "Enabled action").expect("enabled item");
    let disabled = find_ascii_text(&buffer, "Disabled action").expect("disabled item");

    assert_eq!(buffer[selected].bg, Color::Rgb(43, 43, 43));
    assert_eq!(buffer[enabled].bg, Color::Rgb(35, 35, 35));
    assert_eq!(buffer[disabled].bg, Color::Rgb(38, 38, 38));
    assert_eq!(buffer[disabled].fg, Color::DarkGray);
}

#[test]
#[allow(clippy::disallowed_methods)]
fn enabled_items_stay_darker_than_disabled_items_on_light_backgrounds() {
    let theme = Theme::for_terminal_background(Some((240, 240, 240)));
    let mut state = MenuState::try_new([
        basic_item("selected", "Selected action"),
        basic_item("enabled", "Enabled action"),
        basic_item("disabled", "Disabled action").disabled(true),
    ])
    .expect("valid light surface menu");
    let buffer = rendered_buffer(
        &mut state,
        80,
        24,
        "Surface depth",
        None,
        theme,
        Color::Blue,
    );

    let selected = find_ascii_text(&buffer, "Selected action").expect("selected item");
    let enabled = find_ascii_text(&buffer, "Enabled action").expect("enabled item");
    let disabled = find_ascii_text(&buffer, "Disabled action").expect("disabled item");

    assert_eq!(buffer[selected].bg, Color::Rgb(216, 216, 216));
    assert_eq!(buffer[enabled].bg, Color::Rgb(217, 217, 217));
    assert_eq!(buffer[disabled].bg, Color::Rgb(220, 220, 220));
    assert_eq!(buffer[disabled].fg, Color::DarkGray);
}

#[test]
#[allow(clippy::disallowed_methods)]
fn selection_fills_every_item_row_and_uses_symmetric_accent_rails() {
    let theme = Theme::for_terminal_background(Some((20, 20, 20)));
    let mut state = consequence_state();
    let buffer = rendered_buffer(
        &mut state,
        80,
        24,
        "Remove local session?",
        None,
        theme,
        Color::Blue,
    );

    let selected = find_ascii_text(&buffer, "Keep session").expect("selected label");
    let left_rail = find_symbol_on_row(&buffer, selected.1, "▏").expect("left rail");
    let right_rail = find_symbol_on_row(&buffer, selected.1, "▕").expect("right rail");
    assert_eq!(buffer[left_rail].fg, Color::Cyan);
    assert_eq!(buffer[right_rail].fg, Color::Cyan);
    assert_eq!(
        find_symbol_on_row(&buffer, selected.1 + 1, "▏"),
        Some((left_rail.0, selected.1 + 1))
    );
    assert_eq!(
        find_symbol_on_row(&buffer, selected.1 + 1, "▕"),
        Some((right_rail.0, selected.1 + 1))
    );
    for y in selected.1..=selected.1 + 1 {
        for x in left_rail.0..=right_rail.0 {
            assert_eq!(buffer[(x, y)].bg, Color::Rgb(43, 43, 43));
        }
    }
    assert_eq!(buffer[selected].fg, Color::Cyan);
}

#[test]
fn semantic_tones_disabled_copy_and_current_state_keep_distinct_styles() {
    let mut initial = consequence_state();
    let initial_buffer = rendered_buffer(
        &mut initial,
        80,
        24,
        "Remove local session?",
        None,
        Theme::default(),
        Color::Blue,
    );
    let destructive =
        find_ascii_text(&initial_buffer, "Delete local transcript").expect("destructive label");
    assert_eq!(initial_buffer[destructive].fg, Color::Red);

    let mut state = consequence_state();
    state.select_key(&"delete");
    let buffer = rendered_buffer(
        &mut state,
        80,
        24,
        "Remove local session?",
        None,
        Theme::default(),
        Color::Blue,
    );

    let warning = find_ascii_text(&buffer, "Archive before deleting").expect("warning label");
    assert_eq!(buffer[warning].fg, Color::Yellow);
    let disabled = find_ascii_text(&buffer, "Delete remote history").expect("disabled label");
    assert_eq!(buffer[disabled].fg, Color::DarkGray);
    let description =
        find_ascii_text(&buffer, "Supporting copy for Keep session").expect("muted description");
    assert_eq!(buffer[description].fg, Color::DarkGray);
    let current = find_ascii_text(&buffer, "current").expect("current marker");
    assert_eq!(buffer[current].fg, Color::DarkGray);
}

#[test]
fn selected_destructive_items_keep_the_primary_focus_accent() {
    let mut state = consequence_state();
    state.select_key(&"delete");
    let buffer = rendered_buffer(
        &mut state,
        80,
        24,
        "Remove local session?",
        None,
        Theme::default(),
        Color::Blue,
    );

    let selected =
        find_ascii_text(&buffer, "Delete local transcript").expect("selected destructive");
    assert_eq!(buffer[selected].fg, Color::Cyan);
}

#[test]
fn shortcut_columns_and_mnemonic_modifiers_are_explicit_and_aligned() {
    let mut state = MenuState::try_new([
        MenuItem::new("resume", "Resume session")
            .description("Continue")
            .mnemonic('r')
            .number_shortcut(1),
        MenuItem::new("inspect", "Inspect transcript")
            .description("Read only")
            .mnemonic('i'),
        MenuItem::new("share", "Share session")
            .description("Share")
            .number_shortcut(3),
    ])
    .expect("valid shortcut menu");
    state.select_key(&"share");
    let buffer = rendered_buffer(
        &mut state,
        80,
        24,
        "Choose a session action",
        None,
        Theme::default(),
        Color::Blue,
    );

    let resume = find_ascii_text(&buffer, "Resume session").expect("resume label");
    let inspect = find_ascii_text(&buffer, "Inspect transcript").expect("inspect label");
    let share = find_ascii_text(&buffer, "Share session").expect("share label");
    assert_eq!(resume.0, inspect.0);
    assert_eq!(inspect.0, share.0);
    assert_eq!(buffer[(resume.0 - 3, resume.1)].symbol(), "1");
    assert_eq!(buffer[(inspect.0 - 3, inspect.1)].symbol(), " ");
    assert_eq!(buffer[(share.0 - 3, share.1)].symbol(), "3");
    assert!(buffer[resume].modifier.contains(Modifier::BOLD));
    assert!(buffer[inspect].modifier.contains(Modifier::BOLD));
    assert!(!buffer[share].modifier.contains(Modifier::BOLD));
    assert_eq!(state.items()[0].label(), "Resume session");
    assert_eq!(state.items()[1].label(), "Inspect transcript");
    assert_eq!(state.items()[2].label(), "Share session");
}

#[test]
fn fallback_theme_leaves_terminal_relative_backgrounds_unset() {
    let mut state = action_state();
    let buffer = rendered_buffer(
        &mut state,
        80,
        24,
        "Choose how to continue",
        None,
        Theme::default(),
        Color::Blue,
    );

    assert_eq!(buffer[(0, 0)].bg, Color::Blue);
    let title = find_ascii_text(&buffer, "Choose how to continue").expect("title");
    assert_eq!(buffer[title].bg, Color::Reset);
    let selected =
        find_ascii_text(&buffer, "Resume the selected transcript").expect("selected item");
    assert_eq!(buffer[selected].bg, Color::Reset);
}

#[test]
#[allow(clippy::disallowed_methods)]
fn caller_rect_backdrop_toggle_and_max_width_leave_outside_cells_untouched() {
    let theme = Theme::for_terminal_background(Some((20, 20, 20)));
    let mut state = action_state();
    let area = Rect::new(10, 5, 40, 12);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 60, 20));
    Block::default()
        .style(Style::new().bg(Color::Blue))
        .render(buffer.area, &mut buffer);
    StatefulWidget::render(
        OverlayMenu::new("Choose how to continue")
            .theme(theme)
            .max_width(24)
            .backdrop(false),
        area,
        &mut buffer,
        &mut state,
    );

    assert_eq!(buffer[(0, 0)].bg, Color::Blue);
    assert_eq!(buffer[(area.x, area.y)].bg, Color::Blue);
    let title = find_ascii_text(&buffer, "Choose how").expect("truncated title");
    assert_eq!(buffer[title].bg, Color::Rgb(38, 38, 38));
    let surface_cells = (buffer.area.x..buffer.area.right())
        .filter(|x| buffer[(*x, title.1)].bg == Color::Rgb(38, 38, 38))
        .count();
    assert_eq!(surface_cells, 24);
}

#[test]
fn one_row_inset_caller_rect_does_not_render_footer_outside_its_bounds() {
    let mut state = action_state();
    let area = Rect::new(10, 5, 40, 1);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 60, 20));
    for y in [area.y.saturating_sub(1), area.bottom()] {
        for x in area.x..area.right() {
            buffer[(x, y)]
                .set_symbol("x")
                .set_style(Style::new().bg(Color::Blue));
        }
    }

    StatefulWidget::render(
        OverlayMenu::new("Choose how to continue").key_hints(default_hints()),
        area,
        &mut buffer,
        &mut state,
    );

    for y in [area.y.saturating_sub(1), area.bottom()] {
        for x in area.x..area.right() {
            assert_eq!(buffer[(x, y)].symbol(), "x");
            assert_eq!(buffer[(x, y)].bg, Color::Blue);
        }
    }
}

#[test]
fn empty_disabled_and_descriptionless_menus_render_without_fake_selection() {
    let mut empty = MenuState::<&str>::try_new([]).expect("empty menu");
    let empty_render = snapshot(&mut empty, 40, 12, "No actions", None);
    assert!(empty_render.contains("No actions"));
    assert!(!empty_render.contains('▏'));

    let mut disabled = MenuState::try_new([
        MenuItem::new("first", "Unavailable").disabled(true),
        MenuItem::new("second", "Also unavailable").disabled(true),
    ])
    .expect("valid disabled menu");
    let disabled_render = snapshot(&mut disabled, 40, 12, "Actions", None);
    assert!(disabled_render.contains("Unavailable"));
    assert!(!disabled_render.contains('▏'));

    let mut descriptionless =
        MenuState::try_new([MenuItem::new("plain", "No description")]).expect("plain menu");
    let plain_render = snapshot(&mut descriptionless, 40, 12, "Actions", None);
    assert!(plain_render.contains("No description"));
    assert!(plain_render.contains('▏'));
}

#[test]
fn unicode_labels_use_display_width_in_narrow_layouts() {
    let mut unicode = MenuState::try_new([
        MenuItem::new(
            "resume",
            "Résumé 日本語 transcript with a very long structured label",
        )
        .description("Continue after café review with wrapped supporting copy"),
        MenuItem::new("new", "新しい session").description("Create a clean session"),
    ])
    .expect("valid unicode menu");
    assert_snapshot!(
        "menu_unicode_30x12",
        snapshot(&mut unicode, 30, 12, "続ける action", Some("Unicode width"))
    );
}

#[test]
fn zero_and_small_rectangles_are_safe() {
    for (width, height) in [(0, 0), (1, 1), (2, 3), (10, 2), (30, 7)] {
        let mut state = action_state();
        let area = Rect::new(4, 3, width, height);
        let mut buffer = Buffer::empty(area);
        StatefulWidget::render(
            OverlayMenu::new("Choose how to continue"),
            area,
            &mut buffer,
            &mut state,
        );
    }
}

fn snapshot(
    state: &mut MenuState<&'static str>,
    width: u16,
    height: u16,
    title: &'static str,
    subtitle: Option<&'static str>,
) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            let mut menu = OverlayMenu::new(title)
                .key_hints(default_hints())
                .theme(Theme::default());
            if let Some(subtitle) = subtitle {
                menu = menu.subtitle(subtitle);
            }
            frame.render_stateful_widget(menu, frame.area(), state);
        })
        .expect("draw menu");
    terminal.backend().to_string()
}

fn rendered_buffer(
    state: &mut MenuState<&'static str>,
    width: u16,
    height: u16,
    title: &'static str,
    subtitle: Option<&'static str>,
    theme: Theme,
    host_background: Color,
) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            Block::default()
                .style(Style::new().bg(host_background))
                .render(frame.area(), frame.buffer_mut());
            let mut menu = OverlayMenu::new(title)
                .key_hints(default_hints())
                .theme(theme);
            if let Some(subtitle) = subtitle {
                menu = menu.subtitle(subtitle);
            }
            frame.render_stateful_widget(menu, frame.area(), state);
        })
        .expect("draw menu");
    terminal.backend().buffer().clone()
}

fn default_hints() -> [KeyHint<'static>; 3] {
    [
        KeyHint::new("↑↓/jk", "move"),
        KeyHint::new("enter", "select"),
        KeyHint::new("q", "close"),
    ]
}

fn find_symbol_on_row(buffer: &Buffer, y: u16, symbol: &str) -> Option<(u16, u16)> {
    for x in buffer.area.x..buffer.area.right() {
        if buffer[(x, y)].symbol() == symbol {
            return Some((x, y));
        }
    }
    None
}

fn find_ascii_text(buffer: &Buffer, text: &str) -> Option<(u16, u16)> {
    assert!(text.is_ascii());
    let characters = text.chars().collect::<Vec<_>>();
    for y in buffer.area.y..buffer.area.bottom() {
        for x in buffer.area.x..buffer.area.right() {
            if x.saturating_add(characters.len() as u16) > buffer.area.right() {
                break;
            }
            if characters.iter().enumerate().all(|(offset, character)| {
                buffer[(x + offset as u16, y)].symbol() == character.to_string()
            }) {
                return Some((x, y));
            }
        }
    }
    None
}
