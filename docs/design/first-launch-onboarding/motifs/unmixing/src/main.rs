//! `unmixing` motif: the Taylor–Couette demo applied to the `nori` wordmark.
//!
//! The wordmark is a set of dye particles on the Braille dot grid (2×4 dots
//! per cell; dots are square). A laminar annular shear around the word's
//! centre turns each particle by an angle that depends only on its radius and
//! on a single shear amount `s(t)`. `s` rises, holds, and falls back to zero,
//! so the reverse leg is the exact inverse map and every particle lands on the
//! dot it started from. Rendering is a pure function of elapsed milliseconds.
//!
//! Modes:
//!   --cast FILE      deterministic asciicast of the full cycle
//!   --text MS        print the frame at MS as plain text
//! Options:
//!   --size WxH       (default 100x30)
//!   --until MS       end of cast (default: cycle end + 400)
//!   --static         render the settled frame only (reduced motion)

use std::cell::RefCell;
use std::f32::consts::TAU;
use std::fs::File;
use std::io::{self, Write};
use std::rc::Rc;

use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::{Terminal, TerminalOptions, Viewport};

const VERSION: &str = "v0.42.0";

const HOLD_IN_MS: i32 = 300;
const STIR_MS: i32 = 1050;
const PAUSE_MS: i32 = 400;
const FRAME_MS: i32 = 40;

/// Dye particles per dot edge.
const SUB: i32 = 3;
/// Turns made by the innermost dye at full shear.
const INNER_TURNS: f32 = 1.6;
/// Vertical squash of the annulus so the swirl stays in the word's band.
const SQUASH: f32 = 0.42;

const BRAILLE_BITS: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
const QUAD: [&str; 16] = [
    " ", "▘", "▝", "▀", "▖", "▌", "▞", "▛", "▗", "▚", "▐", "▜", "▄", "▙", "▟", "█",
];

// ---------------------------------------------------------------- letters

/// Superellipse "radius": 1.0 on the boundary.
fn sq(x: f32, y: f32, ax: f32, ay: f32) -> f32 {
    const P: f32 = 2.6;
    ((x / ax).abs().powf(P) + (y / ay).abs().powf(P)).powf(1.0 / P)
}

/// Ring of a superellipse with outer half-extents (ax, ay) and stroke t.
fn ring(x: f32, y: f32, ax: f32, ay: f32, tx: f32, ty: f32) -> bool {
    sq(x, y, ax, ay) <= 1.0 && sq(x, y, ax - tx, ay - ty) > 1.0
}

/// Letter geometry in cell-width units (a cell is 1 wide, 2 tall),
/// y down, baseline at 0, x-height top at -XH.
const XH: f32 = 14.0;
const STEM: f32 = 3.5; // vertical stroke width
const BAR: f32 = 3.0; // horizontal stroke thickness
const GAP: f32 = 1.5;

struct Glyph {
    w: f32,
    ink: fn(f32, f32) -> bool,
}

const N_W: f32 = 12.0;
const O_W: f32 = 13.0;
const R_W: f32 = 8.5;

fn ink_n(x: f32, y: f32) -> bool {
    let ah = 6.5; // arch half-height
    let cy = -XH + ah;
    let cx = N_W / 2.0;
    let stem_l = x < STEM && y > -XH;
    let stem_r = x > N_W - STEM && y > cy;
    let arch = y <= cy && ring(x - cx, y - cy, N_W / 2.0, ah, STEM, BAR);
    (stem_l || stem_r || arch) && y <= 0.0
}

fn ink_o(x: f32, y: f32) -> bool {
    ring(x - O_W / 2.0, y + XH / 2.0, O_W / 2.0, XH / 2.0, STEM, BAR)
}

fn ink_r(x: f32, y: f32) -> bool {
    let ah = 6.5;
    let cy = -XH + ah;
    let cx = N_W / 2.0;
    let stem = x < STEM && y > -XH && y <= 0.0;
    let arch = y <= cy && x <= R_W && ring(x - cx, y - cy, N_W / 2.0, ah, STEM, BAR);
    let arch = arch && !(x > R_W - 1.0 && y > -XH + BAR + 0.2);
    stem || arch
}

fn ink_i(x: f32, y: f32) -> bool {
    let stem = x < STEM && y > -XH && y <= 0.0;
    let dot = sq(x - STEM / 2.0, y + XH + 2.5, STEM / 2.0, 1.6) <= 1.05;
    stem || dot
}

const GLYPHS: [Glyph; 4] = [
    Glyph { w: N_W, ink: ink_n },
    Glyph { w: O_W, ink: ink_o },
    Glyph { w: R_W, ink: ink_r },
    Glyph { w: STEM, ink: ink_i },
];

/// Top of the mark above the baseline, in rows.
const MARK_TOP_ROWS: i32 = 9;
const MARK_ROWS: i32 = MARK_TOP_ROWS;

