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
    pub accent: Style,
    pub surface: Style,
    pub surface_alt: Style,
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
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            text: Style::new(),
            muted: Style::new().fg(Color::DarkGray),
            accent: Style::new().fg(Color::Cyan),
            surface: Style::new(),
            surface_alt: Style::new().bg(Color::DarkGray),
            detail_surface: Style::new().bg(Color::DarkGray),
            selected: Style::new()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            disabled: Style::new().fg(Color::DarkGray),
            border: Style::new().fg(Color::DarkGray),
            separator: Style::new().fg(Color::DarkGray),
            title: Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
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
        }
    }
}

#[cfg(test)]
mod tests;
