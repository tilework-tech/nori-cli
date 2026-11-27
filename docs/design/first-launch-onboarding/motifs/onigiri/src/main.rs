//! Onigiri motif for the Nori CLI first launch.
//!
//! A rice ball wrapped in a nori band, drawn from signed distance fields onto
//! quadrant blocks, with a heavy rounded `nori` wordmark beside it. The motion
//! is a pure function of elapsed milliseconds: rice grains (Braille dots)
//! gather into a loose triangle, get pressed solid with a small squash, the
//! nori band wraps up from the bottom, and the wordmark sets in.
//!
//! Usage:
//!   motif-onigiri --cast FILE [--size WxH] [--until MS] [--static]

use std::cell::RefCell;
use std::fs::File;
use std::io::{self, Write};
use std::rc::Rc;

use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::{Terminal, TerminalOptions, Viewport};

const VERSION: &str = "v0.42.0";

// Timeline (ms).
const GRAIN_SPREAD: f32 = 380.0;
const GRAIN_DUR: f32 = 420.0;
const PRESS_AT: f32 = 860.0;
const PRESS_DOWN: f32 = 110.0;
const PRESS_FRONT: f32 = 120.0;
const NORI_AT: f32 = 1080.0;
const NORI_DUR: f32 = 380.0;
const WORD_AT: f32 = 1320.0;
const WORD_SWEEP: f32 = 380.0;
const WORD_FLASH: f32 = 100.0;
const DOT_AT: f32 = 1820.0;
const DOT_DUR: f32 = 160.0;
pub const SETTLED_MS: u64 = 2000;

// Ball geometry, in units of the ball size.
const R_TRI: f32 = 0.37;
const RAD: f32 = 0.19;
const PUFF: f32 = 0.14;
const BAND_W: f32 = 0.40;
const BAND_FRAC: f32 = 0.36;
const NORI_GROW: f32 = 0.0;
/// Flat base so the ball sits on its baseline.
const BASE_V: f32 = 0.39;
const DROPOUT: f32 = 0.10;
/// One half cell is this many cell widths tall (cells are about 2.2:1).
const ASP: f32 = 1.16;

const QUAD: [&str; 16] = [
    " ", "▘", "▝", "▀", "▖", "▌", "▞", "▛", "▗", "▚", "▐", "▜", "▄", "▙", "▟", "█",
];
const BRAILLE_BITS: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum K {
    Empty,
    Rice,
    Sheen,
    Nori,
    Ink,
    Green,
}

impl K {
    fn group(self) -> u8 {
        match self {
            K::Empty => 0,
            K::Rice | K::Sheen => 1,
            K::Nori => 2,
            K::Ink => 3,
            K::Green => 4,
        }
    }
    fn color(self) -> Color {
        match self {
            K::Empty | K::Ink => Color::Reset,
            K::Rice => Color::Gray,
            K::Sheen => Color::White,
            K::Nori => Color::DarkGray,
            K::Green => Color::Green,
        }
    }
}

fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

fn clamp01(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}
fn smooth(t: f32) -> f32 {
    let t = clamp01(t);
    t * t * (3.0 - 2.0 * t)
}
fn ease_out(t: f32) -> f32 {
    let t = 1.0 - clamp01(t);
    1.0 - t * t * t
}

fn put(buf: &mut Buffer, x: i32, y: i32, s: &str, style: Style) {
    if x < 0 || y < 0 {
        return;
    }
    let (x, y) = (x as u16, y as u16);
    if buf.area.contains((x, y).into()) {
        if let Some(c) = buf.cell_mut((x, y)) {
            c.set_symbol(s).set_style(style);
        }
    }
}

// ---------------------------------------------------------------- shapes

/// Equilateral triangle, point up, y up (Inigo Quilez).
fn sd_tri(mut px: f32, mut py: f32, r: f32) -> f32 {
    let k = 3f32.sqrt();
    px = px.abs() - r;
    py += r / k;
    if px + k * py > 0.0 {
        let (nx, ny) = ((px - k * py) / 2.0, (-k * px - py) / 2.0);
        px = nx;
        py = ny;
    }
    px -= px.clamp(-2.0 * r, 0.0);
    -(px * px + py * py).sqrt() * py.signum()
}

