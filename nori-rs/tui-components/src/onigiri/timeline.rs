//! When each part of the welcome happens, in milliseconds since it started.

use std::time::Duration;

pub(crate) const GRAIN_SPREAD: f32 = 380.0;
pub(crate) const GRAIN_DUR: f32 = 420.0;
pub(crate) const PRESS_AT: f32 = 860.0;
pub(crate) const PRESS_DOWN: f32 = 110.0;
/// The bounce after the press dies out over this long.
pub(crate) const PRESS_BOUNCE: f32 = 380.0;
pub(crate) const PRESS_FRONT: f32 = 120.0;
pub(crate) const NORI_AT: f32 = 1080.0;
pub(crate) const NORI_DUR: f32 = 380.0;
/// The light starts circling here, easing up to speed over LIGHT_RAMP.
pub(crate) const LIGHT_AT: f32 = 1900.0;
pub(crate) const LIGHT_RAMP: f32 = 1200.0;
/// One full turn of the light.
pub(crate) const LIGHT_PERIOD: f32 = 6000.0;

/// From here on the ball and wordmark no longer change shape; only the
/// light and the effects that follow it move.
pub(crate) const SETTLED_AT: f32 = 3000.0;

/// Frame interval while things fly in and get pressed.
pub(crate) const INTRO_FRAME: Duration = Duration::from_millis(16);
/// The light only moves on this 30 Hz grid, and loop frames are scheduled on
/// it, so every loop frame is distinct and redraws between ticks change
/// nothing. The light turns 2 degrees per tick; faster frames would not look
/// smoother, only cost more output.
pub(crate) const LIGHT_TICK: f32 = 1000.0 / 30.0;
/// Lands a scheduled frame safely past its tick despite timer rounding.
pub(crate) const TICK_SLACK: Duration = Duration::from_micros(500);
