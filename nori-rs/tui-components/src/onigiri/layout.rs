//! Picks the arrangement and ball size that fit a terminal.

use super::ball::Place;
use super::word;

/// Smallest ball (rows) that plays the full intro with particles.
pub(crate) const INTRO_ROWS: f32 = 11.0;
/// Smallest ball (rows) that carries grain texture and the circling light.
pub(crate) const TEXTURE_ROWS: f32 = 7.0;
/// Smallest ball (rows) worth drawing at all; below it, a one-line header.
const MIN_ROWS: f32 = 4.0;
const MAX_ROWS: f32 = 34.0;
/// Wordmark x-height as a fraction of the ball height.
pub(crate) const WORD_RATIO: f32 = 1.0 / 13.6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Layout {
    /// Ball left, block wordmark right on the ball's baseline.
    Side { rows: f32 },
    /// Ball centred, text wordmark line under it.
    Stack { rows: f32 },
    /// One-line text header.
    Line,
}

/// Largest ball that satisfies `ok`, in whole rows, by bisection.
fn fit(ok: &dyn Fn(f32) -> bool) -> f32 {
    let (mut lo, mut hi) = (0.0f32, MAX_ROWS);
    for _ in 0..30 {
        let mid = (lo + hi) / 2.0;
        if ok(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo.floor()
}

pub fn layout(w: u16, h: u16) -> Layout {
    let (w, h) = (f32::from(w), f32::from(h));
    let ball_w = |r: f32| Place::width(Place::size_for(r));
    let word_w = |r: f32| word::WORD_W * r * WORD_RATIO;
    let side = fit(&|r| {
        ball_w(r) + 0.2 * r + 3.0 + word_w(r) <= w - 10.0 && r <= 0.62 * h && r + 3.0 <= h - 2.0
    });
    let stack = fit(&|r| ball_w(r) <= w - 4.0 && r <= 0.66 * h && r + 4.0 <= h - 1.0);
    // Side by side reads best; stack only when it buys a much bigger ball.
    if side >= MIN_ROWS && (side >= 14.0 || side * 1.6 >= stack) {
        Layout::Side { rows: side }
    } else if stack >= MIN_ROWS {
        Layout::Stack { rows: stack }
    } else {
        Layout::Line
    }
}