/// Rounded, slightly puffy triangle. u right, v down, origin at the
/// triangle's centroid, in units of the ball size.
fn sd_ball(u: f32, v: f32) -> f32 {
    let dt = sd_tri(u, -v, R_TRI) - RAD;
    let dc = (u * u + (v - 0.04).powi(2)).sqrt() - 0.47;
    (dt * (1.0 - PUFF) + dc * PUFF).max(v - BASE_V)
}

/// Axis-aligned box with per-corner radii, y down.
#[allow(clippy::too_many_arguments)]
fn rbox(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: [f32; 4]) -> f32 {
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (bx, by) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    let (px, py) = (x - cx, y - cy);
    // r: [top-left, top-right, bottom-left, bottom-right]
    let rr = match (px > 0.0, py > 0.0) {
        (false, false) => r[0],
        (true, false) => r[1],
        (false, true) => r[2],
        (true, true) => r[3],
    };
    let qx = px.abs() - bx + rr;
    let qy = py.abs() - by + rr;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    qx.max(qy).min(0.0) + outside - rr
}

/// Squash of the ball at time `t` (positive = pressed flat).
fn press(t: f32) -> f32 {
    if t < PRESS_AT {
        0.0
    } else if t < PRESS_AT + PRESS_DOWN {
        smooth((t - PRESS_AT) / PRESS_DOWN)
    } else {
        let tau = (t - PRESS_AT - PRESS_DOWN) / 380.0;
        if tau > 1.0 {
            0.0
        } else {
            (-4.2 * tau).exp() * (tau * std::f32::consts::TAU * 1.15).cos()
        }
    }
}

/// Wordmark sampled in letter units (x right, y down, x-height top at 0,
/// baseline at H).
fn word_class(x: f32, y: f32) -> K {
    const H: f32 = 9.0 * ASP;
    const S: f32 = 3.0;
    if y > H {
        return K::Empty;
    }
    // n
    let (a, w) = (0.0, 10.0);
    if rbox(x, y, a, 0.0, a + w, H + 6.0, [1.0, 5.0, 0.0, 0.0]) < 0.0
        && rbox(x, y, a + S, S, a + w - S, H + 12.0, [0.5, 2.0, 0.0, 0.0]) > 0.0
    {
        return K::Ink;
    }
    // o
    let (a, w) = (11.5, 10.5);
    if rbox(x, y, a, 0.0, a + w, H, [4.2; 4]) < 0.0
        && rbox(x, y, a + S, S, a + w - S, H - S, [1.6; 4]) > 0.0
    {
        return K::Ink;
    }
    // r
    let (a, w) = (23.5, 7.0);
    if rbox(x, y, a, 0.0, a + w, H + 6.0, [1.0, 4.5, 0.0, 0.0]) < 0.0
        && rbox(x, y, a + S, S, a + 40.0, H + 40.0, [0.5, 0.0, 0.0, 0.0]) > 0.0
    {
        return K::Ink;
    }
    // i
    let a = 32.0;
    if rbox(x, y, a, 0.0, a + S, H + 6.0, [0.6, 0.6, 0.0, 0.0]) < 0.0 {
        return K::Ink;
    }
    if rbox(x, y, a, -3.0 * ASP, a + S, -ASP, [0.7; 4]) < 0.0 {
        return K::Green;
    }
    K::Empty
}
const WORD_W: f32 = 35.0;

// ------------------------------------------------------------ rasterising

