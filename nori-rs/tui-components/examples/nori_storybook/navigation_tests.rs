use super::Page;
use super::overlay_menu_action;
use super::overlay_page_navigation;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use nori_tui_components::MenuAction;
use nori_tui_components::MenuShortcut;
use pretty_assertions::assert_eq;

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

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}
