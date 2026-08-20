use crate::color::blend;
use crate::color::is_light;
use crate::terminal_palette::default_bg;
use nori_tui_components::Theme;
use ratatui::style::Color;
use ratatui::style::Style;

pub fn user_message_style() -> Style {
    user_message_style_for(relative_terminal_background())
}

pub(crate) fn component_theme() -> Theme {
    Theme::for_terminal_background(relative_terminal_background())
}

/// Returns the style for a user-authored message using the provided terminal background.
pub fn user_message_style_for(terminal_bg: Option<(u8, u8, u8)>) -> Style {
    match terminal_bg {
        Some(bg) => Style::default().bg(user_message_bg(bg)),
        None => Style::default(),
    }
}

#[allow(clippy::disallowed_methods)]
pub fn user_message_bg(terminal_bg: (u8, u8, u8)) -> Color {
    let top = if is_light(terminal_bg) {
        (0, 0, 0)
    } else {
        (255, 255, 255)
    };
    let (red, green, blue) = blend(top, terminal_bg, 0.1);
    Color::Rgb(red, green, blue)
}

fn relative_terminal_background() -> Option<(u8, u8, u8)> {
    supports_color::on_cached(supports_color::Stream::Stdout)
        .filter(|level| level.has_16m)
        .and_then(|_| default_bg())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_style_has_no_absolute_fallback() {
        assert_eq!(user_message_style_for(None).bg, None);
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn user_message_style_blends_against_dark_and_light_backgrounds() {
        assert_eq!(
            user_message_style_for(Some((20, 20, 20))).bg,
            Some(Color::Rgb(43, 43, 43))
        );
        assert_eq!(
            user_message_style_for(Some((240, 240, 240))).bg,
            Some(Color::Rgb(216, 216, 216))
        );
    }
}