/// Majority class of each quadrant, sampled 3x3. `f` takes units (x in
/// cells, y in half cells).
fn quads(f: &dyn Fn(f32, f32) -> K, cx: i32, cy: i32) -> [K; 4] {
    let mut out = [K::Empty; 4];
    for (q, slot) in out.iter_mut().enumerate() {
        let (qx, qy) = ((q % 2) as f32, (q / 2) as f32);
        let mut counts: Vec<(K, i32)> = vec![];
        for j in 0..3 {
            for i in 0..3 {
                let x = cx as f32 + qx * 0.5 + (i as f32 + 0.5) / 6.0;
                let y = 2.0 * cy as f32 + qy + (j as f32 + 0.5) / 3.0;
                let k = f(x, y);
                match counts.iter_mut().find(|c| c.0 == k) {
                    Some(c) => c.1 += 1,
                    None => counts.push((k, 1)),
                }
            }
        }
        counts.sort_by_key(|c| (-c.1, (c.0 == K::Empty) as i32));
        *slot = counts[0].0;
    }
    out
}

fn draw_quads(buf: &mut Buffer, x: i32, y: i32, q: [K; 4], grain: bool) {
    let mut counts: Vec<(K, i32)> = vec![];
    for k in q {
        match counts.iter_mut().find(|c| c.0 == k) {
            Some(c) => c.1 += 1,
            None => counts.push((k, 1)),
        }
    }
    if counts.len() == 1 {
        match q[0] {
            K::Empty => {}
            K::Rice if grain => {
                let h = hash(x * 7 + 3, y * 13 + 1);
                if h > 0.22 {
                    put(buf, x, y, " ", Style::default().bg(Color::Gray));
                    return;
                }
                let h = h / 0.22;
                let g = ["⠑", "⠊", "⠢", "⠔"][(h * 4.0) as usize % 4];
                put(buf, x, y, g, Style::default().fg(Color::White).bg(Color::Gray));
            }
            K::Rice | K::Sheen | K::Nori => put(buf, x, y, " ", Style::default().bg(q[0].color())),
            K::Ink | K::Green => put(buf, x, y, "█", Style::default().fg(q[0].color())),
        }
        return;
    }
    counts.sort_by_key(|c| (-c.1, (c.0 == K::Empty) as i32));
    let (mut a, mut b) = (counts[0].0, counts[1].0);
    let map = |k: K| -> K {
        if k == a || k == b {
            k
        } else if k.group() == a.group() {
            a
        } else if k.group() == b.group() {
            b
        } else if a == K::Empty {
            b
        } else {
            a
        }
    };
    let q = q.map(map);
    if a == K::Empty {
        std::mem::swap(&mut a, &mut b);
    }
    let mask = q.iter().enumerate().filter(|(_, k)| **k == a).fold(0, |m, (i, _)| m | (1 << i));
    let mut style = Style::default().fg(a.color());
    if b != K::Empty {
        style = style.bg(b.color());
    }
    put(buf, x, y, QUAD[mask], style);
}

fn braille(bits: u32) -> String {
    char::from_u32(0x2800 + bits).unwrap_or(' ').to_string()
}

// ------------------------------------------------------------------ ball

/// A ball placed on the grid: centre x in cells, top in half cells, size in
/// cell widths.
#[derive(Clone, Copy)]
struct Place {
    cx: f32,
    top: f32,
    s: f32,
}