fn mark_width_cells() -> i32 {
    let w: f32 = GLYPHS.iter().map(|g| g.w).sum::<f32>() + GAP * 3.0;
    w.ceil() as i32
}

/// Particles in dot coordinates relative to the mark's top-left dot.
/// Sampled at quadrant resolution so the settled mark is pure block glyphs.
fn particles() -> Vec<(f32, f32)> {
    let cols = mark_width_cells() * 2;
    let rows = MARK_ROWS * 2;
    let mut out = Vec::new();
    for qy in 0..rows {
        for qx in 0..cols {
            let px = (qx as f32 + 0.5) * 0.5;
            let py = (qy as f32 + 0.5) - (MARK_TOP_ROWS * 2) as f32;
            let mut x0 = 0.0;
            let mut hit = false;
            for g in &GLYPHS {
                if px >= x0 && px < x0 + g.w && (g.ink)(px - x0, py) {
                    hit = true;
                }
                x0 += g.w + GAP;
            }
            if hit {
                for dy in 0..2 * SUB {
                    for dx in 0..SUB {
                        out.push((
                            qx as f32 + (dx as f32 + 0.5) / SUB as f32,
                            (qy * 2) as f32 + (dy as f32 + 0.5) / SUB as f32,
                        ));
                    }
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------- flow

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Shear amount in [0, 1]. Symmetric in time, so the return is exact.
fn shear(t: i32) -> f32 {
    let a = HOLD_IN_MS;
    let b = a + STIR_MS;
    let c = b + PAUSE_MS;
    let d = c + STIR_MS;
    if t <= a || t >= d {
        0.0
    } else if t < b {
        smooth((t - a) as f32 / STIR_MS as f32)
    } else if t <= c {
        1.0
    } else {
        smooth((d - t) as f32 / STIR_MS as f32)
    }
}

pub fn cycle_end_ms() -> i32 {
    HOLD_IN_MS + 2 * STIR_MS + PAUSE_MS
}

/// Angular velocity profile: fastest at the centre, still at the outer wall.
/// Linear in radius, which keeps the spiral arms evenly spaced.
fn omega(r: f32, r_out: f32) -> f32 {
    (1.0 - r / r_out).max(0.0)
}

struct Flow {
    cx: f32,
    cy: f32,
    r_out: f32,
}

impl Flow {
    fn new(pts: &[(f32, f32)]) -> Self {
        let n = pts.len() as f32;
        let cx = pts.iter().map(|p| p.0).sum::<f32>() / n;
        let cy = pts.iter().map(|p| p.1).sum::<f32>() / n;
        let rmax = pts
            .iter()
            .map(|p| (p.0 - cx).hypot((p.1 - cy) / SQUASH))
            .fold(0.0, f32::max);
        Flow { cx, cy, r_out: rmax * 1.25 }
    }

    /// Where a particle that starts at `p` sits at shear `s`.
    fn at(&self, p: (f32, f32), s: f32) -> (i32, i32) {
        if s == 0.0 {
            return (p.0.floor() as i32, p.1.floor() as i32);
        }
        let dx = p.0 - self.cx;
        let dy = (p.1 - self.cy) / SQUASH;
        let r = dx.hypot(dy);
        let th = dy.atan2(dx) + s * INNER_TURNS * TAU * omega(r, self.r_out);
        let x = self.cx + r * th.cos();
        let y = self.cy + r * th.sin() * SQUASH;
        (x.floor() as i32, y.floor() as i32)
    }
}

// ---------------------------------------------------------------- render

struct Scene {
    pts: Vec<(f32, f32)>,
    flow: Flow,
}

fn render(scene: &Scene, t: i32, area: Rect, buf: &mut Buffer) {
    let mw = mark_width_cells();
    let total_rows = MARK_ROWS + 2;
    let ox = area.x as i32 + (area.width as i32 - mw) / 2;
    let oy = area.y as i32 + (area.height as i32 - total_rows) / 2;

    let s = shear(t);
    let cw = area.width as usize;
    let ch = area.height as usize;
    let dw = cw * 2;
    // Particles per Braille dot.
    let mut dots = vec![0u16; dw * ch * 4];
    for &p in &scene.pts {
        let (x, y) = scene.flow.at(p, s);
        let gx = x + (ox - area.x as i32) * 2;
        let gy = y + (oy - area.y as i32) * 4;
        if gx < 0 || gy < 0 || gx >= dw as i32 || gy >= ch as i32 * 4 {
            continue;
        }
        dots[gy as usize * dw + gx as usize] += 1;
    }
    let full = (SUB * SUB) as f32;

    for cy in 0..ch {
        for cx in 0..cw {
            let mut bits = 0u32;
            let mut ink = 0.0;
            for (dy, row) in BRAILLE_BITS.iter().enumerate() {
                for (dx, bit) in row.iter().enumerate() {
                    let n = dots[(cy * 4 + dy) * dw + cx * 2 + dx];
                    if n > 0 {
                        bits |= bit;
                        ink += (n as f32 / full).min(1.0);
                    }
                }
            }
            if bits == 0 {
                continue;
            }
            let lit = bits.count_ones() as f32;
            let glyph: String = if s == 0.0 {
                // Settled: every cell is quadrant-aligned, so draw solid blocks.
                let idx = [0x01 | 0x02, 0x08 | 0x10, 0x04 | 0x40, 0x20 | 0x80]
                    .iter()
                    .enumerate()
                    .fold(0, |acc, (k, &m)| acc | (usize::from(bits & m == m) << k));
                QUAD[idx].to_string()
            } else {
                char::from_u32(0x2800 + bits).unwrap_or(' ').to_string()
            };
            // Dye density: how much of each lit dot is still full strength.
            // Thin filaments and half-empty dots read as diluted dye.
            let mut style = Style::new().fg(Color::Green);
            if s > 0.0 && (ink / lit < 0.45 || lit <= 3.0) {
                style = style.add_modifier(Modifier::DIM);
            }
            buf[(area.x + cx as u16, area.y + cy as u16)].set_symbol(&glyph).set_style(style);
        }
    }

    // Version line, small and dim, right-aligned under the mark.
    let label = format!("cli {VERSION}");
    let lx = ox + mw - label.len() as i32;
    let ly = oy + MARK_ROWS + 1;
    if ly < (area.y + area.height) as i32 && lx >= area.x as i32 {
        if s == 0.0 {
            buf.set_string(lx as u16, ly as u16, &label, Style::new().fg(Color::Reset).add_modifier(Modifier::DIM));
        }
    }
}

// ---------------------------------------------------------------- output

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

fn write_cast(scene: &Scene, w: u16, h: u16, until: i32, still: bool, path: &str) {
    let sink = Sink::default();
    let mut term = Terminal::with_options(
        CrosstermBackend::new(sink.clone()),
        TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, w, h)) },
    )
    .expect("terminal");
    let mut f = File::create(path).expect("cast");
    writeln!(f, "{{\"version\": 2, \"width\": {w}, \"height\": {h}, \"env\": {{\"TERM\": \"xterm-256color\"}}}}").unwrap();
    writeln!(f, "[0.0, \"o\", {}]", json_str(b"\x1b[?25l")).unwrap();
    let mut t = 0;
    while t <= until {
        let tt = if still { 0 } else { t };
        term.draw(|fr| {
            let area = fr.area();
            render(scene, tt, area, fr.buffer_mut());
        })
        .expect("draw");
        let bytes = std::mem::take(&mut *sink.0.borrow_mut());
        if !bytes.is_empty() {
            writeln!(f, "[{:.3}, \"o\", {}]", t as f64 / 1000.0, json_str(&bytes)).unwrap();
        }
        t += FRAME_MS;
    }
    writeln!(f, "[{:.3}, \"o\", \"\"]", until as f64 / 1000.0 + 0.3).unwrap();
}

fn print_text(scene: &Scene, w: u16, h: u16, t: i32) {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    render(scene, t, area, &mut buf);
    for y in 0..h {
        let line: String = (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect();
        println!("{}", line.trim_end());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut size = (100u16, 30u16);
    let mut until = cycle_end_ms() + 400;
    let mut cast = None;
    let mut text = None;
    let mut still = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--size" => {
                let (a, b) = args[i + 1].split_once('x').expect("WxH");
                size = (a.parse().expect("w"), b.parse().expect("h"));
                i += 1;
            }
            "--until" => {
                until = args[i + 1].parse().expect("ms");
                i += 1;
            }
            "--cast" => {
                cast = Some(args[i + 1].clone());
                i += 1;
            }
            "--text" => {
                text = Some(args[i + 1].parse::<i32>().expect("ms"));
                i += 1;
            }
            "--static" => still = true,
            other => panic!("unknown flag {other}"),
        }
        i += 1;
    }
    let pts = particles();
    let flow = Flow::new(&pts);
    let scene = Scene { pts, flow };

    // Reversibility check: the map at s = 0 after the cycle is the identity.
    let start: Vec<_> = scene.pts.iter().map(|&p| scene.flow.at(p, 0.0)).collect();
    let back: Vec<_> = scene.pts.iter().map(|&p| scene.flow.at(p, shear(cycle_end_ms()))).collect();
    assert_eq!(back, start);

    if let Some(t) = text {
        print_text(&scene, size.0, size.1, t);
    }
    if let Some(path) = cast {
        write_cast(&scene, size.0, size.1, until, still, &path);
    }
}

