//! Where the ball sits on the cell grid, and its squash during the press.

use std::f32::consts::TAU;

use super::shape::ASP;
use super::shape::ball_extent;
use super::shape::sd_ball;
use super::shape::smooth;
use super::timeline::PRESS_AT;
use super::timeline::PRESS_BOUNCE;
use super::timeline::PRESS_DOWN;

const BAND_W: f32 = 0.40;
const BAND_FRAC: f32 = 0.36;

/// A ball placed on the grid: centre x in cells, top in half cells, size in
/// cell widths.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub cx: f32,
    pub top: f32,
    pub s: f32,
}

impl Place {
    /// Ball whose drawn height is `rows` cells.
    pub fn with_rows(cx: f32, top_row: i32, rows: f32) -> Place {
        Place {
            cx,
            top: top_row as f32 * 2.0,
            s: Place::size_for(rows),
        }
    }
    pub fn rows(s: f32) -> f32 {
        let (vt, vb) = ball_extent();
        (vb - vt) * s / ASP / 2.0
    }
    pub fn width(s: f32) -> f32 {
        1.09 * s
    }
    /// Size whose drawn height is `rows` cells.
    pub fn size_for(rows: f32) -> f32 {
        let (vt, vb) = ball_extent();
        rows * 2.0 * ASP / (vb - vt)
    }
    /// Baseline in half cells.
    pub fn base(&self) -> f32 {
        self.top + Place::rows(self.s) * 2.0
    }
}

/// The cell box around a placed ball and its nori band.
#[derive(Debug, Clone, Copy)]
pub struct Geo {
    pub p: Place,
    vtop: f32,
    vbot: f32,
    pub rows: (i32, i32),
    pub cols: (i32, i32),
    pub band_cols: (i32, i32),
    pub band_top: i32,
}

impl Geo {
    pub fn new(p: Place) -> Self {
        let (vtop, vbot) = ball_extent();
        let r0 = (p.top / 2.0).floor() as i32;
        let r1 = (p.base() / 2.0).ceil() as i32;
        let half_w = 0.62 * p.s;
        let band_rows = (((r1 - r0) as f32 * BAND_FRAC).round() as i32).max(1);
        Geo {
            p,
            vtop,
            vbot,
            rows: (r0 - 1, r1 + 1),
            cols: (
                (p.cx - half_w).floor() as i32,
                (p.cx + half_w).ceil() as i32,
            ),
            band_cols: (
                (p.cx - BAND_W * p.s / 2.0).round() as i32,
                (p.cx + BAND_W * p.s / 2.0).round() as i32,
            ),
            band_top: r1 - band_rows,
        }
    }
    /// Cell-space point (x in cells, y in half cells) to ball units,
    /// squashed about the bottom centre.
    pub fn local(&self, x: f32, y: f32, squash: f32) -> (f32, f32) {
        let u = (x - self.p.cx) / self.p.s;
        let v = (y - self.p.top) * ASP / self.p.s + self.vtop;
        let (sx, sy) = (1.0 + 0.06 * squash, 1.0 - 0.10 * squash);
        (u / sx, self.vbot + (v - self.vbot) / sy)
    }
    pub fn in_band_cols(&self, cx: i32) -> bool {
        cx >= self.band_cols.0 && cx < self.band_cols.1
    }
}

/// Squash of the ball at time `t` (positive = pressed flat).
pub fn press(t: f32) -> f32 {
    if t < PRESS_AT {
        0.0
    } else if t < PRESS_AT + PRESS_DOWN {
        smooth((t - PRESS_AT) / PRESS_DOWN)
    } else {
        let tau = (t - PRESS_AT - PRESS_DOWN) / PRESS_BOUNCE;
        if tau >= 1.0 {
            0.0
        } else {
            (-4.2 * tau).exp() * (tau * TAU * 1.15).cos()
        }
    }
}

/// Majority-coverage quadrant mask of the ball in a cell (bit order as QUAD).
pub fn coverage(g: &Geo, cx: i32, cy: i32, squash: f32) -> i32 {
    let mut mask = 0;
    for q in 0..4 {
        let (qx, qy) = ((q % 2) as f32, (q / 2) as f32);
        let mut n = 0;
        for j in 0..3 {
            for i in 0..3 {
                let x = cx as f32 + qx * 0.5 + (i as f32 + 0.5) / 6.0;
                let y = 2.0 * cy as f32 + qy + (j as f32 + 0.5) / 3.0;
                let (u, v) = g.local(x, y, squash);
                if sd_ball(u, v) < 0.0 {
                    n += 1;
                }
            }
        }
        if n >= 5 {
            mask |= 1 << q;
        }
    }
    mask
}