/// Vertical extent of the unit ball, (top, bottom) in v.
fn ball_extent() -> (f32, f32) {
    let (mut lo, mut hi) = (1.0f32, -1.0f32);
    for j in 0..400 {
        let v = -1.0 + j as f32 / 200.0;
        if (0..200).any(|i| sd_ball(-0.7 + i as f32 / 142.0, v) < 0.0) {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    (lo, hi)
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
        let (vtop, vbot) = ball_extent();
        let half_cells = (vbot - vtop) * p.s / ASP;
        let r0 = (p.top / 2.0).floor() as i32;
        let r1 = ((p.top + half_cells) / 2.0).ceil() as i32;
        let half_w = 0.62 * p.s;
        let band_rows = ((r1 - r0) as f32 * BAND_FRAC).round() as i32;
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
    /// Cell-space point to ball units, squashed about the bottom centre.
    fn local(&self, x: f32, y: f32, squash: f32) -> (f32, f32) {
        let u = (x - self.p.cx) / self.p.s;
        let v = (y - self.p.top) * ASP / self.p.s + self.vtop;
        let (sx, sy) = (1.0 + 0.06 * squash, 1.0 - 0.10 * squash);
        let (_, vbot) = ball_extent_cached();
        (u / sx, vbot + (v - vbot) / sy)
    }
}

fn ball_extent_cached() -> (f32, f32) {
    use std::sync::OnceLock;
    static E: OnceLock<(f32, f32)> = OnceLock::new();
    *E.get_or_init(ball_extent)
}

/// How one rice cell is drawn.
enum Rice {
    Dots(u32, Style),
    Fill(Color),
}

fn style_mode() -> String {
    std::env::var("ONI_STYLE").unwrap_or_else(|_| "solid".into())
}

/// Rice of one cell: solid inside, Braille dots on the rim so the outline
/// stays soft and fine, dimmer dots in the shaded lower right.
fn rice_cell(g: &Geo, cx: i32, cy: i32, squash: f32, texture: bool) -> Option<Rice> {
    let mut bits = 0;
    let mut dmax = -1.0f32;
    for (r, row) in BRAILLE_BITS.iter().enumerate() {
        for (c, bit) in row.iter().enumerate() {
            let x = cx as f32 + (c as f32 + 0.5) / 2.0;
            let y = 2.0 * cy as f32 + (r as f32 + 0.5) / 2.0;
            let (u, v) = g.local(x, y, squash);
            let d = sd_ball(u, v);
            dmax = dmax.max(d);
            if d >= 0.0 {
                continue;
            }
            let bx = cx * 2 + c as i32;
            let by = cy * 4 + r as i32;
            if texture && d < -0.03 && hash(bx * 5 + 1, by * 3 + 7) < DROPOUT {
                continue;
            }
            bits |= bit;
        }
    }
    if bits == 0 {
        return None;
    }
    let (u, v) = g.local(cx as f32 + 0.5, 2.0 * cy as f32 + 1.0, squash);
    let d = sd_ball(u, v);
    let lit = -0.80 * u - 0.60 * (v - 0.02);
    let sheen = (lit > 0.20 && d > -0.11 && d < -0.03) || (u + 0.10).hypot((v + 0.22) * 0.9) < 0.03;
    let shade = 0.75 * u + 0.66 * v > 0.30 && d > -0.09;
    let mode = style_mode();
    let interior = dmax < -0.5 / g.p.s;
    if mode == "hybrid" && interior && !shade {
        return Some(Rice::Fill(if sheen { Color::White } else { Color::Gray }));
    }
    let style = if sheen {
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
    } else if shade {
        Style::default().add_modifier(Modifier::DIM)
    } else {
        Style::default()
    };
    Some(Rice::Dots(bits, style))
}

fn rice_quads(g: &Geo, cx: i32, cy: i32, squash: f32) -> [K; 4] {
    let f = |x: f32, y: f32| {
        let (u, v) = g.local(x, y, squash);
        let d = sd_ball(u, v);
        if d >= 0.0 {
            return K::Empty;
        }
        let lit = -0.80 * u - 0.60 * (v - 0.02);
        if lit > 0.21 && d > -0.075 && d < -0.03 && u < -0.06 {
            K::Sheen
        } else {
            K::Rice
        }
    };
    quads(&f, cx, cy)
}

fn nori_quads(g: &Geo, cx: i32, cy: i32) -> [K; 4] {
    let f = |x: f32, y: f32| {
        let (u, v) = g.local(x, y, 0.0);
        if sd_ball(u, v) < NORI_GROW { K::Nori } else { K::Empty }
    };
    quads(&f, cx, cy)
}

fn draw_ball(buf: &mut Buffer, p: Place, t: f32, texture: bool) {
    let g = Geo::new(p);
    let squash = press(t);
    let steps = ((t - NORI_AT) / NORI_DUR * (g.rows.1 - 1 - g.band_top) as f32).ceil() as i32;
    let band_top = (g.rows.1 - 1 - steps.max(0)).max(g.band_top);
    let in_band =
        |cx: i32, cy: i32| t >= NORI_AT && cx >= g.band_cols.0 && cx < g.band_cols.1 && cy >= band_top;

    let front = if t >= PRESS_AT { 0.25 + 0.85 * ease_out((t - PRESS_AT) / PRESS_FRONT) } else { -1.0 };
    let pressed = |cx: i32, cy: i32| {
        let (u, v) = g.local(cx as f32 + 0.5, 2.0 * cy as f32 + 1.0, 0.0);
        // The solid region grows as a scaled copy of the ball from its centre.
        front > 0.0 && sd_ball(u / front.max(0.01), v / front.max(0.01)) < 0.0
    };

    let mut loose: Vec<((i32, i32), u32, i32)> = vec![];
    if front < 1.09 {
        loose = grains(&g, t);
    }
    for (cell, bits, landed) in loose {
        if pressed(cell.0, cell.1) || cell.1 >= g.rows.1 + 2 {
            continue;
        }
        let style = if landed > 0 { Style::default() } else { Style::default().add_modifier(Modifier::DIM) };
        put(buf, cell.0, cell.1, &braille(bits), style);
    }
    for cy in g.rows.0..g.rows.1 {
        for cx in g.cols.0..g.cols.1 {
            if in_band(cx, cy) {
                draw_quads(buf, cx, cy, nori_quads(&g, cx, cy), false);
            } else if pressed(cx, cy) && style_mode() == "solid" {
                draw_quads(buf, cx, cy, rice_quads(&g, cx, cy, squash), texture);
            } else if pressed(cx, cy) {
                match rice_cell(&g, cx, cy, squash, texture) {
                    Some(Rice::Dots(bits, style)) => put(buf, cx, cy, &braille(bits), style),
                    Some(Rice::Fill(c)) => put(buf, cx, cy, " ", Style::default().bg(c)),
                    None => {}
                }
            }
        }
    }
}

/// Loose grains before the press: a subset of the ball's dots flying in from
/// a larger, looser cloud of the same shape. Returns per-cell dot bits and
/// whether more dots there have landed than are still flying.
fn grains(g: &Geo, t: f32) -> Vec<((i32, i32), u32, i32)> {
    use std::collections::HashMap;
    let mut cells: HashMap<(i32, i32), (u32, i32)> = HashMap::new();
    let (vtop, vbot) = ball_extent_cached();
    let mid_y = g.p.top + (vbot - vtop) * g.p.s / ASP * 0.55;
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
            let ang = h3 * std::f32::consts::TAU;
            let spread = 0.25 + 0.45 * h2;
            let sx = x + (x - g.p.cx) * spread + ang.cos() * 3.0;
            let sy = y + (y - mid_y) * spread + ang.sin() * 3.0;
            let (px, py) = (sx + (x - sx) * k, sy + (y - sy) * k);
            let (dbx, dby) = ((px * 2.0).floor() as i32, (py * 2.0).floor() as i32);
            let e = cells.entry((dbx.div_euclid(2), dby.div_euclid(4))).or_insert((0, 0));
            e.0 |= BRAILLE_BITS[dby.rem_euclid(4) as usize][dbx.rem_euclid(2) as usize];
            e.1 += if k > 0.97 { 1 } else { -1 };
        }
    }
    cells.into_iter().map(|(c, (b, l))| (c, b, l)).collect()
}

