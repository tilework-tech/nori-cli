//! When and how strongly each wordmark effect plays. Times are milliseconds
//! since the welcome started; these are the knobs to tune the wordmark's
//! motion.

use super::super::shape::clamp01;
use super::super::shape::smooth;
use super::super::timeline::HOP;
use super::super::timeline::LAND_AT;

/// The pen starts writing.
pub const WORD_AT: f32 = 150.0;
/// Each letter has its own pen; they start this far apart, left to right.
pub const STAGGER: f32 = 60.0;
/// The longest letter takes this long to write at any size; the others use
/// the same pen speed, so shorter letters finish sooner.
pub const LETTER_DUR: f32 = 650.0;
/// Every pen has finished by here.
#[cfg(test)]
pub const DRAW_END: f32 = WORD_AT + 3.0 * STAGGER + LETTER_DUR;
/// A freshly drawn stroke fades from the pen's white to unlit glass.
pub const TRAIL: f32 = 260.0;

/// A letter ignites this long after its stroke is finished.
pub const IGNITE_DELAY: f32 = 40.0;
/// Ignition stutter, one entry per IGNITE_SLOT.
const IGNITE: [f32; 11] = [1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.2, 1.0, 1.0, 1.0];
const IGNITE_SLOT: f32 = 30.0;
pub const IGNITE_DUR: f32 = IGNITE.len() as f32 * IGNITE_SLOT;
/// After igniting, the neon cools to the resting wordmark over this long.
pub const COOL: f32 = 900.0;
/// Glow left behind once the neon has cooled (0 = none).
pub const GLOW_REST: f32 = 0.2;
/// Glow strength: background brightness reached inside a lit letter, and
/// how quickly overlapping glow saturates.
pub const GLOW_MAX: f32 = 0.6;
pub const GLOW_K: f32 = 0.5;
/// Glow radius in cells: a constant plus this much per letter row.
pub const GLOW_R: (f32, f32) = (1.0, 0.15);

/// The i dot falls while the ball hops and lands on the same frame.
pub const DOT_FALL: f32 = HOP;
pub const DOT_LAND: f32 = LAND_AT;
pub const DOT_AT: f32 = DOT_LAND - DOT_FALL;
pub const SQUASH: f32 = 70.0;
pub const BOUNCE: f32 = 220.0;
/// The landing ripple runs down the i stem at this many ms per row.
pub const RIPPLE_ROW: f32 = 45.0;
pub const SPLASH: f32 = 400.0;
/// The dot's landing glow fades over this long.
pub const DOT_GLOW: f32 = 500.0;
pub const VERSION_AT: f32 = DOT_LAND + 150.0;

/// The shimmer fades in from here once the neon has cooled.
pub const SHIMMER_AT: f32 = 3400.0;
const SHIMMER_RAMP: f32 = 800.0;
/// Shimmer band half-widths, in cells per letter row (plus a constant):
/// the leading edge is short and white, the trailing tail long and green.
pub const HEAD_W: (f32, f32) = (0.5, 1.0);
pub const TAIL_W: (f32, f32) = (1.4, 1.0);
/// Columns the band leans per row, so it sweeps on a slant.
pub const SLANT: f32 = 0.5;
/// Background glow under the shimmer's core (0 = none).
pub const SHIMMER_GLOW: f32 = 0.45;

/// A letter's tube brightness `dt` ms after its ignition starts: `None`
/// before, the stutter during, then fully lit.
pub fn ignite(dt: f32) -> Option<f32> {
    if dt < 0.0 {
        return None;
    }
    let slot = (dt / IGNITE_SLOT) as usize;
    Some(IGNITE.get(slot).copied().unwrap_or(1.0))
}

/// How far a lit letter has cooled, 0..1, `dt` ms after ignition starts.
pub fn cooled(dt: f32) -> f32 {
    smooth((dt - IGNITE_DUR) / COOL)
}

pub fn shimmer_amp(t: f32) -> f32 {
    smooth((t - SHIMMER_AT) / SHIMMER_RAMP)
}

/// Shimmer brightness at `dx` cells from the band centre.
pub fn shimmer(dx: f32, moving_right: bool, h: f32) -> f32 {
    let behind = if moving_right { dx < 0.0 } else { dx > 0.0 };
    let (k, c) = if behind { TAIL_W } else { HEAD_W };
    let w = k * h + c;
    (-(dx / w).powi(2)).exp()
}

/// Glow background brightness from a summed glow field.
pub fn glow(field: f32) -> f32 {
    GLOW_MAX * (1.0 - (-field * GLOW_K).exp()).powf(1.6)
}

/// Fraction of the fall completed, under gravity.
pub fn fall(t: f32) -> f32 {
    clamp01((t - DOT_AT) / DOT_FALL).powi(2)
}
