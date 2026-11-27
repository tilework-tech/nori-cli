//! Noren motif: setup opens behind a split shop curtain printed with `nori`.
//!
//! The curtain is a pixel shader over half-block cells (one cell = two square
//! pixels). Every frame is a pure function of elapsed milliseconds and the
//! time of the parting key, so stills can be cut at exact times.
//!
//! Usage:
//!   motif-noren --cast FILE [--size WxH] [--key MS|none] [--flick]
//!               [--until MS] [--letters white|knock]

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
const PANELS: i32 = 4;

const TITLE: &str = "Choose your first agent";
const SUBTITLE: &str = "Nori runs it for you. Switch any time with /agent.";
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
const HINT: &str = "↑↓ move  enter choose  q quit";

#[derive(Clone, Copy, PartialEq)]
enum Px {
    Clear,
    Bg,
    C(Color),
}

#[derive(Clone, Copy)]
struct Motion {
    key_ms: Option<f32>,
    /// Everyday startup: no idle, opens at t=0, fast, barely sways.
    flick: bool,
}

impl Motion {
    fn speed(&self) -> f32 {
        if self.flick { 0.5 } else { 1.0 }
    }

    /// Opening progress at depth `d` (0 = rod, 1 = hem). Deeper cloth lags
    /// the rod and overshoots once when it arrives.
    fn q(&self, t: f32, d: f32) -> f32 {
        let Some(k) = self.key_ms else { return 0.0 };
        let sp = self.speed();
        let tt = (t - k - 130.0 * sp * d.powf(1.3)) / 1000.0 / sp;
        if tt <= 0.0 {
            return 0.0;
        }
        let a = (tt / 0.8).min(1.0);
        let base = 1.0 - (1.0 - a).powi(3);
        let sway_amp = if self.flick { 0.03 } else { 0.13 };
        let st = tt - 0.62;
        let sway = if st > 0.0 { d * d * sway_amp * (-st * 3.2).exp() * (2.0 * PI * st * 1.25).sin() } else { 0.0 };
        base + sway
    }

    fn since_key(&self, t: f32) -> Option<f32> {
        self.key_ms.and_then(|k| (t >= k).then(|| t - k))
    }
}

struct Geo {
    s: f32,
    x0: f32,
    x1: f32,
    pw: f32,
    ow: f32,
    top: i32,
    bottom: i32,
    sleeve: i32,
    wx: f32,
    wy: f32,
}

impl Geo {
    fn new(w: u16, h: u16) -> Self {
        let s = ((w as f32 - 6.0) / 72.0).min(h as f32 / 30.0).min(1.2);
        let mx = (4.0 * s).round().max(1.0);
        let x0 = mx;
        let x1 = w as f32 - mx;
        let pw = (x1 - x0 - (PANELS - 1) as f32) / PANELS as f32;
        let ow = (7.0 * s).round().max(3.0);
        let top = 2;
        let bottom = top + (38.0 * s).round() as i32;
        let sleeve = (3.0 * s).round().max(1.0) as i32;
        let wx = (x0 + x1) / 2.0 - 65.0 * s / 2.0;
        let wy = top as f32 + 15.0 * s;
        Self { s, x0, x1, pw, ow, top, bottom, sleeve, wx, wy }
    }

    fn closed_left(&self, i: i32) -> f32 {
        self.x0 + i as f32 * (self.pw + 1.0)
    }

    fn open_left(&self, i: i32) -> f32 {
        match i {
            0 => self.x0,
            1 => self.x0 + self.ow + 1.0,
            2 => self.x1 - 2.0 * self.ow - 1.0,
            _ => self.x1 - self.ow,
        }
    }

    /// Right edge of the left bunch once parted; content starts after it.
    fn bunch_right(&self) -> f32 {
        self.x0 + 2.0 * self.ow + 1.0
    }
}