// ------------------------------------------------------------- wordmark

/// Wordmark with its x-height top at half-cell `top`, left edge at `left`.
fn draw_word(buf: &mut Buffer, left: i32, top: i32, t: f32, scale: f32) {
    let f = |x: f32, y: f32| word_class((x - left as f32) / scale, (y - top as f32) * ASP / scale);
    let w = (WORD_W * scale).ceil() as i32;
    let y0 = top.div_euclid(2) - 3;
    let y1 = (top + (10.0 * scale) as i32).div_euclid(2) + 1;
    for cy in y0..y1 {
        for cx in left..left + w + 1 {
            let xw = (cx - left) as f32 / scale;
            let letter = [11.0, 23.0, 31.5].iter().filter(|b| xw >= **b).count() as f32;
            let at = WORD_AT + WORD_SWEEP / 3.0 * letter + 25.0 * hash(cx, cy);
            let q = quads(&f, cx, cy);
            let is_dot = q.contains(&K::Green);
            if is_dot {
                draw_dot(buf, cx, cy, q, t, &f);
                continue;
            }
            if t < at {
                continue;
            }
            if t < at + WORD_FLASH {
                let mut bits = 0;
                for (r, row) in BRAILLE_BITS.iter().enumerate() {
                    for (c, bit) in row.iter().enumerate() {
                        let x = cx as f32 + (c as f32 + 0.5) / 2.0;
                        let y = 2.0 * cy as f32 + (r as f32 + 0.5) / 2.0;
                        if f(x, y) == K::Ink {
                            bits |= bit;
                        }
                    }
                }
                if bits != 0 {
                    put(buf, cx, cy, &braille(bits), Style::default().add_modifier(Modifier::DIM));
                }
                continue;
            }
            draw_quads(buf, cx, cy, q, false);
        }
    }
}

