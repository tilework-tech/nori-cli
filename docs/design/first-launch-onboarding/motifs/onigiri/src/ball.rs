//! The onigiri: geometry, the intro (gather, press, wrap) and the lit rice.
//!
//! Rice is shaded as a puffy dome under one light that circles the ball
//! after the intro. Each Braille dot may belong to a rice grain with its own
//! tilt; grains that tilt toward the light glint and grains in shadow show as
//! pits, so the texture moves with the light while the shape stays still.

use std::collections::HashMap;
use std::f32::consts::{PI, TAU};

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};

use crate::color::{color, Mat, Tier};
use crate::shape::*;

// Intro timeline (ms).
const GRAIN_SPREAD: f32 = 380.0;
const GRAIN_DUR: f32 = 420.0;
pub const PRESS_AT: f32 = 860.0;
const PRESS_DOWN: f32 = 110.0;
const PRESS_FRONT: f32 = 120.0;
pub const NORI_AT: f32 = 1080.0;
const NORI_DUR: f32 = 380.0;
/// The light starts circling here, easing up to speed over LIGHT_RAMP.
pub const LIGHT_AT: f32 = 1900.0;
const LIGHT_RAMP: f32 = 1200.0;
/// One full turn of the light.
pub const LIGHT_PERIOD: f32 = 6000.0;
/// Resting light: upper left.
const LIGHT_REST: f32 = -0.75 * PI;
const LIGHT_ELEV: f32 = 0.70;

const BAND_W: f32 = 0.40;
const BAND_FRAC: f32 = 0.36;
const GRAIN_DENSITY: f32 = 0.50;

pub struct Paint<'a> {
    pub buf: &'a mut Buffer,
    pub tier: Tier,
}

impl Paint<'_> {
    pub fn put(&mut self, x: i32, y: i32, s: &str, style: Style) {
        if x < 0 || y < 0 {
            return;
        }
        let (x, y) = (x as u16, y as u16);
        if self.buf.area.contains((x, y).into()) {
            if let Some(c) = self.buf.cell_mut((x, y)) {
                c.set_symbol(s).set_style(style);
            }
        }
    }
    fn c(&self, m: Mat, b: f32) -> Color {
        color(self.tier, m, b)
    }
}

/// A ball placed on the grid: centre x in cells, top in half cells, size in
/// cell widths.
#[derive(Clone, Copy)]
pub struct Place {
    pub cx: f32,
    pub top: f32,
    pub s: f32,
}

