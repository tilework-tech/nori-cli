//! Moiré wordmark motif for the Nori CLI first launch.
//!
//! Two fine line gratings, sampled at Braille resolution (2x4 dots per cell),
//! drift over each other at slightly different angles and pitches inside a
//! bounded 60x12 cell box. Grating B is shifted by half a period inside the
//! letter pixels of `nori`, so the word exists only in the interference of the
//! two (XOR per dot). The angles converge, then snap equal: the background
//! cancels to empty, the word locks solid, and the cells resolve to crisp
//! quadrant blocks with a green i dot. The frame is a pure function of
//! elapsed milliseconds.
//!
//!   --cast FILE    write a deterministic asciicast
//!   --size WxH     cast size (default 100x30)
//!   --until MS     end of the cast (default 3000)
//!   --static       draw the settled frame from t=0 (reduced motion)

use std::cell::RefCell;
use std::f32::consts::PI;
use std::fs::File;
use std::io::{self, Write};
use std::rc::Rc;

use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::{Terminal, TerminalOptions, Viewport};

const VERSION: &str = "v0.42.0";
const BOX_W: u16 = 60;
const BOX_H: u16 = 12;

const LOCK_MS: f32 = 1900.0;
const FADE_MS: f32 = 320.0;
const RESOLVE_MS: f32 = 420.0;

const BRAILLE_BITS: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
const QUADS: [&str; 16] = [
    " ", "▘", "▝", "▀", "▖", "▌", "▞", "▛", "▗", "▚", "▐", "▜", "▄", "▙", "▟", "█",
];

// ---------------------------------------------------------------- wordmark

const TOP: f32 = 14.0;
const BOT: f32 = 42.0;
const MID: f32 = 28.0;
const X_N: f32 = 12.0;
const X_O: f32 = X_N + 26.0 + 5.0;
const X_R: f32 = X_O + 28.0 + 5.0;
const X_I: f32 = X_R + 19.0 + 5.0;

fn rect(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> bool {
    x >= x0 && x < x1 && y >= y0 && y < y1
}

fn ring(x: f32, y: f32, cx: f32, cy: f32, r_in: f32, r_out: f32) -> bool {
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    d >= r_in && d < r_out
}

/// Is the point (in dot units) inside the i dot?
fn in_dot(x: f32, y: f32) -> bool {
    ((x - (X_I + 4.0)).powi(2) + (y - 7.0).powi(2)).sqrt() < 4.6
}

/// Is the point (in dot units) inside a letter of `nori`?
fn in_word(x: f32, y: f32) -> bool {
    // n: stem, right leg, arch.
    let n = rect(x, y, X_N, TOP, X_N + 8.0, BOT)
        || rect(x, y, X_N + 18.0, MID - 1.0, X_N + 26.0, BOT)
        || (y < MID && ring(x, y, X_N + 13.0, MID - 1.0, 5.0, 13.0));
    // o: ring.
    let o = ring(x, y, X_O + 14.0, MID, 6.0, 14.0);
    // r: stem, shoulder, terminal.
    let r = rect(x, y, X_R, TOP, X_R + 8.0, BOT)
        || (y < MID && x < X_R + 14.0 && ring(x, y, X_R + 14.0, MID, 6.0, 14.0))
        || rect(x, y, X_R + 14.0, TOP, X_R + 19.0, TOP + 8.0);
    // i: stem and dot.
    let i = rect(x, y, X_I, TOP, X_I + 8.0, BOT) || in_dot(x, y);
    n || o || r || i
}

// ---------------------------------------------------------------- gratings

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

struct Grating {
    angle: f32,
    pitch: f32,
    phase: f32,
}

impl Grating {
    /// Signed phase in periods at a dot position.
    fn u(&self, x: f32, y: f32) -> f32 {
        ((x - 60.0) * self.angle.cos() + (y - 24.0) * self.angle.sin()) / self.pitch + self.phase
    }
}

fn frac(v: f32) -> f32 {
    v - v.floor()
}

struct Field {
    a: Grating,
    b: Grating,
}

/// Grating parameters at time `t` ms. Before the lock the two gratings
/// differ in angle and pitch and drift in phase; at LOCK_MS they snap equal.
fn field(t: f32) -> Field {
    // At the locked angle and pitch every dot centre sits a quarter of a
    // stripe from an edge, so the carrier samples cleanly as 2-on 2-off.
    let base = (45.0f32).to_radians();
    let pitch = 2.0 * std::f32::consts::SQRT_2;
    if t >= LOCK_MS {
        let g = || Grating { angle: base, pitch, phase: 0.125 };
        return Field { a: g(), b: g() };
    }
    let s = smooth(t / LOCK_MS);
    // Fast convergence early, slow near the end so the bands are wide and
    // slow when the word is most legible.
    let e = 1.0 - (1.0 - s).powf(1.6);
    let d_ang = (9.0 + (1.4 - 9.0) * e).to_radians();
    let d_pitch = 0.05 * (1.0 - e);
    let swing = (1.0 - e) * (-24.0f32).to_radians();
    let ts = t / 1000.0;
    // Phase drift moves the bands; it decays to zero at the lock so the
    // snap lands on an already-quiet background.
    let drift = t / LOCK_MS + 0.08 * (ts * PI * 1.3).sin() * (1.0 - e);
    Field {
        a: Grating { angle: base + swing + d_ang * 0.5, pitch: pitch * (1.0 + d_pitch), phase: 0.125 },
        b: Grating { angle: base + swing - d_ang * 0.5, pitch: pitch * (1.0 - d_pitch), phase: 0.125 + drift },
    }
}

/// XOR of the two gratings at one dot; B is half a period out inside letters.
fn dot_on(f: &Field, x: f32, y: f32) -> bool {
    let shift = if in_word(x, y) { 0.5 } else { 0.0 };
    let a = frac(f.a.u(x, y)) < 0.5;
    let b = frac(f.b.u(x, y) + shift) < 0.5;
    a ^ b
}

/// Smooth moiré density (0..1) at a dot: the local XOR coverage, which is
/// what the eye integrates. Used only for tone.
fn band(f: &Field, x: f32, y: f32) -> f32 {
    let shift = if in_word(x, y) { 0.5 } else { 0.0 };
    let d = f.a.u(x, y) - f.b.u(x, y) - shift;
    let w = d - d.round();
    2.0 * w.abs()
}

fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

// ---------------------------------------------------------------- drawing

fn put(buf: &mut Buffer, x: u16, y: u16, s: &str, style: Style) {
    if buf.area.contains((x, y).into()) {
        if let Some(c) = buf.cell_mut((x, y)) {
            c.set_symbol(s).set_style(style);
        }
    }
}

fn green() -> Style {
    Style::default().fg(Color::Green)
}
fn dim() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}
fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// Quadrant glyph for a cell from the letter mask: each quadrant is one dot
/// column by two dot rows; it is on when either dot is inside.
fn quad_cell(cx: i32, cy: i32, mask: impl Fn(f32, f32) -> bool) -> usize {
    let mut q = 0;
    for (i, (qx, qy)) in [(0, 0), (1, 0), (0, 1), (1, 1)].iter().enumerate() {
        let x = (cx * 2 + qx) as f32 + 0.5;
        let y0 = (cy * 4 + qy * 2) as f32 + 0.5;
        if mask(x, y0) || mask(x, y0 + 1.0) {
            q |= 1 << i;
        }
    }
    q
}