/// The green i dot drops onto the stem last, like a sesame seed.
fn draw_dot(buf: &mut Buffer, cx: i32, cy: i32, q: [K; 4], t: f32, f: &dyn Fn(f32, f32) -> K) {
    if t < DOT_AT {
        return;
    }
    let k = clamp01((t - DOT_AT) / DOT_DUR);
    if k >= 1.0 {
        draw_quads(buf, cx, cy, q, false);
        return;
    }
    // Falling: sample the dot shifted up by a few half cells.
    let lift = (1.0 - ease_out(k)) * 5.0;
    let g = |x: f32, y: f32| match f(x, y + lift) {
        K::Green => K::Green,
        _ => K::Empty,
    };
    for dy in -3..=0 {
        draw_quads(buf, cx, cy + dy, quads(&g, cx, cy + dy), false);
    }
}

// --------------------------------------------------------------- screens

fn render(t: u64, area: Rect, buf: &mut Buffer, settled: bool) {
    let t = if settled { 1.0e6 } else { t as f32 };
    let dim = Style::default().add_modifier(Modifier::DIM);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let ver_t = WORD_AT + WORD_SWEEP + 120.0;
    if area.width >= 90 && area.height >= 26 {
        // Hero: ball left, wordmark right on the ball's baseline.
        let ox = area.x as i32 + (area.width as i32 - 80) / 2;
        let ball = Place { cx: ox as f32 + 17.5, top: 10.0, s: 33.0 };
        draw_ball(buf, ball, t, true);
        let word_left = ox + 43;
        let (vtop, vbot) = ball_extent_cached();
        let base = (ball.top + (vbot - vtop) * ball.s / ASP).round() as i32; // half cells
        draw_word(buf, word_left, base - 9, t, 1.0);
        if t >= ver_t {
            put(buf, word_left, base / 2 + 1, "cli", Style::default());
            buf.set_string((word_left + 4) as u16, (base / 2 + 1) as u16, VERSION, dim);
            buf.set_string((word_left + 13) as u16, (base / 2 + 1) as u16, "~/tidepool", dim);
        }
        if area.height >= 28 {
            // The everyday header, shown here as a specimen.
            let y = area.y as i32 + area.height as i32 - 6;
            let rule = "╌".repeat(area.width as usize - 4);
            buf.set_string(area.x + 2, y as u16, rule, dim);
            buf.set_string(area.x + 2, y as u16 + 1, " everyday header ", dim);
            draw_mini(buf, area.x as i32 + 3, y + 2);
        }
    } else {
        // Narrow: smaller ball, text wordmark centred under it.
        let cx = area.x as f32 + area.width as f32 / 2.0;
        let h = ((area.height as f32 - 6.0) * 2.0).clamp(10.0, 20.0);
        draw_ball(buf, Place { cx, top: 2.0, s: h }, t, false);
        let (vtop, vbot) = ball_extent_cached();
        let y = area.y + ((2.0 + (vbot - vtop) * h / ASP) / 2.0).round() as u16 + 2;
        if t >= WORD_AT {
            let x = (cx - 9.0) as u16;
            buf.set_string(x, y, "nor", bold);
            buf.set_string(x + 3, y, "ı", bold);
            if t >= DOT_AT {
                put(buf, x as i32 + 3, y as i32 - 1, "▄", Style::default().fg(Color::Green));
            }
            buf.set_string(x + 5, y, "cli", Style::default());
            buf.set_string(x + 9, y, VERSION, dim);
        }
    }
}

