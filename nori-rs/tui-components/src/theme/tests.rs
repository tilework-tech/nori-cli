use super::*;
use pretty_assertions::assert_eq;
use ratatui::style::Color;

#[test]
fn default_theme_uses_semantic_terminal_safe_tokens() {
    let theme = Theme::default();

    assert_eq!(theme.text.fg, None);
    assert_eq!(theme.accent.fg, Some(Color::Cyan));
    assert_eq!(theme.surface.bg, None);
    assert_eq!(theme.surface_alt.bg, Some(Color::DarkGray));
    assert_eq!(theme.selected.bg, Some(Color::Cyan));
    assert_eq!(theme.warning.fg, Some(Color::Yellow));
    assert_eq!(theme.error.fg, Some(Color::Red));
}
