//! Picks the arrangement and ball size that fit a terminal.

use super::geometry::Place;
use super::mini;
use super::word;

/// Smallest ball (rows) that plays the full intro with particles.
pub(crate) const INTRO_ROWS: f32 = 11.0;
/// Smallest ball (rows) that carries grain texture and the circling light.
pub(crate) const TEXTURE_ROWS: f32 = 7.0;
/// Smallest ball (rows) worth drawing at all; below it, a one-line header.
const MIN_ROWS: f32 = 4.0;
const MAX_ROWS: f32 = 34.0;
/// Narrowest area for the mini onigiri beside the text wordmark.
const MINI_COLS: f32 = 30.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Layout {
    /// Ball left, block wordmark right on the ball's baseline.
    Side { rows: f32 },
    /// Ball centred, block wordmark and caption centred under it.
    Stack { rows: f32 },
    /// The hand-pixelled mini onigiri, 3 rows tall, beside a text wordmark.
    Mini,
    /// One-line text header.
    Line,
}

/// Rows between the ball and the stacked wordmark: room for the i dot and
/// a row of air above it.
pub(crate) const STACK_GAP: i32 = 2;

/// Rows a stacked welcome takes: the ball, a gap, the wordmark, a gap and
/// the caption.
pub(crate) fn stack_height(rows: f32) -> f32 {
    rows + (STACK_GAP + word::letter_rows(rows)) as f32 + 2.0
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
    let side = fit(&|r| {
        ball_w(r) + 0.2 * r + 3.0 + word::width_for(r) <= w - 10.0
            && r <= 0.62 * h
            && r + 3.0 <= h - 2.0
    });
    let stack = fit(&|r| {
        ball_w(r).max(word::width_for(r)) <= w - 4.0 && r <= 0.66 * h && stack_height(r) <= h - 1.0
    });
    // Cells are about twice as tall as wide, so this is a portrait area.
    let portrait = 2.0 * h > w;
    // Side by side reads best in landscape; stack in portrait, or when it
    // buys a much bigger ball.
    if side >= MIN_ROWS && !(portrait && stack >= side) && (side >= 14.0 || side * 1.6 > stack) {
        Layout::Side { rows: side }
    } else if stack >= TEXTURE_ROWS {
        // Smaller, the block letters under the ball stop reading as `nori`.
        Layout::Stack { rows: stack }
    } else if h >= mini::HEIGHT as f32 && w >= MINI_COLS {
        Layout::Mini
    } else {
        Layout::Line
    }
}
