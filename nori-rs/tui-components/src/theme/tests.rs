use super::*;
use pretty_assertions::assert_eq;
use ratatui::style::Color;

#[test]
#[allow(clippy::disallowed_methods)]
fn default_theme_uses_close_semantic_surface_tokens() {
    let theme = Theme::default();

    assert_eq!(theme.text.fg, None);
    assert_eq!(theme.accent.fg, Some(Color::Cyan));
    assert_eq!(theme.surface.bg, None);
    assert_eq!(theme.input.bg, Some(Color::Indexed(235)));
    assert_eq!(theme.row.bg, Some(Color::Indexed(235)));
    assert_eq!(theme.row_alt.bg, Some(Color::Indexed(236)));
    assert_eq!(theme.selected.fg, Some(Color::Cyan));
    assert_eq!(theme.selected.bg, Some(Color::Indexed(237)));
    assert_eq!(theme.warning.fg, Some(Color::Yellow));
    assert_eq!(theme.error.fg, Some(Color::Red));
}