fn rbox(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
    let r = r.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
    let cx = x.clamp(x0 + r, x1 - r);
    let cy = y.clamp(y0 + r, y1 - r);
    let (dx, dy) = (x - cx, y - cy);
    x >= x0 && x <= x1 && y >= y0 && y <= y1 && dx * dx + dy * dy <= r * r
}

fn rect(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> bool {
    x >= x0 && x < x1 && y >= y0 && y < y1
}

/// Heavy rounded lowercase `nori` in word units: x-height 20, stem 6,
/// 65 wide. The i dot sits above y = 0.
fn letter(x: f32, y: f32) -> bool {
    // n
    if (0.0..18.0).contains(&x) {
        let arch = rbox(x, y, 0.0, 0.0, 18.0, 40.0, 9.0) && !rbox(x, y, 6.0, 6.5, 12.0, 40.0, 3.0);
        return (arch && (0.0..20.0).contains(&y)) || rect(x, y, 0.0, 0.0, 6.0, 20.0);
    }
    // o
    let ox = x - 21.0;
    if (0.0..18.0).contains(&ox) {
        return rbox(ox, y, 0.0, 0.0, 18.0, 20.0, 9.0) && !rbox(ox, y, 6.0, 6.5, 12.0, 13.5, 3.0);
    }
    // r
    let rx = x - 42.0;
    if (0.0..15.0).contains(&rx) {
        let shoulder = rbox(rx, y, 0.0, 0.0, 18.0, 40.0, 9.0) && !rbox(rx, y, 6.0, 6.5, 30.0, 40.0, 3.0);
        return rect(rx, y, 0.0, 0.0, 6.0, 20.0) || (shoulder && y < 6.5 + 0.6 * (15.0 - rx).max(0.0) && rx < 15.0);
    }
    // i
    let ix = x - 59.0;
    if (0.0..6.0).contains(&ix) {
        let (dx, dy) = (ix - 3.0, y + 6.5);
        return rect(ix, y, 0.0, 0.0, 6.0, 20.0) || dx * dx + dy * dy <= 3.4 * 3.4;
    }
    false
}

/// The curtain at pixel (px, py). Pixels are one column wide and half a row
/// tall.
fn curtain_px(g: &Geo, m: Motion, t: f32, px: i32, py: i32, white_letters: bool) -> Px {
    let x = px as f32 + 0.5;
    // Rod with round end knobs, behind the cloth.
    let rod = {
        let (ra, rb) = (g.x0 - 2.0, g.x1 + 2.0);
        if py == g.top + 1 && x >= ra && x < rb {
            Px::C(Color::DarkGray)
        } else if py == g.top && (x < ra + 1.0 && x >= ra || x >= rb - 1.0 && x < rb) {
            Px::C(Color::DarkGray)
        } else {
            Px::Clear
        }
    };
    if py < g.top || py > g.bottom + 2 {
        return rod;
    }
    let span = (g.bottom - g.top) as f32;
    let d = ((py - g.top) as f32 / span).clamp(0.0, 1.0);
    let q = m.q(t, d);
    let idle = if m.flick { 0.0 } else { 1.0 };
    // The whole hem lifts a little while the cloth is in flight.
    let qb = m.q(t, 1.0).clamp(0.0, 1.0);
    let lift = 4.0 * qb * (1.0 - qb) * 2.5 * g.s;
    for i in (0..PANELS).rev() {
        let lc = g.closed_left(i);
        let left = lc + (g.open_left(i) - lc) * q;
        let width = (g.pw + (g.ow - g.pw) * q).max(1.0);
        if x < left || x >= left + width {
            continue;
        }
        let u = (x - left) / width * g.pw;
        let gx = lc + u;
        let compress = g.pw / width;
        // Breeze ripple along the hem, one pixel either way, plus soft corners.
        let ripple = idle
            * (0.7 * (gx * 0.13 + t * 0.0048 + i as f32 * 1.3).sin() + 0.4 * (gx * 0.05 - t * 0.0029).sin());
        let edge = x - left < 1.0 || left + width - x <= 1.0;
        let hem = g.bottom as f32 + ripple.round() - lift - if edge { 1.0 } else { 0.0 };
        if py as f32 > hem {
            continue;
        }
        if py < g.top + g.sleeve {
            // Sleeve wrapped over the rod.
            return Px::C(Color::LightGreen);
        }
        let hem_band = py as f32 > hem - 2.0 * g.s.max(0.6);
        // Vertical folds, defined in cloth space so they crowd together as
        // the panel bunches up.
        let phase = u / g.pw * 2.0 * PI * 2.0 + i as f32 * 1.7;
        let fold = phase.sin();
        // As the cloth bunches, the print turns into the folds: only the
        // faces still pointing at the viewer show it.
        let facing = phase.cos();
        let shows_print = facing >= -1.0 + (compress - 1.0) * 0.9;
        let ly = (py as f32 + 0.5 - g.wy) / g.s;
        let lx = (gx - g.wx) / g.s;
        if !hem_band && shows_print && letter(lx, ly) {
            return if white_letters { Px::C(Color::White) } else { Px::Bg };
        }
        let crest = 0.93 - 0.25 * ((compress - 1.0) / 3.0).min(1.0);
        if hem_band {
            return Px::C(Color::LightGreen);
        }
        if fold > crest {
            return Px::C(Color::LightGreen);
        }
        return Px::C(Color::Green);
    }
    rod
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

fn center(area: Rect, w: usize) -> u16 {
    area.x + (area.width.saturating_sub(w as u16)) / 2
}

/// The first setup question, sitting behind the curtain from frame one.
fn draw_question(buf: &mut Buffer, area: Rect, g: &Geo, m: Motion, t: f32) {
    let wide = area.width >= 70;
    let label_w = OPTIONS.iter().map(|o| o.0.chars().count()).max().unwrap_or(0) as u16;
    let block_w = if wide { 54 } else { 3 + label_w };
    let x = if wide {
        center(area, block_w as usize)
    } else {
        (g.bunch_right().ceil() as u16 + 2).max(area.x + 2)
    };
    let mut y = area.y + 3;
    if wide {
        buf.set_string(x, y, "nori cli", dim());
        buf.set_string(x + 9, y, VERSION, dim());
        buf.set_string(x + 9 + VERSION.len() as u16 + 3, y, "~/tidepool", dim());
        y += 2;
    }
    // Jiro types the title once the curtain is mostly open.
    let since = m.since_key(t).unwrap_or(-1.0);
    let t0 = if m.flick { 0.0 } else { 520.0 };
    if since >= t0 {
        let n = TITLE.chars().count();
        let dt = since - t0;
        let k = if m.flick { n } else { (((dt - 140.0) / 28.0).floor().max(0.0) as usize).min(n) };
        let typed: String = TITLE.chars().take(k).collect();
        buf.set_string(x, y, "〉", green());
        buf.set_string(x + 2, y, &typed, bold());
        if k < n || dt < 140.0 + 28.0 * n as f32 + 260.0 {
            if !m.flick {
                let mx = x + 2 + k as u16;
                buf.set_string(mx, y, "_", green());
                buf.set_string(mx + 2, y, "▐", green());
            }
        }
    }
    y += 1;
    if wide {
        buf.set_string(x + 2, y, SUBTITLE, dim());
        y += 1;
    }
    y += 1;
    let rows_left = (area.y + area.height).saturating_sub(y + 2) as usize;
    for (i, (label, desc)) in OPTIONS.iter().enumerate().take(rows_left) {
        let ry = y + i as u16;
        if i == 0 {
            buf.set_string(x, ry, "▏", green());
            buf.set_string(x + 1, ry, "›", green().add_modifier(Modifier::BOLD));
            buf.set_string(x + 3, ry, *label, bold());
            if wide {
                buf.set_string(x + 3 + label_w + 2 + 14, ry, "▕", green());
            }
        } else {
            buf.set_string(x + 3, ry, *label, Style::default());
        }
        if wide {
            buf.set_string(x + 3 + label_w + 2, ry, *desc, dim());
        }
    }
    y += OPTIONS.len().min(rows_left) as u16 + 1;
    if wide && y < area.y + area.height {
        buf.set_string(x + 3, y, HINT, dim());
    }
}

/// Below the closed hem: Jiro and the invitation to step in.
fn draw_doorstep(buf: &mut Buffer, area: Rect, g: &Geo) {
    let row = area.y + ((g.bottom + 1) / 2) as u16 + ((area.height as i32 - (g.bottom + 1) / 2) / 3).max(2) as u16;
    let msg = "press any key to step in";
    let w = 5 + 2 + msg.chars().count();
    let x = center(area, w);
    buf.set_string(x, row, "〉_ ▐", green());
    buf.set_string(x + 7, row, msg, dim());
    if row + 2 < area.y + area.height {
        let foot = format!("cli {VERSION}   ~/tidepool");
        buf.set_string(center(area, foot.chars().count()), row + 2, &foot, dim());
    }
}

fn render(m: Motion, t: f32, area: Rect, buf: &mut Buffer, white_letters: bool) {
    let g = Geo::new(area.width, area.height);
    // Behind the closed curtain there is only shade; the question appears
    // the moment the cloth starts to move.
    if m.since_key(t).is_some() {
        draw_question(buf, area, &g, m, t);
    } else {
        draw_doorstep(buf, area, &g);
    }
    for row in 0..area.height {
        for col in 0..area.width {
            let (px, py) = (col as i32, row as i32 * 2);
            let top = curtain_px(&g, m, t, px, py, white_letters);
            let bot = curtain_px(&g, m, t, px, py + 1, white_letters);
            if (top, bot) == (Px::Clear, Px::Clear) {
                continue;
            }
            let Some(cell) = buf.cell_mut((area.x + col, area.y + row)) else { continue };
            cell.reset();
            match (top, bot) {
                (Px::Clear, Px::Clear) => {}
                (Px::Bg, Px::Bg) | (Px::Bg, Px::Clear) | (Px::Clear, Px::Bg) => {
                    cell.set_symbol(" ").set_style(Style::default().bg(Color::Reset));
                }
                (Px::C(a), Px::C(b)) if a == b => {
                    cell.set_symbol(" ").set_style(Style::default().bg(a));
                }
                (Px::C(a), Px::C(b)) => {
                    cell.set_symbol("▀").set_style(Style::default().fg(a).bg(b));
                }
                (Px::C(a), Px::Bg | Px::Clear) => {
                    cell.set_symbol("▀").set_style(Style::default().fg(a).bg(Color::Reset));
                }
                (Px::Bg | Px::Clear, Px::C(b)) => {
                    cell.set_symbol("▄").set_style(Style::default().fg(b).bg(Color::Reset));
                }
            }
        }
    }
}

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
    let mut cast = String::from("out.cast");
    let mut key: Option<f32> = Some(800.0);
    let mut flick = false;
    let mut until = 3400u64;
    let mut white = false;
    let frame_ms = 40u64;
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
            "--key" => key = v().parse().ok(),
            "--flick" => flick = true,
            "--until" => until = v().parse().unwrap(),
            "--letters" => white = v() == "white",
            other => panic!("unknown flag {other}"),
        }
    }
    if flick {
        key = Some(0.0);
    }
    let m = Motion { key_ms: key, flick };
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
        term.draw(|fr| {
            let area = fr.area();
            render(m, t as f32, area, fr.buffer_mut(), white);
        })
        .expect("draw");
        let bytes = std::mem::take(&mut *sink.0.borrow_mut());
        if !bytes.is_empty() {
            writeln!(f, "[{:.3}, \"o\", {}]", t as f64 / 1000.0, json_str(&bytes)).unwrap();
        }
        t += frame_ms;
    }
    writeln!(f, "[{:.3}, \"o\", \"\"]", until as f64 / 1000.0 + 0.5).unwrap();
}
