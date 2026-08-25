use super::*;
use pretty_assertions::assert_eq;
use ratatui::style::Color;

#[test]
fn default_theme_separates_pointer_information_and_text_hierarchy() {
    let theme = Theme::default();

    assert_eq!(theme.text.fg, None);
    assert_eq!(theme.pointer.fg, Some(Color::Green));
    assert_eq!(theme.info.fg, Some(Color::Cyan));
    assert_eq!(theme.title.fg, None);
    assert!(
        theme
            .title
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    assert_eq!(theme.backdrop.bg, None);
    assert_eq!(theme.menu_surface.bg, None);
    assert_eq!(theme.menu_item_surface.bg, None);
    assert_eq!(theme.surface.bg, None);
    assert_eq!(theme.input.bg, None);
    assert_eq!(theme.row.bg, None);
    assert_eq!(theme.row_alt.bg, None);
    assert_eq!(theme.selected.fg, None);
    assert_eq!(theme.selected.bg, None);
    assert!(
        !theme
            .selected
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    assert_eq!(theme.warning.fg, Some(Color::Yellow));
    assert_eq!(theme.error.fg, Some(Color::Red));
}

#[test]
#[allow(clippy::disallowed_methods)]
fn terminal_theme_derives_close_surfaces_from_dark_background() {
    let theme = Theme::for_terminal_background(Some((20, 20, 20)));

    assert_eq!(theme.row.bg, Some(Color::Rgb(29, 29, 29)));
    assert_eq!(theme.backdrop.bg, Some(Color::Rgb(29, 29, 29)));
    assert_eq!(theme.menu_surface.bg, Some(Color::Rgb(38, 38, 38)));
    assert_eq!(theme.menu_item_surface.bg, Some(Color::Rgb(35, 35, 35)));
    assert_eq!(theme.row_alt.bg, Some(Color::Rgb(36, 36, 36)));
    assert_eq!(theme.input.bg, Some(Color::Rgb(38, 38, 38)));
    assert_eq!(theme.detail_surface.bg, Some(Color::Rgb(38, 38, 38)));
    assert_eq!(theme.selected.bg, Some(Color::Rgb(43, 43, 43)));
}

#[test]
#[allow(clippy::disallowed_methods)]
fn terminal_theme_derives_close_surfaces_from_light_background() {
    let theme = Theme::for_terminal_background(Some((240, 240, 240)));

    assert_eq!(theme.row.bg, Some(Color::Rgb(230, 230, 230)));
    assert_eq!(theme.backdrop.bg, Some(Color::Rgb(230, 230, 230)));
    assert_eq!(theme.menu_surface.bg, Some(Color::Rgb(220, 220, 220)));
    assert_eq!(theme.menu_item_surface.bg, Some(Color::Rgb(217, 217, 217)));
    assert_eq!(theme.row_alt.bg, Some(Color::Rgb(223, 223, 223)));
    assert_eq!(theme.input.bg, Some(Color::Rgb(220, 220, 220)));
    assert_eq!(theme.selected.bg, Some(Color::Rgb(216, 216, 216)));
}