fn draw_moire(buf: &mut Buffer, x0: u16, y0: u16, t: f32) {
    let f = field(t);
    let since_lock = t - LOCK_MS;
    for cy in 0..BOX_H as i32 {
        for cx in 0..BOX_W as i32 {
            let (sx, sy) = (x0 + cx as u16, y0 + cy as u16);
            let word_q = quad_cell(cx, cy, in_word);
            let dot_q = quad_cell(cx, cy, in_dot);
            // After the snap each letter locks whole to blocks, i first
            // (its dot turns green), then r, o, n.
            if since_lock >= 0.0 && word_q != 0 {
                let x = cx as f32 * 2.0;
                let order = if x >= X_I - 2.0 {
                    0.0
                } else if x >= X_R - 2.0 {
                    1.0
                } else if x >= X_O - 2.0 {
                    2.0
                } else {
                    3.0
                };
                let lock_at = 80.0 + order * (RESOLVE_MS - 80.0) / 4.0;
                if since_lock >= lock_at {
                    let style = if dot_q != 0 { green() } else { Style::default() };
                    put(buf, sx, sy, QUADS[word_q], style);
                    continue;
                }
            }
            let mut bits = 0u32;
            let mut tone = 0.0;
            for (r, row) in BRAILLE_BITS.iter().enumerate() {
                for (c, bit) in row.iter().enumerate() {
                    let x = (cx * 2 + c as i32) as f32 + 0.5;
                    let y = (cy * 4 + r as i32) as f32 + 0.5;
                    if dot_on(&f, x, y) {
                        bits |= bit;
                    }
                    tone += band(&f, x, y);
                }
            }
            if t < FADE_MS {
                // Fade in from an empty box: drop dots by a fixed noise rank.
                let k = smooth(t / FADE_MS);
                for (r, row) in BRAILLE_BITS.iter().enumerate() {
                    for (c, bit) in row.iter().enumerate() {
                        if hash(cx * 2 + c as i32, cy * 4 + r as i32) > k {
                            bits &= !bit;
                        }
                    }
                }
            }
            if bits == 0 {
                continue;
            }
            let tone = tone / 8.0;
            let style = if since_lock >= 0.0 {
                bold()
            } else if tone > 0.72 {
                bold()
            } else if tone > 0.42 {
                Style::default()
            } else if tone > 0.2 {
                green().add_modifier(Modifier::DIM)
            } else {
                dim()
            };
            let s = char::from_u32(0x2800 + bits).unwrap().to_string();
            put(buf, sx, sy, &s, style);
        }
    }
}

