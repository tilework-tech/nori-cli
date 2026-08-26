use super::DetailStory;
use super::Page;
use super::overlay_menu_action;
use super::overlay_page_navigation;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use nori_tui_components::MenuAction;
use nori_tui_components::MenuShortcut;
use nori_tui_components::PickerAction;
use nori_tui_components::Theme;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;

#[test]
fn overlay_page_has_an_alternate_route_back_to_other_pages() {
    assert_eq!(
        overlay_page_navigation(Page::OverlayMenu, key(KeyCode::Left)),
        Some(Page::Details)
    );
    assert_eq!(
        overlay_page_navigation(Page::OverlayMenu, key(KeyCode::Right)),
        Some(Page::Picker)
    );
    assert_eq!(
        overlay_page_navigation(Page::Picker, key(KeyCode::Left)),
        None
    );
}

#[test]
fn overlay_page_translates_navigation_activation_and_shortcuts_before_page_keys() {
    let cases = [
        (KeyCode::Up, MenuAction::MoveUp),
        (KeyCode::Char('k'), MenuAction::MoveUp),
        (KeyCode::Down, MenuAction::MoveDown),
        (KeyCode::Char('J'), MenuAction::MoveDown),
        (KeyCode::Enter, MenuAction::ActivateSelected),
        (
            KeyCode::Char('1'),
            MenuAction::InvokeShortcut(MenuShortcut::Number(1)),
        ),
        (
            KeyCode::Char('5'),
            MenuAction::InvokeShortcut(MenuShortcut::Number(5)),
        ),
        (
            KeyCode::Char('R'),
            MenuAction::InvokeShortcut(MenuShortcut::Character('R')),
        ),
        (
            KeyCode::Char('s'),
            MenuAction::InvokeShortcut(MenuShortcut::Character('s')),
        ),
        (
            KeyCode::Char('i'),
            MenuAction::InvokeShortcut(MenuShortcut::Character('i')),
        ),
        (
            KeyCode::Char('a'),
            MenuAction::InvokeShortcut(MenuShortcut::Character('a')),
        ),
    ];

    for (code, expected) in cases {
        assert_eq!(
            overlay_menu_action(Page::OverlayMenu, key(code)),
            Some(expected)
        );
    }
    assert_eq!(
        overlay_menu_action(Page::Picker, key(KeyCode::Char('1'))),
        None
    );
}

#[test]
fn active_picker_search_owns_storybook_shortcut_characters() {
    assert!(super::picker_owns_global_shortcuts(Page::Picker, true));
    assert!(!super::picker_owns_global_shortcuts(Page::Picker, false));
    assert!(!super::picker_owns_global_shortcuts(Page::Markdown, true));

    for character in ['q', '1', '6', 'd', 'm', 's'] {
        assert_eq!(
            super::picker_action(key(KeyCode::Char(character)), true),
            Some(PickerAction::AppendQuery(character))
        );
    }
}

#[test]
fn detail_stories_cycle_through_each_configurable_presentation() {
    assert_eq!(DetailStory::default(), DetailStory::AutoWithHeading);

    let transitions = [
        (
            DetailStory::AutoWithHeading,
            DetailStory::Zebra,
            DetailStory::WithoutHeading,
        ),
        (
            DetailStory::Zebra,
            DetailStory::NormalDensity,
            DetailStory::AutoWithHeading,
        ),
        (
            DetailStory::NormalDensity,
            DetailStory::ResponsiveStacked,
            DetailStory::Zebra,
        ),
        (
            DetailStory::ResponsiveStacked,
            DetailStory::FixedWithHeading,
            DetailStory::NormalDensity,
        ),
        (
            DetailStory::FixedWithHeading,
            DetailStory::WithoutHeading,
            DetailStory::ResponsiveStacked,
        ),
        (
            DetailStory::WithoutHeading,
            DetailStory::AutoWithHeading,
            DetailStory::FixedWithHeading,
        ),
    ];

    for (current, next, previous) in transitions {
        assert_eq!(current.next(), next);
        assert_eq!(current.previous(), previous);
    }
}

#[test]
fn active_page_navigation_colors_only_its_compact_number() {
    let area = Rect::new(0, 0, 100, 1);
    let mut buffer = Buffer::empty(area);
    super::render_navigation(area, &mut buffer, Page::Picker, Theme::default());

    let active_number = find_ascii_text(&buffer, "1").expect("active page number");
    let active_label = find_ascii_text(&buffer, "Picker").expect("active page label");
    assert_eq!(buffer[active_number].fg, Color::Green);
    assert_eq!(buffer[active_label].fg, Color::Reset);
    assert!(buffer[active_label].modifier.contains(Modifier::BOLD));
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
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
