use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;

/// Semantic styling tokens used by every component in this crate.
///
/// Applications can provide a different theme without changing component
/// state or behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub text: Style,
    pub muted: Style,
    /// Compact focus and interaction indicators such as pointers, rails, and shortcuts.
    pub pointer: Style,
    pub backdrop: Style,
    pub menu_surface: Style,
    pub menu_item_surface: Style,
    pub surface: Style,
    pub input: Style,
    pub row: Style,
    pub row_alt: Style,
    pub detail_surface: Style,
    pub selected: Style,
    pub disabled: Style,
    pub border: Style,
    pub separator: Style,
    pub title: Style,
    pub info: Style,
    pub success: Style,
    pub warning: Style,
    pub error: Style,
    pub code: Style,
    pub link: Style,
    pub table_header: Style,
    pub table_rule: Style,
    pub provider_claude: Style,
    pub provider_codex: Style,
    pub provider_gemini: Style,
    pub provider_antigravity: Style,
    pub provider_nori: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            text: Style::new(),
            muted: Style::new().fg(Color::DarkGray),
            pointer: Style::new().fg(Color::Green),
            backdrop: Style::new(),
            menu_surface: Style::new(),
            menu_item_surface: Style::new(),
            surface: Style::new(),
            input: Style::new(),
            row: Style::new(),
            row_alt: Style::new(),
            detail_surface: Style::new(),
            selected: Style::new(),
            disabled: Style::new().fg(Color::DarkGray),
            border: Style::new().fg(Color::DarkGray),
            separator: Style::new().fg(Color::DarkGray),
            title: Style::new().add_modifier(Modifier::BOLD),
            info: Style::new().fg(Color::Cyan),
            success: Style::new().fg(Color::Green),
            warning: Style::new().fg(Color::Yellow),
            error: Style::new().fg(Color::Red),
            code: Style::new().fg(Color::Cyan),
            link: Style::new()
                .fg(Color::Cyan)
                .add_modifier(Modifier::UNDERLINED),
            table_header: Style::new().add_modifier(Modifier::BOLD),
            table_rule: Style::new().fg(Color::DarkGray),
            provider_claude: Style::new().fg(Color::Yellow),
            provider_codex: Style::new().fg(Color::White),
            provider_gemini: Style::new().fg(Color::Blue),
            provider_antigravity: Style::new().fg(Color::Blue),
            provider_nori: Style::new().fg(Color::Green),
        }
    }
}

impl Theme {
    /// Builds the default theme with neutral surfaces derived from the terminal background.
    ///
    /// Consumers should pass a background only when the terminal reports both its RGB
    /// background and true-color support. `None` deliberately leaves every neutral
    /// background unset rather than substituting fixed palette indices.
    #[allow(clippy::disallowed_methods)]
    pub fn for_terminal_background(terminal_bg: Option<(u8, u8, u8)>) -> Self {
        let mut theme = Self::default();
        let Some(background) = terminal_bg else {
            return theme;
        };

        theme.backdrop = theme.backdrop.bg(relative_surface(background, 4));
        let menu_surface = relative_surface(background, 8);
        theme.menu_surface = theme.menu_surface.bg(menu_surface);
        theme.menu_item_surface = theme.menu_item_surface.bg(darken_surface(menu_surface, 3));
        theme.row = theme.row.bg(relative_surface(background, 4));
        theme.row_alt = theme.row_alt.bg(relative_surface(background, 7));
        theme.input = theme.input.bg(relative_surface(background, 8));
        theme.detail_surface = theme.detail_surface.bg(relative_surface(background, 8));
        theme.selected = theme.selected.bg(relative_surface(background, 10));
        theme
    }
}

#[allow(clippy::disallowed_methods)]
fn darken_surface(surface: Color, amount: u8) -> Color {
    match surface {
        Color::Rgb(red, green, blue) => Color::Rgb(
            red.saturating_sub(amount),
            green.saturating_sub(amount),
            blue.saturating_sub(amount),
        ),
        other => other,
    }
}

#[allow(clippy::disallowed_methods)]
fn relative_surface(background: (u8, u8, u8), strength: u16) -> Color {
    let target = if is_light(background) { 0 } else { 255 };
    Color::Rgb(
        mix_channel(background.0, target, strength),
        mix_channel(background.1, target, strength),
        mix_channel(background.2, target, strength),
    )
}

fn is_light((red, green, blue): (u8, u8, u8)) -> bool {
    let luma = u32::from(red) * 299 + u32::from(green) * 587 + u32::from(blue) * 114;
    luma > 128_000
}

fn mix_channel(channel: u8, target: u8, strength: u16) -> u8 {
    let mixed = (u16::from(channel) * (100 - strength) + u16::from(target) * strength) / 100;
    u8::try_from(mixed).unwrap_or(target)
}

#[cfg(test)]
mod tests;