const OPTIONS: [(&str, &str); 8] = [
    ("Claude Code", "Anthropic"),
    ("Codex", "OpenAI"),
    ("Antigravity", "Google"),
    ("Grok Build", "xAI"),
    ("OpenCode", "any provider"),
    ("Cursor Agent", "Cursor"),
    ("Hermes Agent", "Nous Research"),
    ("Pi", "any provider"),
];

fn render(t: f32, area: Rect, buf: &mut Buffer) {
    let x0 = area.x + area.width.saturating_sub(BOX_W) / 2;
    let y0 = area.y + 1;
    draw_moire(buf, x0, y0, t);
    let vy = y0 + BOX_H + 1;
    let label = format!("cli {VERSION}");
    let vx = x0 + (X_N as u16) / 2;
    if t >= LOCK_MS + 200.0 {
        buf.set_string(vx, vy, "cli", Style::default());
        buf.set_string(vx + 4, vy, VERSION, dim());
    } else {
        buf.set_string(vx, vy, &label, dim());
    }
    let mut y = vy + 2;
    let mx = vx;
    buf.set_string(mx, y, "〉", green());
    buf.set_string(mx + 2, y, "Choose your first agent", bold());
    y += 1;
    buf.set_string(mx + 2, y, "Nori runs it for you. Switch any time with /agent.", dim());
    y += 2;
    let width = 3 + 12 + 2 + 13 + 2;
    for (i, (l, d)) in OPTIONS.iter().enumerate() {
        let ry = y + i as u16;
        if ry + 2 >= area.y + area.height {
            break;
        }
        if i == 0 {
            put(buf, mx, ry, "▏", green());
            buf.set_string(mx + 1, ry, "›", green().add_modifier(Modifier::BOLD));
            buf.set_string(mx + 3, ry, *l, bold());
            put(buf, mx + width - 1, ry, "▕", green());
        } else {
            buf.set_string(mx + 3, ry, *l, Style::default());
        }
        buf.set_string(mx + 3 + 12 + 2, ry, *d, dim());
    }
    y += OPTIONS.len() as u16 + 1;
    if y < area.y + area.height {
        buf.set_string(mx + 3, y, "↑↓ move  enter choose  q quit", dim());
    }
}

// ---------------------------------------------------------------- cast

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

fn main() {
    let mut size = (100u16, 30u16);
    let mut cast = String::from("moire.cast");
    let mut until = 3000u64;
    let mut frame_ms = 40u64;
    let mut stat = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut v = || args.next().expect("missing value");
        match a.as_str() {
            "--size" => {
                let s = v();
                let (w, h) = s.split_once('x').expect("WxH");
                size = (w.parse().unwrap(), h.parse().unwrap());
            }
            "--cast" => cast = v(),
            "--until" => until = v().parse().unwrap(),
            "--frame-ms" => frame_ms = v().parse().unwrap(),
            "--static" => stat = true,
            other => panic!("unknown flag {other}"),
        }
    }
    let (w, h) = size;
    let sink = Sink::default();
    let mut term = Terminal::with_options(
        CrosstermBackend::new(sink.clone()),
        TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, w, h)) },
    )
    .expect("terminal");
    let mut f = File::create(&cast).expect("cast");
    writeln!(f, "{{\"version\": 2, \"width\": {w}, \"height\": {h}, \"env\": {{\"TERM\": \"xterm-256color\"}}}}").unwrap();
    writeln!(f, "[0.0, \"o\", {}]", json_str(b"\x1b[?25l")).unwrap();
    let mut t = 0;
    while t <= until {
        let at = if stat { 10_000.0 } else { t as f32 };
        term.draw(|fr| render(at, fr.area(), fr.buffer_mut())).expect("draw");
        let bytes = std::mem::take(&mut *sink.0.borrow_mut());
        if !bytes.is_empty() {
            writeln!(f, "[{:.3}, \"o\", {}]", t as f64 / 1000.0, json_str(&bytes)).unwrap();
        }
        t += frame_ms;
    }
    writeln!(f, "[{:.3}, \"o\", \"\"]", until as f64 / 1000.0 + 0.05).unwrap();
}