/// Four-row ball with a one-line wordmark: the everyday header.
fn draw_mini(buf: &mut Buffer, x: i32, y: i32) {
    let p = Place { cx: x as f32 + 4.5, top: (y * 2) as f32, s: 9.0 };
    draw_ball(buf, p, 1.0e6, false);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let dim = Style::default().add_modifier(Modifier::DIM);
    let tx = (x + 11) as u16;
    buf.set_string(tx, (y + 1) as u16, "nori", bold);
    buf.set_string(tx + 5, (y + 1) as u16, "cli", Style::default());
    buf.set_string(tx + 9, (y + 1) as u16, VERSION, dim);
    buf.set_string(tx, (y + 2) as u16, "~/tidepool", dim);
    buf.set_string(tx + 11, (y + 2) as u16, "·", dim);
    buf.set_string(tx + 13, (y + 2) as u16, "Claude Code", dim);
}

// ------------------------------------------------------------------ cast

#[derive(Clone, Default)]
struct Sink(Rc<RefCell<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn json_str(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn write_cast(path: &str, (w, h): (u16, u16), until: u64, settled: bool) {
    let sink = Sink::default();
    let mut term = Terminal::with_options(
        CrosstermBackend::new(sink.clone()),
        TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, w, h)) },
    )
    .expect("terminal");
    let mut f = File::create(path).expect("cast");
    writeln!(f, "{{\"version\": 2, \"width\": {w}, \"height\": {h}, \"env\": {{\"TERM\": \"xterm-256color\"}}}}").unwrap();
    writeln!(f, "[0.0, \"o\", {}]", json_str(b"\x1b[?25l")).unwrap();
    let frame = 40;
    let mut t = 0;
    while t <= until {
        term.draw(|fr| {
            let area = fr.area();
            render(t, area, fr.buffer_mut(), settled);
        })
        .expect("draw");
        let bytes = std::mem::take(&mut *sink.0.borrow_mut());
        if !bytes.is_empty() {
            writeln!(f, "[{:.3}, \"o\", {}]", t as f64 / 1000.0, json_str(&bytes)).unwrap();
        }
        t += frame;
    }
    writeln!(f, "[{:.3}, \"o\", \"\"]", until as f64 / 1000.0 + 0.5).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut cast = None;
    let mut size = (100, 30);
    let mut until = SETTLED_MS + 1500;
    let mut settled = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--cast" => {
                cast = args.get(i + 1).cloned();
                i += 1;
            }
            "--size" => {
                let v = &args[i + 1];
                let (a, b) = v.split_once('x').expect("WxH");
                size = (a.parse().expect("w"), b.parse().expect("h"));
                i += 1;
            }
            "--until" => {
                until = args[i + 1].parse().expect("ms");
                i += 1;
            }
            "--static" => settled = true,
            other => panic!("unknown arg {other}"),
        }
        i += 1;
    }
    let path = cast.expect("--cast FILE");
    write_cast(&path, size, if settled { 0 } else { until }, settled);
}
