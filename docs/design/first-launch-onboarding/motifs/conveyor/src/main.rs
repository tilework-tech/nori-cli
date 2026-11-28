//! Conveyor motif draft for the Nori CLI first launch.
//!
//! The lowercase `nori` letters ride in on a kaiten-style belt, the belt
//! decelerates and stops, its treads overshoot and settle, and the green i dot
//! drops on last. Every frame is a pure function of elapsed milliseconds.
//!
//!   motif-conveyor --cast FILE [--size WxH] [--until MS] [--key MS]
//!
//! `--key MS` simulates a keypress at MS: the motion snaps to the settled frame.
//! Wide terminals get the big mark on a three-row belt; narrow or short ones get
//! a compact mark on a one-row belt (3 rows in total).

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

// Cell size in px for JetBrains Mono 20 under agg. Shapes are defined in px
// and sampled at quadrant (half cell by half cell) centres.
const SUB_W: f32 = 6.15;
const SUB_H: f32 = 14.6;

// Timeline (ms).
const CRUISE_MS: f32 = 700.0;
const STOP_MS: f32 = 2000.0;
const SETTLE_MS: f32 = 320.0;
const DROP_START_MS: f32 = 2080.0;
const DROP_MS: f32 = 240.0;
const BOUNCE_MS: f32 = 130.0;
pub const END_MS: u64 = 2480;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ink {
    Empty,
    Letter,
    Dot,
    Plate,
    Steel,
    Frame,
}

impl Ink {
    fn style(self) -> Style {
        match self {
            Ink::Empty => Style::default(),
            Ink::Letter => Style::default().add_modifier(Modifier::BOLD),
            Ink::Dot => Style::default().fg(Color::Green),
            Ink::Plate => Style::default().add_modifier(Modifier::DIM),
            Ink::Steel => Style::default().fg(Color::DarkGray),
            Ink::Frame => Style::default().fg(Color::Black),
        }
    }
    /// The colour this ink paints when used as a cell background.
    fn bg(self) -> Option<Color> {
        match self {
            Ink::Letter => Some(Color::Gray),
            Ink::Dot => Some(Color::Green),
            Ink::Steel => Some(Color::DarkGray),
            Ink::Frame => Some(Color::Black),
            Ink::Plate | Ink::Empty => None,
        }
    }
    fn rank(self) -> i32 {
        match self {
            Ink::Dot => 5,
            Ink::Letter => 4,
            Ink::Plate => 3,
            Ink::Steel => 2,
            Ink::Frame => 1,
            Ink::Empty => 0,
        }
    }
}

const QUAD: [&str; 16] = [
    " ", "▘", "▝", "▀", "▖", "▌", "▞", "▛", "▗", "▚", "▐", "▜", "▄", "▙", "▟", "█",
];

/// Paint one cell from its four quadrant inks (TL, TR, BL, BR).
fn paint(buf: &mut Buffer, x: u16, y: u16, q: [Ink; 4]) {
    let mut kinds: Vec<Ink> = q.iter().copied().filter(|k| *k != Ink::Empty).collect();
    kinds.sort_by_key(|k| -k.rank());
    kinds.dedup();
    if kinds.is_empty() {
        return;
    }
    let has_empty = q.contains(&Ink::Empty);
    let mask = |ink: Ink| (0..4).filter(|i| q[*i] == ink).fold(0, |m, i| m | (1 << i));
    let fg = kinds[0];
    let mut style = fg.style();
    let mut bits = mask(fg);
    if !has_empty && kinds.len() >= 2 {
        if let Some(bg) = kinds[1].bg() {
            style = style.bg(bg);
            // A third ink, if any, is absorbed into the background.
        } else {
            bits = 15;
        }
    }
    if let Some(c) = buf.cell_mut((x, y)) {
        c.set_symbol(QUAD[bits]).set_style(style);
    }
}

