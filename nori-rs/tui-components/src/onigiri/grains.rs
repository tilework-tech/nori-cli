//! Loose grains before the press: a subset of the ball's dots flying in from
//! a larger, looser cloud of the same shape.

use std::f32::consts::TAU;

use super::geometry::Geo;
use super::geometry::Place;
use super::geometry::Pose;
use super::shape::BRAILLE_BITS;
use super::shape::clamp01;
use super::shape::hash;
use super::shape::sd_ball;
use super::timeline::GATHER_AT;
use super::timeline::GRAIN_DUR;
use super::timeline::GRAIN_OVERSHOOT;
use super::timeline::GRAIN_SPREAD;

/// One flying dot: where it starts and lands (cells, half cells) and when.
struct Dot {
    from: (f32, f32),
    to: (f32, f32),
    start: f32,
    dur: f32,
}

#[derive(Clone, Copy, Default)]
struct Slot {
    bits: i32,
    landed: i32,
    used: bool,
}

pub struct Grains {
    dots: Vec<Dot>,
    /// Cell box every dot stays inside, as (x0, y0, width).
    origin: (i32, i32, i32),
    slots: Vec<Slot>,
}

impl Grains {
    /// No grains, for balls that skip the intro.
    pub fn none() -> Grains {
        Grains {
            dots: Vec::new(),
            origin: (0, 0, 0),
            slots: Vec::new(),
        }
    }

    pub fn new(g: &Geo) -> Grains {
        let mid_y = g.p.top + Place::rows(g.p.s) * 2.0 * 0.55;
        // Fly distance scales with the ball so small balls stay on screen.
        let fly = (g.p.s / 11.0).clamp(1.0, 3.5);
        let mut dots = Vec::new();
        for by in (g.rows.0 * 4)..(g.rows.1 * 4) {
            for bx in (g.cols.0 * 2)..(g.cols.1 * 2) {
                let (x, y) = ((bx as f32 + 0.5) / 2.0, (by as f32 + 0.5) / 2.0);
                let (u, v) = g.local(x, y, Pose::REST);
                if sd_ball(u, v) >= 0.0 || hash(bx, by) > 0.6 {
                    continue;
                }
                let h2 = hash(by + 911, bx - 77);
                let h3 = hash(bx * 3 + 5, by * 5 + 9);
                let dist = u.hypot(v);
                let start =
                    GATHER_AT + GRAIN_SPREAD * (0.55 * h2 + 0.45 * (1.0 - dist * 1.8).max(0.0));
                let ang = h3 * TAU;
                let spread = 0.25 + 0.45 * h2;
                let sx = x + (x - g.p.cx) * spread + ang.cos() * fly;
                let sy = y + (y - mid_y) * spread + ang.sin() * fly;
                dots.push(Dot {
                    from: (sx, sy),
                    to: (x, y),
                    start,
                    dur: GRAIN_DUR * (0.8 + 0.4 * h3),
                });
            }
        }
        // Dots move in straight lines and overshoot their end by under 15%,
        // so the box around the start and the farthest overshoot holds
        // every frame.
        let cell = |x: f32, y: f32| {
            (
                ((x * 2.0).floor() as i32).div_euclid(2),
                ((y * 2.0).floor() as i32).div_euclid(4),
            )
        };
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for d in &dots {
            let past = (
                d.to.0 + (d.to.0 - d.from.0) * 0.15,
                d.to.1 + (d.to.1 - d.from.1) * 0.15,
            );
            for (x, y) in [d.from, d.to, past] {
                let (cx, cy) = cell(x, y);
                (x0, y0, x1, y1) = (
                    x0.min(cx - 1),
                    y0.min(cy - 1),
                    x1.max(cx + 1),
                    y1.max(cy + 1),
                );
            }
        }
        let (w, h) = if dots.is_empty() {
            (0, 0)
        } else {
            (x1 - x0 + 1, y1 - y0 + 1)
        };
        Grains {
            dots,
            origin: (x0, y0, w),
            slots: vec![Slot::default(); (w * h).max(0) as usize],
        }
    }

    /// Cells holding grains at time `t`: (x, y, dot bits, whether more dots
    /// there have landed than are still flying).
    pub fn at(&mut self, t: f32) -> impl Iterator<Item = (i32, i32, i32, bool)> + '_ {
        let (x0, y0, w) = self.origin;
        self.slots.fill(Slot::default());
        for d in &self.dots {
            // A grain comes into frame when it starts flying.
            if t < d.start {
                continue;
            }
            let x = clamp01((t - d.start) / d.dur);
            let k = spring(x);
            let (px, py) = (
                d.from.0 + (d.to.0 - d.from.0) * k,
                d.from.1 + (d.to.1 - d.from.1) * k,
            );
            let (dbx, dby) = ((px * 2.0).floor() as i32, (py * 2.0).floor() as i32);
            let (cx, cy) = (dbx.div_euclid(2), dby.div_euclid(4));
            let slot = &mut self.slots[((cy - y0) * w + cx - x0) as usize];
            slot.used = true;
            slot.bits |= BRAILLE_BITS[dby.rem_euclid(4) as usize][dbx.rem_euclid(2) as usize];
            slot.landed += if x > 0.97 { 1 } else { -1 };
        }
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.used)
            .map(move |(i, s)| {
                let i = i as i32;
                (x0 + i % w, y0 + i / w, s.bits, s.landed > 0)
            })
    }
}

/// Ease out past the end and back (an overshoot of about 12% at the default
/// GRAIN_OVERSHOOT).
fn spring(x: f32) -> f32 {
    let (c1, c3) = (GRAIN_OVERSHOOT, GRAIN_OVERSHOOT + 1.0);
    1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
}
