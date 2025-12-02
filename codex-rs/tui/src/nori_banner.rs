//! Nori welcome banner widget with ASCII art and status line.
//!
//! This module provides a self-contained banner widget that displays:
//! - Green ANSI-colored ASCII art for "NORI"
//! - A status line with profile name and tagline

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::WidgetRef;

/// ASCII art for "NORI" logo
const NORI_ASCII_ART: &[&str] = &[
    r" _   _  ___  ____  ___ ",
    r"| \ | |/ _ \|  _ \|_ _|",
    r"|  \| | | | | |_) || | ",
    r"| |\  | |_| |  _ < | | ",
    r"|_| \_|\___/|_| \_\___|",
];

/// A welcome banner widget displaying the Nori ASCII art and status line.
pub struct NoriBanner {
    profile: String,
}

impl NoriBanner {
    /// Creates a new NoriBanner with the given profile name.
    pub fn new(profile: impl Into<String>) -> Self {
        Self {
            profile: profile.into(),
        }
    }

    /// Renders the banner lines with styling applied.
    fn render_lines(&self) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();

        // Add ASCII art lines in green
        for art_line in NORI_ASCII_ART {
            lines.push(Line::from(Span::styled(
                art_line.to_string(),
                ratatui::style::Style::default().fg(Color::Green),
            )));
        }

        // Add empty line for spacing
        lines.push(Line::from(""));

        // Add profile line
        lines.push(Line::from(format!("profile: {}", self.profile)));

        // Add tagline
        lines.push(Line::from("🍙 powered by Nori 🍙"));

        lines
    }
}

impl WidgetRef for &NoriBanner {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let lines = self.render_lines();
        Paragraph::new(lines).render(area, buf);
    }
}

#[cfg(all(test, feature = "vt100-tests"))]
mod tests {
    use super::*;
    use crate::test_backend::VT100Backend;
    use ratatui::Terminal;

    #[test]
    fn nori_banner_renders_ascii_art() {
        let banner = NoriBanner::new("clifford");
        let mut terminal =
            Terminal::new(VT100Backend::new(80, 12)).expect("Failed to create terminal");
        terminal
            .draw(|f| (&banner).render_ref(f.area(), f.buffer_mut()))
            .expect("Failed to draw");

        let contents = terminal.backend().to_string();

        // Verify ASCII art is present (check for distinctive parts of the art)
        assert!(
            contents.contains(r"|_| \_|"),
            "Banner should contain ASCII art for NORI logo"
        );
        assert!(
            contents.contains(r"| \ | |"),
            "Banner should contain ASCII art characters"
        );

        insta::assert_snapshot!("nori_banner_ascii_art", terminal.backend().to_string());
    }

    #[test]
    fn nori_banner_shows_profile_and_tagline() {
        let banner = NoriBanner::new("test-profile");
        let mut terminal =
            Terminal::new(VT100Backend::new(80, 12)).expect("Failed to create terminal");
        terminal
            .draw(|f| (&banner).render_ref(f.area(), f.buffer_mut()))
            .expect("Failed to draw");

        let contents = terminal.backend().to_string();

        assert!(
            contents.contains("profile: test-profile"),
            "Banner should show profile name"
        );
        assert!(
            contents.contains("🍙 powered by Nori 🍙"),
            "Banner should show tagline"
        );

        insta::assert_snapshot!(
            "nori_banner_profile_tagline",
            terminal.backend().to_string()
        );
    }

    #[test]
    fn nori_banner_uses_green_color() {
        let banner = NoriBanner::new("clifford");
        let lines = banner.render_lines();

        // Check that the first line (ASCII art) has green styling
        let first_line = &lines[0];
        assert!(!first_line.spans.is_empty(), "First line should have spans");

        let first_span = &first_line.spans[0];
        assert_eq!(
            first_span.style.fg,
            Some(Color::Green),
            "ASCII art should be styled with green color"
        );
    }
}
