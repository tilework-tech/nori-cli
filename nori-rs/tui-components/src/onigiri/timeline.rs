//! When each part of the welcome happens, in milliseconds since it started.

use std::time::Duration;

/// Grains start flying in here, as the wordmark's pens finish.
pub(crate) const GATHER_AT: f32 = 700.0;
pub(crate) const GRAIN_SPREAD: f32 = 380.0;
pub(crate) const GRAIN_DUR: f32 = 420.0;
/// Grains overshoot their spot by this much of their path, then spring back.
pub(crate) const GRAIN_OVERSHOOT: f32 = 1.9;
pub(crate) const PRESS_AT: f32 = GATHER_AT + 860.0;
pub(crate) const PRESS_DOWN: f32 = 110.0;
pub(crate) const PRESS_FRONT: f32 = 120.0;
/// After the press the ball springs up stretched, falls back and lands.
pub(crate) const HOP_AT: f32 = PRESS_AT + PRESS_DOWN;
pub(crate) const HOP: f32 = 260.0;
/// Hop height as a fraction of the ball's height.
pub(crate) const HOP_HEIGHT: f32 = 0.08;
pub(crate) const LAND_AT: f32 = HOP_AT + HOP;
/// The landing squash wobbles out over this long.
pub(crate) const WOBBLE: f32 = 520.0;
/// From here the ball no longer changes shape.
pub(crate) const PRESS_DONE: f32 = LAND_AT + WOBBLE;
/// The band wraps once the ball has stopped wobbling.
pub(crate) const NORI_AT: f32 = PRESS_DONE;
pub(crate) const NORI_DUR: f32 = 380.0;
/// The light starts circling once the ball has settled, easing up to speed
/// over LIGHT_RAMP.
pub(crate) const LIGHT_AT: f32 = PRESS_DONE;
pub(crate) const LIGHT_RAMP: f32 = 1200.0;
/// One full turn of the light.
pub(crate) const LIGHT_PERIOD: f32 = 6000.0;

/// From here on the ball and wordmark no longer change shape; only the
/// light and the effects that follow it move.
pub(crate) const SETTLED_AT: f32 = 3000.0;

/// The intro plays this many times faster than the timeline above. After
/// SETTLED_AT the scene runs in real time, so the light keeps LIGHT_PERIOD.
pub(crate) const SPEED: f32 = 1.15;

/// Timeline time for a wall-clock time since the welcome started.
pub(crate) fn scene_ms(real_ms: f32) -> f32 {
    if real_ms * SPEED < SETTLED_AT {
        real_ms * SPEED
    } else {
        real_ms + SETTLED_AT - SETTLED_AT / SPEED
    }
}

/// Wall-clock time at which the timeline reaches `scene_ms`.
pub(crate) fn real_ms(scene_ms: f32) -> f32 {
    if scene_ms < SETTLED_AT {
        scene_ms / SPEED
    } else {
        scene_ms - SETTLED_AT + SETTLED_AT / SPEED
    }
}

/// Frame interval while things fly in and get pressed.
pub(crate) const INTRO_FRAME: Duration = Duration::from_millis(16);
/// The light only moves on this 30 Hz grid, and loop frames are scheduled on
/// it, so every loop frame is distinct and redraws between ticks change
/// nothing. The light turns 2 degrees per tick; faster frames would not look
/// smoother, only cost more output.
pub(crate) const LIGHT_TICK: f32 = 1000.0 / 30.0;
/// Lands a scheduled frame safely past its tick despite timer rounding.
pub(crate) const TICK_SLACK: Duration = Duration::from_micros(500);

/// A bump (see `Onigiri::bump`): the ball crouches, hops and lands with a
/// wobble, and the i dot hops with it. The bump waits for the intro's
/// landing wobble to end.
pub(crate) const BUMP_CROUCH: f32 = 80.0;
pub(crate) const BUMP_HOP: f32 = 240.0;
/// Bump hop height as a fraction of the ball's height.
pub(crate) const BUMP_HEIGHT: f32 = 0.06;
pub(crate) const BUMP_WOBBLE: f32 = 400.0;
pub(crate) const BUMP_LAND: f32 = BUMP_CROUCH + BUMP_HOP;
pub(crate) const BUMP_DUR: f32 = BUMP_LAND + BUMP_WOBBLE;
