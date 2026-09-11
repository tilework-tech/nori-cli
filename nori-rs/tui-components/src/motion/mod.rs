//! Deterministic animated scenery for caller-owned full-screen experiences.
//!
//! Render this layer first, then render onboarding, credentials, or other content
//! in a [`MotionBackground::quiet_area`]. The caller owns the alternate screen,
//! clock, redraw scheduling, input, and all authentication behavior.

mod fields;

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::widgets::Widget;

use crate::Theme;

/// Optional fixed destination for the dot-to-braille zoom.
/// `Cycle` retains the original time-driven formation sequence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MotionFormation {
    #[default]
    Cycle,
    Platoon,
    Muster,
}

/// The five destinations in the motion suite, in visual transition order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MotionScene {
    #[default]
    Dots,
    Formations,
    Sandboxes,
    Creatures,
    Blueprint,
}

impl MotionScene {
    pub const ALL: [Self; 5] = [
        Self::Dots,
        Self::Formations,
        Self::Sandboxes,
        Self::Creatures,
        Self::Blueprint,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dots => "Dots",
            Self::Formations => "Formations",
            Self::Sandboxes => "Sandboxes",
            Self::Creatures => "Creatures",
            Self::Blueprint => "Blueprint",
        }
    }

    fn position(self) -> f64 {
        match self {
            Self::Dots => 0.0,
            Self::Formations => 1.0,
            Self::Sandboxes => 2.0,
            Self::Creatures => 3.0,
            Self::Blueprint => 4.0,
        }
    }
}

/// Caller-held animation clock and interruptible scene transition.
///
/// Adjacent scenes take three seconds to morph. No wall clock, background task,
/// random source, or input handling is hidden in this state. Pause by not calling
/// [`Self::advance`]. Reduced motion freezes scenery and switches scenes instantly.
#[derive(Clone, Debug, Default)]
pub struct MotionState {
    scene: MotionScene,
    position: f64,
    elapsed: Duration,
    reduced_motion: bool,
}

impl MotionState {
    pub fn new(scene: MotionScene) -> Self {
        Self {
            scene,
            position: scene.position(),
            ..Self::default()
        }
    }

    /// The requested destination, including while a transition is in progress.
    pub fn scene(&self) -> MotionScene {
        self.scene
    }

    pub fn set_scene(&mut self, scene: MotionScene) {
        self.scene = scene;
        if self.reduced_motion {
            self.position = scene.position();
        }
    }

    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.reduced_motion = reduced;
        if reduced {
            self.position = self.scene.position();
        }
    }

    pub fn reduced_motion(&self) -> bool {
        self.reduced_motion
    }

    pub fn advance(&mut self, delta: Duration) {
        if self.reduced_motion {
            return;
        }
        self.elapsed = self.elapsed.saturating_add(delta);
        let distance = self.scene.position() - self.position;
        self.position += distance.signum() * distance.abs().min(delta.as_secs_f64() / 3.0);
    }
}

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
    fn style(self, intensity: f64, highlight: bool) -> Style {
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

/// Background widget with an optional blank content region and soft outer falloff.
///
/// Work is bounded by the visible cell count, with at most eight field samples per
/// cell. Choose a cadence using measurements at the intended viewport. Unicode assumes a font with
/// single-cell braille and geometric symbols; use [`Self::ascii`] for a fallback.
#[derive(Clone, Debug)]
pub struct MotionBackground<'a> {
    state: &'a MotionState,
    palette: MotionPalette,
    quiet_area: Option<Rect>,
    ascii: bool,
    formation: MotionFormation,
}

impl<'a> MotionBackground<'a> {
    pub fn new(state: &'a MotionState) -> Self {
        Self {
            state,
            palette: MotionPalette::default(),
            quiet_area: None,
            ascii: false,
            formation: MotionFormation::Cycle,
        }
    }

    pub fn palette(mut self, palette: MotionPalette) -> Self {
        self.palette = palette;
        self
    }

    /// Absolute terminal coordinates. This region is cleared, not preserved;
    /// foreground widgets must be rendered after the background on each frame.
    pub fn quiet_area(mut self, area: Rect) -> Self {
        self.quiet_area = Some(area);
        self
    }

    pub fn ascii(mut self, enabled: bool) -> Self {
        self.ascii = enabled;
        self
    }

    /// Pin a formation for repeatable zooms instead of the ambient cycle.
    pub fn formation(mut self, formation: MotionFormation) -> Self {
        self.formation = formation;
        self
    }
}

impl Widget for MotionBackground<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Keep coordinates relative to the original area when the buffer clips it.
        let visible = area.intersection(buf.area);
        // Bound the floating-point clock even for a caller supplying Duration::MAX.
        let time = (self.state.elapsed.as_secs() % 86_400) as f64
            + f64::from(self.state.elapsed.subsec_nanos()) / 1e9;
        let field = fields::Field::new(area, time, self.state.position, self.formation);
        let quiet = self.quiet_area.filter(|rect| !rect.is_empty());
        for y in visible.top()..visible.bottom() {
            for x in visible.left()..visible.right() {
                let cell = &mut buf[(x, y)];
                cell.reset();
                cell.set_style(self.palette.surface);
                let fade = quiet.map_or(1.0, |rect| {
                    let dx = (i32::from(rect.left()) - i32::from(x))
                        .max(i32::from(x) - i32::from(rect.right()) + 1)
                        .max(0);
                    let dy = (i32::from(rect.top()) - i32::from(y))
                        .max(i32::from(y) - i32::from(rect.bottom()) + 1)
                        .max(0);
                    fields::smooth((f64::from(dx).hypot(f64::from(dy) * 2.0)) / 6.0)
                });
                if fade == 0.0 {
                    continue;
                }
                let sample = field.cell(i32::from(x - area.x), i32::from(y - area.y), self.ascii);
                if sample.glyph == ' ' || sample.intensity * fade < 0.035 {
                    continue;
                }
                cell.set_char(sample.glyph);
                cell.set_style(
                    self.palette
                        .style(sample.intensity * fade, sample.highlight),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
