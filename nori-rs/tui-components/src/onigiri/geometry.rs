//! Where the ball sits on the cell grid, and its pose during the press.

use std::f32::consts::TAU;

use super::shape::ASP;
use super::shape::ball_extent;
use super::shape::sd_ball;
use super::shape::smooth;
use super::timeline::BUMP_CROUCH;
use super::timeline::BUMP_HEIGHT;
use super::timeline::BUMP_HOP;
use super::timeline::BUMP_LAND;
use super::timeline::BUMP_WOBBLE;
use super::timeline::HOP;
use super::timeline::HOP_AT;
use super::timeline::HOP_HEIGHT;
use super::timeline::LAND_AT;
use super::timeline::PRESS_AT;
use super::timeline::PRESS_DOWN;
use super::timeline::WOBBLE;

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
    /// Cell-space point (x in cells, y in half cells) to ball units, with
    /// the ball lifted and squashed about its bottom centre.
    pub fn local(&self, x: f32, y: f32, pose: Pose) -> (f32, f32) {
        let u = (x - self.p.cx) / self.p.s;
        let v = (y + pose.lift - self.p.top) * ASP / self.p.s + self.vtop;
        let (sx, sy) = (1.0 + 0.06 * pose.squash, 1.0 - 0.10 * pose.squash);
        (u / sx, self.vbot + (v - self.vbot) / sy)
    }
    pub fn in_band_cols(&self, cx: i32) -> bool {
        cx >= self.band_cols.0 && cx < self.band_cols.1
    }
}

/// The ball's squash (positive = flattened, negative = stretched tall) and
/// lift above its baseline in half cells.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub squash: f32,
    pub lift: f32,
}

impl Pose {
    pub const REST: Pose = Pose {
        squash: 0.0,
        lift: 0.0,
    };

    /// Pressed flat, springing up stretched, then landing and wobbling.
    pub fn at(t: f32, p: &Place) -> Pose {
        if t < PRESS_AT {
            Pose::REST
        } else if t < HOP_AT {
            Pose {
                squash: smooth((t - PRESS_AT) / PRESS_DOWN),
                lift: 0.0,
            }
        } else if t < LAND_AT {
            let x = (t - HOP_AT) / HOP;
            // Stretched while moving fast, round at the top of the hop.
            let stretch = -0.7 * (2.0 * x - 1.0).powi(2);
            Pose {
                squash: 1.0 + (stretch - 1.0) * smooth(x / 0.25),
                lift: HOP_HEIGHT * Place::rows(p.s) * 2.0 * 4.0 * x * (1.0 - x),
            }
        } else {
            let tau = (t - LAND_AT) / WOBBLE;
            let squash = if tau >= 1.0 {
                0.0
            } else {
                0.9 * (-4.0 * tau).exp() * (tau * TAU * 1.6).cos() * (1.0 - tau)
            };
            Pose { squash, lift: 0.0 }
        }
    }

    /// `b` ms into a bump: a crouch, a small stretched hop, then a landing
    /// squash that wobbles out.
    pub fn bump(b: f32, p: &Place) -> Pose {
        if b < BUMP_CROUCH {
            Pose {
                squash: 0.6 * smooth(b / BUMP_CROUCH),
                lift: 0.0,
            }
        } else if b < BUMP_LAND {
            let x = (b - BUMP_CROUCH) / BUMP_HOP;
            let stretch = -0.4 * (2.0 * x - 1.0).powi(2);
            Pose {
                squash: 0.6 + (stretch - 0.6) * smooth(x / 0.25),
                lift: BUMP_HEIGHT * Place::rows(p.s) * 2.0 * 4.0 * x * (1.0 - x),
            }
        } else {
            let tau = ((b - BUMP_LAND) / BUMP_WOBBLE).min(1.0);
            Pose {
                squash: 0.6 * (-4.0 * tau).exp() * (tau * TAU * 1.6).cos() * (1.0 - tau),
                lift: 0.0,
            }
        }
    }
}

/// Majority-coverage quadrant mask of the ball in a cell (bit order as QUAD).
pub fn coverage(g: &Geo, cx: i32, cy: i32, pose: Pose) -> i32 {
    let mut mask = 0;
    for q in 0..4 {
        let (qx, qy) = ((q % 2) as f32, (q / 2) as f32);
        let mut n = 0;
        for j in 0..3 {
            for i in 0..3 {
                let x = cx as f32 + qx * 0.5 + (i as f32 + 0.5) / 6.0;
                let y = 2.0 * cy as f32 + qy + (j as f32 + 0.5) / 3.0;
                let (u, v) = g.local(x, y, pose);
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