impl Place {
    /// Ball whose drawn height is `rows` cells.
    pub fn with_rows(cx: f32, top_row: i32, rows: f32) -> Place {
        let (vt, vb) = ball_extent();
        Place { cx, top: top_row as f32 * 2.0, s: rows * 2.0 * ASP / (vb - vt) }
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

struct Geo {
    p: Place,
    vtop: f32,
    rows: (i32, i32),
    cols: (i32, i32),
    band_cols: (i32, i32),
    band_top: i32,
}

impl Geo {
    fn new(p: Place) -> Self {
        let (vtop, _) = ball_extent();
        let r0 = (p.top / 2.0).floor() as i32;
        let r1 = (p.base() / 2.0).ceil() as i32;
        let half_w = 0.62 * p.s;
        let band_rows = (((r1 - r0) as f32 * BAND_FRAC).round() as i32).max(1);
        Geo {
            p,
            vtop,
            rows: (r0 - 1, r1 + 1),
            cols: ((p.cx - half_w).floor() as i32, (p.cx + half_w).ceil() as i32),
            band_cols: (
                (p.cx - BAND_W * p.s / 2.0).round() as i32,
                (p.cx + BAND_W * p.s / 2.0).round() as i32,
            ),
            band_top: r1 - band_rows,
        }
    }
    /// Cell-space point (x in cells, y in half cells) to ball units,
    /// squashed about the bottom centre.
    fn local(&self, x: f32, y: f32, squash: f32) -> (f32, f32) {
        let u = (x - self.p.cx) / self.p.s;
        let v = (y - self.p.top) * ASP / self.p.s + self.vtop;
        let (sx, sy) = (1.0 + 0.06 * squash, 1.0 - 0.10 * squash);
        let (_, vbot) = ball_extent();
        (u / sx, vbot + (v - vbot) / sy)
    }
}

/// Squash of the ball at time `t` (positive = pressed flat).
fn press(t: f32) -> f32 {
    if t < PRESS_AT {
        0.0
    } else if t < PRESS_AT + PRESS_DOWN {
        smooth((t - PRESS_AT) / PRESS_DOWN)
    } else {
        let tau = (t - PRESS_AT - PRESS_DOWN) / 380.0;
        if tau > 1.0 { 0.0 } else { (-4.2 * tau).exp() * (tau * TAU * 1.15).cos() }
    }
}

/// Light direction in screen space (x right, y down, z toward the viewer).
pub fn light(t: f32, animate: bool) -> [f32; 3] {
    let tau = if animate { (t - LIGHT_AT).max(0.0) } else { 0.0 };
    // Constant acceleration over the ramp, then constant speed.
    let w = TAU / LIGHT_PERIOD;
    let phase = if tau < LIGHT_RAMP { w * tau * tau / (2.0 * LIGHT_RAMP) } else { w * (tau - LIGHT_RAMP / 2.0) };
    let th = LIGHT_REST + phase;
    let (ce, se) = (LIGHT_ELEV.cos(), LIGHT_ELEV.sin());
    [ce * th.cos(), ce * th.sin(), se]
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / l, v[1] / l, v[2] / l]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Height of the rice surface: a rounded rim falling off at the outline,
/// on top of a gentle dome.
fn pillow(u: f32, v: f32) -> f32 {
    // Rounded profile from the outline to the deepest interior point, so the
    // whole body is one surface.
    const DEPTH: f32 = 0.36;
    let s = clamp01(-sd_ball(u, v) / DEPTH);
    (1.0 - (1.0 - s) * (1.0 - s)).sqrt() * DEPTH * 0.8
}

/// The pillow blurred a little so the triangle's medial creases soften.
fn height(u: f32, v: f32) -> f32 {
    const R: f32 = 0.045;
    const TAPS: [(f32, f32); 7] =
        [(0.0, 0.0), (1.0, 0.0), (-1.0, 0.0), (0.5, 0.87), (-0.5, 0.87), (0.5, -0.87), (-0.5, -0.87)];
    TAPS.iter().map(|(a, b)| pillow(u + a * R, v + b * R)).sum::<f32>() / TAPS.len() as f32
}

fn normal(u: f32, v: f32) -> [f32; 3] {
    let e = 0.006;
    let dx = (height(u + e, v) - height(u - e, v)) / (2.0 * e);
    let dy = (height(u, v + e) - height(u, v - e)) / (2.0 * e);
    norm([-dx, -dy, 1.0])
}

fn shade(n: [f32; 3], l: [f32; 3]) -> f32 {
    let diffuse = dot(n, l).max(0.0);
    let h = norm([l[0], l[1], l[2] + 1.0]);
    let spec = dot(n, h).max(0.0).powi(48);
    0.20 + 0.66 * diffuse + 0.32 * spec
}

/// Majority-coverage quadrant mask of the ball in a cell (bit order as QUAD).
fn coverage(g: &Geo, cx: i32, cy: i32, squash: f32, scale: f32) -> u32 {
    let mut mask = 0;
    for q in 0..4 {
        let (qx, qy) = ((q % 2) as f32, (q / 2) as f32);
        let mut n = 0;
        for j in 0..3 {
            for i in 0..3 {
                let x = cx as f32 + qx * 0.5 + (i as f32 + 0.5) / 6.0;
                let y = 2.0 * cy as f32 + qy + (j as f32 + 0.5) / 3.0;
                let (u, v) = g.local(x, y, squash);
                if sd_ball(u / scale, v / scale) < 0.0 {
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

fn rice_cell(p: &mut Paint, g: &Geo, cx: i32, cy: i32, squash: f32, l: [f32; 3], texture: bool) {
    let mask = coverage(g, cx, cy, squash, 1.0);
    if mask == 0 {
        return;
    }
    let (u, v) = g.local(cx as f32 + 0.5, 2.0 * cy as f32 + 1.0, squash);
    let n = normal(u, v);
    let b = shade(n, l);
    if mask != 15 {
        p.put(cx, cy, QUAD[mask as usize], Style::default().fg(p.c(Mat::Rice, b)));
        return;
    }
    let bg = p.c(Mat::Rice, b);
    if !texture {
        p.put(cx, cy, " ", Style::default().bg(bg));
        return;
    }
    let (mut glint, mut pit) = (0u32, 0u32);
    let (lx, ly) = (g.cols.0 * 2, g.rows.0 * 4);
    for (r, row) in BRAILLE_BITS.iter().enumerate() {
        for (c, bit) in row.iter().enumerate() {
            let bx = cx * 2 + c as i32 - lx;
            let by = cy * 4 + r as i32 - ly;
            // Grains are staggered pairs of dots sharing one tilt.
            let id = (bx, (by + (bx & 1)) >> 1);
            if hash(id.0 * 3 + 11, id.1 * 7 + 5) > GRAIN_DENSITY {
                continue;
            }
            let a = hash(id.0 + 101, id.1 - 33) * TAU;
            let tilt = 0.55 + 0.35 * hash(id.0 - 9, id.1 + 71);
            let ng = norm([n[0] + tilt * a.cos(), n[1] + tilt * a.sin(), n[2]]);
            let bg_ = shade(ng, l);
            if bg_ - b > 0.20 {
                glint |= bit;
            } else if b - bg_ > 0.22 && p.tier != Tier::Ansi16 {
                pit |= bit;
            }
        }
    }
    if glint.count_ones() >= pit.count_ones() && glint != 0 {
        let fg = p.c(Mat::Glint, (b + 0.35).min(1.0));
        p.put(cx, cy, &braille(glint), Style::default().fg(fg).bg(bg));
    } else if pit != 0 {
        let fg = p.c(Mat::Pit, (b - 0.20).max(0.0));
        p.put(cx, cy, &braille(pit), Style::default().fg(fg).bg(bg));
    } else {
        // No grain here: split the cell so shading gets two rows of detail.
        let half = |dy: f32| {
            let (u, v) = g.local(cx as f32 + 0.5, 2.0 * cy as f32 + dy, squash);
            p.c(Mat::Rice, shade(normal(u, v), l))
        };
        let (top, bot) = (half(0.5), half(1.5));
        p.put(cx, cy, "▀", Style::default().fg(top).bg(bot));
    }
}

fn nori_cell(p: &mut Paint, g: &Geo, cx: i32, cy: i32, l: [f32; 3]) {
    let mask = coverage(g, cx, cy, 0.0, 1.0);
    if mask == 0 {
        return;
    }
    // The sheet follows the rice surface but is only faintly glossy.
    let lum = |dy: f32| {
        let (u, v) = g.local(cx as f32 + 0.5, 2.0 * cy as f32 + dy, 0.0);
        let n = normal(u, v);
        let h = norm([l[0], l[1], l[2] + 1.0]);
        let gloss = dot(n, h).max(0.0).powi(16);
        p.c(Mat::Nori, 0.32 + 0.20 * (dot(n, l) - 0.6) + 0.35 * gloss)
    };
    if mask == 15 {
        p.put(cx, cy, "▀", Style::default().fg(lum(0.5)).bg(lum(1.5)));
    } else {
        p.put(cx, cy, QUAD[mask as usize], Style::default().fg(lum(1.0)));
    }
}

/// Draw the ball at time `t`. `intro` plays gather/press/wrap; otherwise the
/// ball is settled. `animate_light` lets the light circle after LIGHT_AT.
pub fn draw_ball(p: &mut Paint, place: Place, t: f32, intro: bool, animate_light: bool, texture: bool) {
    let g = Geo::new(place);
    let t_shape = if intro { t } else { 1.0e6 };
    let squash = press(t_shape);
    let l = light(t, animate_light);
    let steps = ((t_shape - NORI_AT) / NORI_DUR * (g.rows.1 - 1 - g.band_top) as f32).ceil() as i32;
    let band_top = (g.rows.1 - 1 - steps.max(0)).max(g.band_top);
    let in_band = |cx: i32, cy: i32| {
        t_shape >= NORI_AT && cx >= g.band_cols.0 && cx < g.band_cols.1 && cy >= band_top
    };
    let front = if t_shape >= PRESS_AT {
        0.25 + 0.85 * ease_out((t_shape - PRESS_AT) / PRESS_FRONT)
    } else {
        -1.0
    };
    let pressed = |cx: i32, cy: i32| {
        let (u, v) = g.local(cx as f32 + 0.5, 2.0 * cy as f32 + 1.0, 0.0);
        front > 0.0 && sd_ball(u / front.max(0.01), v / front.max(0.01)) < 0.0
    };

    if front < 1.09 {
        for (cell, bits, landed) in grains(&g, t_shape) {
            if pressed(cell.0, cell.1) || cell.1 >= g.rows.1 + 2 {
                continue;
            }
            let style = match (p.tier, landed > 0) {
                (Tier::Ansi16, true) => Style::default(),
                (Tier::Ansi16, false) => Style::default().add_modifier(Modifier::DIM),
                (_, true) => Style::default().fg(p.c(Mat::Rice, 0.80)),
                (_, false) => Style::default().fg(p.c(Mat::Rice, 0.42)),
            };
            p.put(cell.0, cell.1, &braille(bits), style);
        }
    }
    for cy in g.rows.0..g.rows.1 {
        for cx in g.cols.0..g.cols.1 {
            if in_band(cx, cy) {
                nori_cell(p, &g, cx, cy, l);
            } else if pressed(cx, cy) {
                rice_cell(p, &g, cx, cy, squash, l, texture);
            }
        }
    }
}

/// Loose grains before the press: a subset of the ball's dots flying in from
/// a larger, looser cloud of the same shape. Returns per-cell dot bits and
/// whether more dots there have landed than are still flying.
fn grains(g: &Geo, t: f32) -> Vec<((i32, i32), u32, i32)> {
    let mut cells: HashMap<(i32, i32), (u32, i32)> = HashMap::new();
    let mid_y = g.p.top + Place::rows(g.p.s) * 2.0 * 0.55;
    // Fly distance scales with the ball so small balls stay on screen.
    let fly = (g.p.s / 11.0).clamp(1.0, 3.5);
    for by in (g.rows.0 * 4)..(g.rows.1 * 4) {
        for bx in (g.cols.0 * 2)..(g.cols.1 * 2) {
            let (x, y) = ((bx as f32 + 0.5) / 2.0, (by as f32 + 0.5) / 2.0);
            let (u, v) = g.local(x, y, 0.0);
            if sd_ball(u, v) >= 0.0 || hash(bx, by) > 0.6 {
                continue;
            }
            let h2 = hash(by + 911, bx - 77);
            let h3 = hash(bx * 3 + 5, by * 5 + 9);
            let dist = u.hypot(v);
            let start = GRAIN_SPREAD * (0.55 * h2 + 0.45 * (1.0 - dist * 1.8).max(0.0));
            let k = ease_out((t - start) / (GRAIN_DUR * (0.8 + 0.4 * h3)));
            let ang = h3 * TAU;
            let spread = 0.25 + 0.45 * h2;
            let sx = x + (x - g.p.cx) * spread + ang.cos() * fly;
            let sy = y + (y - mid_y) * spread + ang.sin() * fly;
            let (px, py) = (sx + (x - sx) * k, sy + (y - sy) * k);
            let (dbx, dby) = ((px * 2.0).floor() as i32, (py * 2.0).floor() as i32);
            let e = cells.entry((dbx.div_euclid(2), dby.div_euclid(4))).or_insert((0, 0));
            e.0 |= BRAILLE_BITS[dby.rem_euclid(4) as usize][dbx.rem_euclid(2) as usize];
            e.1 += if k > 0.97 { 1 } else { -1 };
        }
    }
    cells.into_iter().map(|(c, (b, l))| (c, b, l)).collect()
}