fn put(buf: &mut Buffer, x: u16, y: u16, s: &str, style: Style) {
    if buf.area.contains((x, y).into()) {
        if let Some(c) = buf.cell_mut((x, y)) {
            c.set_symbol(s).set_style(style);
        }
    }
}

/// Inside a rectangle with per-corner circular radii (tl, tr, bl, br), in px.
#[allow(clippy::too_many_arguments)]
fn in_rrect(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: [f32; 4]) -> bool {
    if x < x0 || x > x1 || y < y0 || y > y1 {
        return false;
    }
    let corners = [(x0 + r[0], y0 + r[0], r[0]), (x1 - r[1], y0 + r[1], r[1]), (x0 + r[2], y1 - r[2], r[2]), (x1 - r[3], y1 - r[3], r[3])];
    for (i, (cx, cy, rr)) in corners.iter().enumerate() {
        let in_zone = match i {
            0 => x < *cx && y < *cy,
            1 => x > *cx && y < *cy,
            2 => x < *cx && y > *cy,
            _ => x > *cx && y > *cy,
        };
        if in_zone && (x - cx).powi(2) + (y - cy).powi(2) > rr * rr {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------- big mark

const XH: f32 = 6.0 * 2.0 * SUB_H; // x-height: six rows
const STEM: f32 = 6.0 * SUB_W; // vertical stroke: three columns
const BAR: f32 = 3.0 * SUB_H; // horizontal stroke: three half rows

/// (letter, x offset in quadrant columns, width in quadrant columns)
const LETTERS: [(char, i32, i32); 4] = [('n', 0, 24), ('o', 29, 26), ('r', 60, 18), ('i', 83, 6)];
const WORD_SUBW: i32 = 89;

fn letter_hit(ch: char, w: i32, x: f32, y: f32) -> bool {
    let wp = w as f32 * SUB_W;
    match ch {
        'n' => {
            in_rrect(x, y, 0.0, 0.0, wp, XH, [64.0, 64.0, 0.0, 0.0])
                && !in_rrect(x, y, STEM, BAR, wp - STEM, XH + 40.0, [26.0, 26.0, 0.0, 0.0])
        }
        'o' => {
            in_rrect(x, y, 0.0, 0.0, wp, XH, [70.0; 4])
                && !in_rrect(x, y, STEM, BAR, wp - STEM, XH - BAR, [30.0; 4])
        }
        'r' => {
            in_rrect(x, y, 0.0, 0.0, wp, XH, [64.0, 22.0, 0.0, 0.0])
                && !in_rrect(x, y, STEM, BAR, wp + 40.0, XH + 40.0, [26.0, 0.0, 0.0, 0.0])
        }
        _ => in_rrect(x, y, 0.0, 0.0, wp, XH, [6.0, 6.0, 0.0, 0.0]),
    }
}

/// Belt travel in quadrant columns at time t, for a total travel of `d`.
fn travel(t: f32, d: f32) -> f32 {
    let v = d / (CRUISE_MS + (STOP_MS - CRUISE_MS) / 2.0);
    if t <= CRUISE_MS {
        v * t
    } else if t <= STOP_MS {
        let u = (t - CRUISE_MS) / (STOP_MS - CRUISE_MS);
        v * CRUISE_MS + v * (STOP_MS - CRUISE_MS) * (u - u * u / 2.0)
    } else {
        d
    }
}

/// The treads run past the stop by a hair and spring back.
fn tread_travel(t: f32, d: f32) -> f32 {
    let base = travel(t, d);
    if t > STOP_MS && t < STOP_MS + SETTLE_MS {
        let x = (t - STOP_MS) / SETTLE_MS;
        base + 3.2 * (PI * x).sin() * (1.0 - x)
    } else {
        base
    }
}

/// Vertical offset of the i dot in quadrant rows, or None while hidden.
fn dot_offset(t: f32) -> Option<i32> {
    if t < DROP_START_MS {
        return None;
    }
    let t = t - DROP_START_MS;
    if t < DROP_MS {
        let x = t / DROP_MS;
        return Some((-10.0 * (1.0 - x * x)).round() as i32);
    }
    let b = t - DROP_MS;
    if b < BOUNCE_MS {
        let x = b / BOUNCE_MS;
        return Some(-((1.4 * (PI * x).sin()).round() as i32));
    }
    Some(0)
}

struct Belt {
    x0: i32, // first column
    x1: i32, // last column (inclusive)
    y: i32,  // top row
}

/// Ink of the three-row belt at quadrant (sx, sy), with treads at `s`.
fn belt_ink(b: &Belt, sx: i32, sy: i32, s: i32) -> Ink {
    let k = sy - b.y * 2; // 0..6
    if !(0..6).contains(&k) {
        return Ink::Empty;
    }
    let r = 3.0 * SUB_H; // roller radius, px
    let px = (sx as f32 + 0.5) * SUB_W;
    let py = (k as f32 + 0.5) * SUB_H - r;
    let cl = b.x0 as f32 * 2.0 * SUB_W + r;
    let cr = (b.x1 + 1) as f32 * 2.0 * SUB_W - r;
    let pitch = 10; // quadrant columns per plate
    let seam = |pos: i32| pos.rem_euclid(pitch) == 0;
    let disc = [cl, cr].iter().any(|cx| (px - cx).powi(2) + py * py <= r * r);
    let straight = px >= cl && px <= cr;
    if disc && !(straight && (k == 0 || k == 5)) {
        return Ink::Steel;
    }
    if straight {
        return match k {
            0 => {
                if seam(sx + s) {
                    Ink::Steel
                } else {
                    Ink::Plate
                }
            }
            1 => Ink::Steel,
            5 => {
                if seam(sx - s) {
                    Ink::Frame
                } else {
                    Ink::Steel
                }
            }
            _ => Ink::Frame,
        };
    }
    Ink::Empty
}

struct Layout {
    word_x: i32, // settled column of the word
    mark_y: i32, // top row of the x-height
    belt: Belt,
    menu_x: u16,
    menu_y: u16,
    compact: bool,
}

fn layout(area: Rect) -> Layout {
    let compact = area.width < 64 || area.height < 26;
    if compact {
        let word_x = 3;
        return Layout {
            word_x,
            mark_y: 1,
            belt: Belt { x0: 1, x1: area.width as i32 - 2, y: 3 },
            menu_x: 2,
            menu_y: 5,
            compact,
        };
    }
    let word_w = WORD_SUBW / 2;
    let word_x = (area.width as i32 - word_w) / 2 - 4;
    Layout {
        word_x,
        mark_y: 4,
        belt: Belt { x0: 2, x1: area.width as i32 - 3, y: 10 },
        menu_x: word_x as u16,
        menu_y: 15,
        compact,
    }
}

fn draw_big(buf: &mut Buffer, area: Rect, l: &Layout, t: f32) {
    let d = ((area.width as i32 - l.word_x) * 2 + 4) as f32;
    let s_letters = travel(t, d).round() as i32;
    let s_treads = tread_travel(t, d).round() as i32;
    let lag = d as i32 - s_letters; // quadrant columns still to travel
    let origin = l.word_x * 2 + lag;
    let dot = dot_offset(t);
    let sy0 = l.mark_y * 2;
    for cy in 0..area.height as i32 {
        for cx in 0..area.width as i32 {
            let mut q = [Ink::Empty; 4];
            for (i, slot) in q.iter_mut().enumerate() {
                let sx = cx * 2 + (i as i32 & 1);
                let sy = cy * 2 + (i as i32 >> 1);
                let mut ink = belt_ink(&l.belt, sx, sy, s_treads);
                let ly = (sy - sy0) as f32 * SUB_H + SUB_H / 2.0;
                for (ch, off, w) in LETTERS {
                    let lx = (sx - origin - off) as f32 * SUB_W + SUB_W / 2.0;
                    if lx >= 0.0 && lx < w as f32 * SUB_W && letter_hit(ch, w, lx, ly) {
                        ink = Ink::Letter;
                    }
                    if ch == 'i' {
                        if let Some(o) = dot {
                            let rel = sy - sy0 - o; // dot rows -5..-3
                            let col = sx - origin - off;
                            if (-5..=-3).contains(&rel) && (0..w).contains(&col) {
                                ink = Ink::Dot;
                            }
                        }
                    }
                }
                *slot = ink;
            }
            paint(buf, area.x + cx as u16, area.y + cy as u16, q);
        }
    }
    // Axles turn with the treads; rail bolts stay put.
    let r_cols = (3.0 * SUB_H / (2.0 * SUB_W)).round() as i32;
    let ang = s_treads as f32 * SUB_W / (3.0 * SUB_H);
    let axle = ["◐", "◒", "◑", "◓"][((ang / (PI / 2.0)).floor() as i32).rem_euclid(4) as usize];
    let ay = (l.belt.y + 1) as u16;
    let axle_style = Style::default().fg(Color::Reset).bg(Color::DarkGray);
    put(buf, area.x + (l.belt.x0 + r_cols - 1) as u16, area.y + ay, axle, axle_style);
    put(buf, area.x + (l.belt.x1 - r_cols + 1) as u16, area.y + ay, axle, axle_style);
    let bolt = Style::default().fg(Color::DarkGray).bg(Color::Black);
    let mut bx = l.belt.x0 + r_cols + 6;
    while bx < l.belt.x1 - r_cols - 4 {
        put(buf, area.x + bx as u16, area.y + ay, "·", bolt);
        bx += 12;
    }
    // Version rides on the belt behind the i, like a plate label.
    let vx = origin / 2 + WORD_SUBW / 2 + 3;
    let vy = (l.mark_y + 5) as u16;
    for (i, ch) in VERSION.chars().enumerate() {
        let x = vx + i as i32;
        if (0..area.width as i32).contains(&x) {
            put(buf, area.x + x as u16, area.y + vy, &ch.to_string(), Style::default().fg(Color::DarkGray));
        }
    }
}

// ------------------------------------------------------------ compact mark

/// Two rows of half blocks; the i's top half is its green dot.
const COMPACT: [&str; 2] = ["█▀▀▄ ▄▀▀▄ █▀▀ ▀", "█  █ ▀▄▄▀ █   █"];
const COMPACT_W: i32 = 15;

fn draw_compact(buf: &mut Buffer, area: Rect, l: &Layout, t: f32) {
    let d = ((area.width as i32 - l.word_x) * 2 + 2) as f32;
    let s_letters = travel(t, d).round() as i32;
    let s_treads = tread_travel(t, d).round() as i32;
    let lag = (d as i32 - s_letters) / 2;
    let ox = l.word_x + lag;
    let dot_on = dot_offset(t).is_some_and(|o| o >= -1);
    for (row, line) in COMPACT.iter().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            let x = ox + col as i32;
            if ch == ' ' || !(0..area.width as i32).contains(&x) {
                continue;
            }
            let y = area.y + l.mark_y as u16 + row as u16;
            let is_dot = col as i32 == COMPACT_W - 1 && row == 0;
            let style = if is_dot {
                if dot_on {
                    Style::default().fg(Color::Green).bg(Color::Gray)
                } else {
                    Style::default().fg(Color::Reset)
                }
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            };
            let sym = if is_dot && !dot_on { "▄" } else { &ch.to_string() as &str };
            put(buf, area.x + x as u16, y, sym, style);
        }
    }
    // One-row belt: plates on the top half, return run on the bottom half.
    let y = area.y + l.belt.y as u16;
    for x in l.belt.x0..=l.belt.x1 {
        let mut q = [Ink::Empty; 4];
        for (i, slot) in q.iter_mut().enumerate() {
            let sx = x * 2 + (i as i32 & 1);
            let top = i < 2;
            let ends = (x == l.belt.x0 && i & 1 == 0) || (x == l.belt.x1 && i & 1 == 1);
            *slot = if ends {
                Ink::Empty
            } else if top {
                if (sx + s_treads).rem_euclid(8) == 0 { Ink::Steel } else { Ink::Plate }
            } else if (sx - s_treads).rem_euclid(8) == 0 {
                Ink::Frame
            } else {
                Ink::Steel
            };
        }
        if x == l.belt.x0 || x == l.belt.x1 {
            let sym = if x == l.belt.x0 { "▐" } else { "▌" };
            put(buf, area.x + x as u16, y, sym, Style::default().fg(Color::DarkGray));
            continue;
        }
        paint(buf, area.x + x as u16, y, q);
    }
    let vx = l.word_x + COMPACT_W + 2;
    if t >= STOP_MS {
        buf.set_string(area.x + vx as u16, area.y + l.mark_y as u16 + 1, VERSION, Style::default().fg(Color::DarkGray));
    }
}

// -------------------------------------------------------------------- menu

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

fn draw_menu(buf: &mut Buffer, area: Rect, l: &Layout) {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let green = Style::default().fg(Color::Green);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let x = area.x + l.menu_x;
    let mut y = area.y + l.menu_y;
    buf.set_string(x, y, "〉", green);
    buf.set_string(x + 2, y, "Choose your first agent", bold);
    y += 1;
    if !l.compact {
        buf.set_string(x + 2, y, "Nori runs it for you. Switch any time with /agent.", dim);
        y += 1;
    }
    y += 1;
    let rows = (area.y + area.height).saturating_sub(y + 2) as usize;
    let show_desc = area.width >= l.menu_x + 32;
    for (i, (label, desc)) in OPTIONS.iter().enumerate().take(rows) {
        if i == 0 {
            put(buf, x, y, "▏", green);
            buf.set_string(x + 1, y, "›", green.add_modifier(Modifier::BOLD));
            buf.set_string(x + 3, y, *label, bold);
        } else {
            buf.set_string(x + 3, y, *label, Style::default());
        }
        if show_desc {
            buf.set_string(x + 18, y, *desc, dim);
        }
        y += 1;
    }
    y += 1;
    if y < area.y + area.height {
        buf.set_string(x + 3, y, "↑↓ move  enter choose  q quit", dim);
    }
}

fn render(t_ms: u64, key_ms: Option<u64>, area: Rect, buf: &mut Buffer) {
    let t = match key_ms {
        Some(k) if t_ms >= k => END_MS as f32,
        _ => t_ms as f32,
    };
    let l = layout(area);
    if l.compact {
        draw_compact(buf, area, &l, t);
    } else {
        draw_big(buf, area, &l, t);
    }
    draw_menu(buf, area, &l);
}

// ------------------------------------------------------------------- casts

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

/// Deterministic asciicast: the exact bytes ratatui emits on a fixed clock.
fn write_cast(path: &str, w: u16, h: u16, until: u64, key: Option<u64>) {
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
        term.draw(|fr| render(t, key, fr.area(), fr.buffer_mut())).expect("draw");
        let bytes = std::mem::take(&mut *sink.0.borrow_mut());
        if !bytes.is_empty() {
            writeln!(f, "[{:.3}, \"o\", {}]", t as f64 / 1000.0, json_str(&bytes)).unwrap();
        }
        t += 20;
    }
    writeln!(f, "[{:.3}, \"o\", \"\"]", until as f64 / 1000.0 + 0.5).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut path = None;
    let (mut w, mut h) = (100, 30);
    let mut until = END_MS + 900;
    let mut key = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--cast" => path = Some(args[i + 1].clone()),
            "--size" => {
                let (a, b) = args[i + 1].split_once('x').expect("WxH");
                w = a.parse().expect("width");
                h = b.parse().expect("height");
            }
            "--until" => until = args[i + 1].parse().expect("ms"),
            "--key" => key = Some(args[i + 1].parse().expect("ms")),
            other => panic!("unknown flag {other}"),
        }
        i += 2;
    }
    write_cast(&path.expect("--cast FILE"), w, h, until, key);
}
