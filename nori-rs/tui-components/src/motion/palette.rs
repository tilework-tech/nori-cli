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

/// Prepared once per frame. Non-RGB styles retain the exact dimming threshold.
pub(super) struct PreparedPalette {
    ramps: [[Style; 17]; 2],
    rgb: [bool; 2],
}

impl PreparedPalette {
    pub fn style(&self, intensity: f64, highlight: bool) -> Style {
        let ramp = usize::from(highlight);
        let index = if self.rgb[ramp] {
            (intensity.clamp(0.0, 1.0) * 16.0).round() as usize
        } else if intensity < 0.3 {
            0
        } else {
            16
        };
        self.ramps[ramp][index]
    }
}

impl MotionPalette {
    pub(super) fn prepare(self) -> PreparedPalette {
        PreparedPalette {
            ramps: std::array::from_fn(|ramp| {
                std::array::from_fn(|level| self.style(level as f64 / 16.0, ramp == 1))
            }),
            rgb: [self.ink, self.highlight].map(|ink| {
                matches!(
                    (ink.fg, self.surface.bg),
                    (Some(Color::Rgb(..)), Some(Color::Rgb(..)))
                )
            }),
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn prepared_styles_preserve_custom_colors_modifiers_and_dimming_thresholds() {
        for palette in [
            MotionPalette::default(),
            MotionPalette::nori(),
            MotionPalette {
                surface: Style::new()
                    .bg(Color::Rgb(230, 220, 210))
                    .add_modifier(Modifier::ITALIC),
                ink: Style::new()
                    .fg(Color::Rgb(20, 30, 40))
                    .remove_modifier(Modifier::DIM),
                highlight: Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            },
        ] {
            let prepared = palette.prepare();
            for step in -10..=1010 {
                let intensity = f64::from(step) / 1000.0;
                for highlight in [false, true] {
                    assert_eq!(
                        prepared.style(intensity, highlight),
                        palette.style(intensity, highlight)
                    );
                }
            }
        }
    }
}
