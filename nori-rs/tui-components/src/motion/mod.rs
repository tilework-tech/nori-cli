//! Deterministic animated scenery for caller-owned full-screen experiences.
//!
//! Render this layer first, then render onboarding, credentials, or other content
//! in a [`MotionBackground::quiet_area`]. The caller owns the alternate screen,
//! clock, redraw scheduling, input, and all authentication behavior.

mod fields;
mod palette;
mod state;

pub use palette::MotionPalette;
pub use state::MotionState;

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

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

/// Background widget with an optional blank content region and soft outer falloff.
///
/// Work is bounded by the visible cell count, with at most eight field samples per
/// cell. Choose a cadence using measurements at the intended viewport. Unicode
/// assumes single-cell braille and geometric symbols; use [`Self::ascii`] for a
/// fallback. Timing and transition progress belong to the caller. [`MotionState`]
/// is an optional controller for simple linear transitions.
///
/// ```
/// use std::time::Duration;
/// use nori_tui_components::{MotionBackground, MotionScene, MotionState};
/// use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
///
/// let area = Rect::new(0, 0, 80, 24);
/// let mut buffer = Buffer::empty(area);
/// MotionBackground::new(MotionScene::Dots, Duration::from_secs(12))
///     .transition_to(MotionScene::Formations, 0.8)
///     .render(area, &mut buffer);
///
/// let mut controller = MotionState::new(MotionScene::Dots);
/// controller.set_scene(MotionScene::Formations);
/// controller.advance(Duration::from_millis(40));
/// controller.background().render(area, &mut buffer);
/// ```
#[derive(Clone, Debug)]
pub struct MotionBackground {
    scene: MotionScene,
    elapsed: Duration,
    position: f64,
    reduced_motion: bool,
    palette: MotionPalette,
    quiet_area: Option<Rect>,
    ascii: bool,
    formation: MotionFormation,
    sandbox_zoom: Option<f64>,
}

impl MotionBackground {
    /// Render a scene at explicit ambient time, without a transition controller.
    pub fn new(scene: MotionScene, elapsed: Duration) -> Self {
        Self {
            scene,
            elapsed,
            position: scene.position(),
            reduced_motion: false,
            palette: MotionPalette::default(),
            quiet_area: None,
            ascii: false,
            formation: MotionFormation::Cycle,
            sandbox_zoom: None,
        }
    }

    /// Continuously zoom out from the sandbox pulse grid to the braille field.
    /// Unlike the suite's scene crossfade, this shrinks the sandbox geometry and
    /// blends into the destination field without a black midpoint. The caller
    /// owns duration/easing and can pin a destination with [`Self::formation`].
    /// Reduced motion shows the destination immediately.
    pub fn sandbox_zoom(elapsed: Duration, progress: f64) -> Self {
        let progress = if progress.is_nan() {
            0.0
        } else {
            progress.clamp(0.0, 1.0)
        };
        let mut background = Self::new(MotionScene::Formations, elapsed);
        background.position = 2.0 - progress;
        background.sandbox_zoom = Some(progress);
        background
    }

    /// Interpolate from this frame's position to a target scene. The caller owns
    /// duration and easing. Progress is clamped to 0..=1; NaN means zero.
    /// Intermediate scenes follow suite order; endpoints match their static scene.
    pub fn transition_to(mut self, target: MotionScene, progress: f64) -> Self {
        let progress = if progress.is_nan() {
            0.0
        } else {
            progress.clamp(0.0, 1.0)
        };
        self.sandbox_zoom = None;
        self.position += (target.position() - self.position) * progress;
        self.scene = target;
        self
    }

    /// Freeze ambient time at zero and show the requested destination immediately.
    /// For freezing an already-playing frame, hold time/progress constant or use
    /// [`MotionState::set_reduced_motion`].
    pub fn reduced_motion(mut self, reduced: bool) -> Self {
        self.reduced_motion = reduced;
        self
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

impl Widget for MotionBackground {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Keep coordinates relative to the original area when the buffer clips it.
        let visible = area.intersection(buf.area);
        if visible.is_empty() {
            return;
        }
        let palette = self.palette.prepare();
        // Bound the floating-point clock even for a caller supplying Duration::MAX.
        let (position, time) = if self.reduced_motion {
            (self.scene.position(), 0.0)
        } else {
            (
                self.position,
                (self.elapsed.as_secs() % 86_400) as f64
                    + f64::from(self.elapsed.subsec_nanos()) / 1e9,
            )
        };
        let mut field = match self.sandbox_zoom.filter(|_| !self.reduced_motion) {
            Some(progress) => fields::Field::sandbox_zoom(area, time, progress, self.formation),
            None => fields::Field::new(area, time, position, self.formation),
        };
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
                cell.set_style(palette.style(sample.intensity * fade, sample.highlight));
            }
        }
    }
}

#[cfg(test)]
mod tests;
