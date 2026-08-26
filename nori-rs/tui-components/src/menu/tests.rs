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
    assert!(page_down.contains("› Three"));
    assert!(page_down.contains('↑'));
    assert!(page_down.contains('↓'));
    assert!(state.viewport_offset() > 0);

    assert_eq!(
        state.handle(MenuAction::PageUp),
        MenuOutcome::SelectionChanged(Some("one"))
    );
    let page_up = snapshot(&mut state, 30, 7, "Choose", None);
    assert!(page_up.contains("› One"));
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
fn menu_80x24_dense_zebra_snapshot() {
    let mut action = action_state();
    assert_snapshot!(
        "menu_80x24_dense_zebra",
        snapshot_with_presentation(
            &mut action,
            80,
            24,
            "Choose how to continue",
            Some("Dense spacing with alternating item surfaces"),
            MenuDensity::Dense,
            MenuRowPattern::Zebra,
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
    assert_eq!(buffer[title].fg, Color::Reset);
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
fn default_selection_uses_a_surface_and_pointer_without_edge_rails() {
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
    let pointer = find_symbol_on_row(&buffer, selected.1, "›").expect("selection pointer");
    assert_eq!(buffer[pointer].fg, Color::Green);
    assert_eq!(buffer[selected].fg, Color::Reset);
    let description =
        find_ascii_text(&buffer, "Supporting copy for Keep session").expect("description");
    assert_eq!(buffer[description].fg, Color::DarkGray);
    for row in selected.1..=description.1 {
        assert_eq!(find_symbol_on_row(&buffer, row, "▏"), None);
        assert_eq!(find_symbol_on_row(&buffer, row, "▕"), None);
    }
    for cell in [pointer, selected, description] {
        assert_eq!(buffer[cell].bg, Color::Rgb(43, 43, 43));
    }
}

#[test]
#[allow(clippy::disallowed_methods)]
fn fullscreen_selection_rails_are_explicit_and_replace_the_pointer() {
    let theme = Theme::for_terminal_background(Some((20, 20, 20)));
    let mut state = consequence_state();
    let buffer = rendered_buffer_with_selection_rails(
        &mut state,
        80,
        24,
        "Remove local session?",
        None,
        theme,
        Color::Blue,
    );

    let selected = find_ascii_text(&buffer, "Keep session").expect("selected label");
    let description =
        find_ascii_text(&buffer, "Supporting copy for Keep session").expect("description");
    for row in selected.1..=description.1 {
        let left = find_symbol_on_row(&buffer, row, "▏").expect("left selection rail");
        let right = find_symbol_on_row(&buffer, row, "▕").expect("right selection rail");
        assert_eq!(buffer[left].fg, Color::Green);
        assert_eq!(buffer[right].fg, Color::Green);
        assert_eq!(find_symbol_on_row(&buffer, row, "›"), None);
    }
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
fn selected_destructive_items_move_color_to_the_pointer() {
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
    assert_eq!(buffer[selected].fg, Color::Reset);
    let pointer = find_symbol_on_row(&buffer, selected.1, "›").expect("selection pointer");
    assert_eq!(buffer[pointer].fg, Color::Green);
}

#[test]
#[allow(clippy::disallowed_methods)]
fn shortcut_columns_align_numbers_and_bold_unselected_mnemonics() {
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
    let theme = Theme::for_terminal_background(Some((20, 20, 20)));
    let buffer = rendered_buffer(
        &mut state,
        80,
        24,
        "Choose a session action",
        None,
        theme,
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
    assert_eq!(buffer[(resume.0 - 3, resume.1)].fg, Color::Green);
    assert_eq!(buffer[resume].fg, Color::Reset);
    assert_eq!(buffer[(resume.0 + 1, resume.1)].fg, Color::Reset);
    assert_eq!(buffer[(share.0 - 3, share.1)].fg, Color::Green);
    assert!(buffer[resume].modifier.contains(Modifier::BOLD));
    assert!(buffer[inspect].modifier.contains(Modifier::BOLD));
    assert!(!buffer[share].modifier.contains(Modifier::BOLD));
}

#[test]
fn selected_mnemonics_keep_label_color_while_numbers_stay_green() {
    let mut state = MenuState::try_new([MenuItem::new("resume", "Resume session")
        .mnemonic('r')
        .number_shortcut(1)])
    .expect("valid selected shortcut menu");
    let selected_buffer = rendered_buffer(
        &mut state,
        80,
        24,
        "Choose a session action",
        None,
        Theme::default(),
        Color::Blue,
    );
    let selected_resume =
        find_ascii_text(&selected_buffer, "Resume session").expect("selected resume label");
    assert_eq!(selected_buffer[selected_resume].fg, Color::Reset);
    assert!(
        selected_buffer[selected_resume]
            .modifier
            .contains(Modifier::BOLD)
    );
    assert_eq!(
        selected_buffer[(selected_resume.0 - 3, selected_resume.1)].fg,
        Color::Green
    );
}

#[test]
fn semantic_mnemonics_bold_the_existing_consequence_tone() {
    let mut destructive_state = MenuState::try_new([
        MenuItem::new("keep", "Keep session"),
        MenuItem::new("delete", "Delete session")
            .mnemonic('d')
            .tone(MenuItemTone::Destructive),
    ])
    .expect("valid semantic shortcut menu");
    let semantic_buffer = rendered_buffer(
        &mut destructive_state,
        80,
        24,
        "Choose a session action",
        None,
        Theme::default(),
        Color::Blue,
    );
    let destructive =
        find_ascii_text(&semantic_buffer, "Delete session").expect("destructive label");
    assert_eq!(semantic_buffer[destructive].fg, Color::Red);
    assert!(
        semantic_buffer[destructive]
            .modifier
            .contains(Modifier::BOLD)
    );
}

#[test]
#[allow(clippy::disallowed_methods)]
fn dense_menu_keeps_item_anatomy_without_inter_item_rows_and_tightens_padding() {
    let theme = Theme {
        menu_surface: Style::new().bg(Color::Cyan),
        menu_item_surface: Style::new().bg(Color::Black),
        selected: Style::new().bg(Color::Magenta),
        ..Theme::default()
    };
    let mut normal_state = action_state();
    let normal = rendered_buffer_with_presentation(
        &mut normal_state,
        80,
        24,
        theme,
        MenuDensity::Normal,
        MenuRowPattern::Plain,
    );
    let mut dense_state = action_state();
    let dense = rendered_buffer_with_presentation(
        &mut dense_state,
        80,
        24,
        theme,
        MenuDensity::Dense,
        MenuRowPattern::Plain,
    );

    let normal_first = find_ascii_text(&normal, "Resume the selected transcript").expect("row");
    let normal_second = find_ascii_text(&normal, "Start a new session").expect("row");
    let dense_first = find_ascii_text(&dense, "Resume the selected transcript").expect("row");
    let dense_second = find_ascii_text(&dense, "Start a new session").expect("row");
    assert_eq!(normal_second.1 - normal_first.1, 3);
    assert_eq!(dense_second.1 - dense_first.1, 2);
    assert!(dense_first.0 < normal_first.0);
    assert_eq!(
        find_ascii_text(&dense, "Continue the selected transcript")
            .expect("dense description")
            .1,
        dense_first.1 + 1
    );
    assert_eq!(
        selected_surface_bounds(&normal, normal_first.1, Color::Magenta),
        Some((normal_first.0 - 2, normal_first.0 + 51))
    );
    assert_eq!(
        selected_surface_bounds(&dense, dense_first.1, Color::Magenta),
        Some((dense_first.0 - 2, dense_first.0 + 53))
    );
    let normal_title = find_ascii_text(&normal, "Choose how to continue").expect("normal title");
    let dense_title = find_ascii_text(&dense, "Choose how to continue").expect("dense title");
    assert_eq!(
        normal_title.1,
        menu_surface_top(&normal, normal_title.0, Color::Cyan) + 1
    );
    assert_eq!(
        dense_title.1,
        menu_surface_top(&dense, dense_title.0, Color::Cyan)
    );
}

#[test]
#[allow(clippy::disallowed_methods)]
fn dense_menu_zebra_surfaces_preserve_selection_and_disabled_precedence() {
    let theme = Theme {
        menu_surface: Style::new().bg(Color::Blue),
        menu_item_surface: Style::new().bg(Color::Black),
        menu_item_surface_alt: Style::new().bg(Color::Yellow),
        selected: Style::new().bg(Color::Magenta),
        ..Theme::default()
    };
    let items = [
        basic_item("first", "First action"),
        basic_item("selected", "Selected alternate action"),
        basic_item("plain", "Plain action"),
        basic_item("alternate", "Alternate action"),
        basic_item("disabled", "Disabled action").disabled(true),
    ];
    let mut state = MenuState::try_new(items.clone()).expect("valid zebra menu");
    state.select_key(&"selected");
    let zebra = rendered_buffer_with_presentation(
        &mut state,
        80,
        24,
        theme,
        MenuDensity::Dense,
        MenuRowPattern::Zebra,
    );

    let first = find_ascii_text(&zebra, "First action").expect("first row");
    let selected =
        find_ascii_text(&zebra, "Selected alternate action").expect("selected alternate row");
    let plain = find_ascii_text(&zebra, "Plain action").expect("plain row");
    let alternate = find_ascii_text(&zebra, "Alternate action").expect("alternate row");
    let disabled = find_ascii_text(&zebra, "Disabled action").expect("disabled row");
    assert_eq!(zebra[first].bg, Color::Black);
    assert_eq!(zebra[selected].bg, Color::Magenta);
    assert_eq!(zebra[plain].bg, Color::Black);
    assert_eq!(zebra[alternate].bg, Color::Yellow);
    assert_eq!(zebra[disabled].bg, Color::Blue);

    let mut plain_state = MenuState::try_new(items).expect("valid plain menu");
    plain_state.select_key(&"selected");
    let plain_menu = rendered_buffer_with_presentation(
        &mut plain_state,
        80,
        24,
        theme,
        MenuDensity::Dense,
        MenuRowPattern::Plain,
    );
    let unselected_alternate =
        find_ascii_text(&plain_menu, "Alternate action").expect("plain alternate row");
    assert_eq!(plain_menu[unselected_alternate].bg, Color::Black);
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
    assert!(plain_render.contains('›'));
    assert!(!plain_render.contains('▏'));
    assert!(!plain_render.contains('▕'));
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

fn snapshot_with_presentation(
    state: &mut MenuState<&'static str>,
    width: u16,
    height: u16,
    title: &'static str,
    subtitle: Option<&'static str>,
    density: MenuDensity,
    row_pattern: MenuRowPattern,
) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            let mut menu = OverlayMenu::new(title)
                .key_hints(default_hints())
                .theme(Theme::default())
                .density(density)
                .row_pattern(row_pattern);
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
    rendered_buffer_with_options(
        state,
        width,
        height,
        title,
        subtitle,
        theme,
        host_background,
        false,
    )
}

fn rendered_buffer_with_selection_rails(
    state: &mut MenuState<&'static str>,
    width: u16,
    height: u16,
    title: &'static str,
    subtitle: Option<&'static str>,
    theme: Theme,
    host_background: Color,
) -> Buffer {
    rendered_buffer_with_options(
        state,
        width,
        height,
        title,
        subtitle,
        theme,
        host_background,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn rendered_buffer_with_options(
    state: &mut MenuState<&'static str>,
    width: u16,
    height: u16,
    title: &'static str,
    subtitle: Option<&'static str>,
    theme: Theme,
    host_background: Color,
    fullscreen_selection_rails: bool,
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
                .theme(theme)
                .fullscreen_selection_rails(fullscreen_selection_rails);
            if let Some(subtitle) = subtitle {
                menu = menu.subtitle(subtitle);
            }
            frame.render_stateful_widget(menu, frame.area(), state);
        })
        .expect("draw menu");
    terminal.backend().buffer().clone()
}

fn rendered_buffer_with_presentation(
    state: &mut MenuState<&'static str>,
    width: u16,
    height: u16,
    theme: Theme,
    density: MenuDensity,
    row_pattern: MenuRowPattern,
) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            Block::default()
                .style(Style::new().bg(Color::Blue))
                .render(frame.area(), frame.buffer_mut());
            let menu = OverlayMenu::new("Choose how to continue")
                .key_hints(default_hints())
                .theme(theme)
                .density(density)
                .row_pattern(row_pattern);
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

fn selected_surface_bounds(
    buffer: &Buffer,
    y: u16,
    selected_background: Color,
) -> Option<(u16, u16)> {
    let xs = (buffer.area.x..buffer.area.right())
        .filter(|x| buffer[(*x, y)].bg == selected_background)
        .collect::<Vec<_>>();
    Some((*xs.first()?, *xs.last()?))
}

fn menu_surface_top(buffer: &Buffer, x: u16, menu_background: Color) -> u16 {
    (buffer.area.y..buffer.area.bottom())
        .find(|y| buffer[(x, *y)].bg == menu_background)
        .expect("menu surface")
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
