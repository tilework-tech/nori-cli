//! Scenery colors independent of animation timing and field geometry.

use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;

use crate::Theme;

/// Explicit scenery styles, separate from the semantic colors of foreground UI.
#[derive(Clone, Copy, Debug)]
pub struct MotionPalette {
    pub surface: Style,
    pub ink: Style,
    pub highlight: Style,
}

impl MotionPalette {
    /// Terminal-compatible default; never invents a background color.
    pub fn from_theme(theme: Theme) -> Self {
        Self {
            surface: theme.surface,
            ink: theme.muted,
            highlight: theme.muted,
        }
    }

    /// Opt-in, true-color art direction from the Nori Motion Suite mockup.
    /// The caller must also give foreground content suitable contrasting colors.
    #[allow(clippy::disallowed_methods)]
    pub fn nori() -> Self {
        Self {
            surface: Style::new().bg(Color::Rgb(10, 15, 12)),
            ink: Style::new().fg(Color::Rgb(125, 214, 160)),
            highlight: Style::new().fg(Color::Rgb(214, 255, 224)),
        }
    }

    #[allow(clippy::disallowed_methods)]
    pub(super) fn style(self, intensity: f64, highlight: bool) -> Style {
        let ink = if highlight { self.highlight } else { self.ink };
        let mut style = self.surface.patch(ink);
        if let (Some(Color::Rgb(r, g, b)), Some(Color::Rgb(br, bg, bb))) = (ink.fg, self.surface.bg)
        {
            // Quantization keeps subtle field changes from repainting every cell.
            let amount = (intensity.clamp(0.0, 1.0) * 16.0).round() / 16.0;
            let mix = |base, top| (f64::from(base) + f64::from(top - base) * amount) as u8;
            style = style.fg(Color::Rgb(
                mix(i32::from(br), i32::from(r)),
                mix(i32::from(bg), i32::from(g)),
                mix(i32::from(bb), i32::from(b)),
            ));
        } else if intensity < 0.3 {
            style = style.add_modifier(Modifier::DIM);
        }
        style
    }
}

impl Default for MotionPalette {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}
