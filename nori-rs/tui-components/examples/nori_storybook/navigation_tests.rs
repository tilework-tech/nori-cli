use super::Page;
use super::overlay_page_navigation;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

#[test]
fn overlay_page_has_an_alternate_route_back_to_other_pages() {
    assert_eq!(
        overlay_page_navigation(Page::OverlayMenu, key(KeyCode::Left)),
        Some(Page::States)
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

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}
