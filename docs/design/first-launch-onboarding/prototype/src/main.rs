//! First-launch onboarding design study.
//!
//! Renders a three-row half-block `nori` wordmark, an optional bounded
//! dot/Braille reveal around it, and the setup questions with Jiro's face
//! typing selected titles. All content is synthetic.
//!
//! Modes:
//!   (default)            interactive, in the current terminal
//!   --cast FILE          write a deterministic asciicast of a scripted run
//!   --bench              headless frame-cost measurement
//!
//! Options:
//!   --mark static|reveal       wordmark treatment (default reveal)
//!   --flow one|sparse          Jiro types only the first question, or a
//!                              short title on every step (default one)
//!   --size WxH                 cast size (default 80x24)
//!   --keys "MS:KEY,..."        scripted keys for --cast (up, down, enter, 1-9)
//!   --until MS                 end the cast at MS
//!   --frame-ms N               animation frame interval (default 40)
//!   --trace FILE               interactive: log frame and key latencies

use std::cell::RefCell;
use std::fs::File;
use std::rc::Rc;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::crossterm::{cursor, execute, terminal};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::{Terminal, TerminalOptions, Viewport};

const VERSION: &str = "v0.42.0";

/// Three rows of half blocks: six pixel rows. The i dot sits alone on the
/// top pixel row so it can carry the brand green.
const MARK: [&str; 3] = [
    "              ▀",
    "█▀▀▄ ▄▀▀▄ █▀▀ █",
    "█  █ ▀▄▄▀ █   █",
];
const MARK_W: u16 = 15;
const DOT_COL: u16 = 14;

const REVEAL_MS: u64 = 720;
const FACE_OPEN_MS: u64 = 200;
const BLINK_FRAME_MS: u64 = 80;
const LETTER_MS: u64 = 30;
const HOLD_MS: u64 = 240;

const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
const BRAILLE_BITS: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
const DOTS: [&str; 5] = [" ", "·", "∘", "○", "●"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkMode {
    Static,
    Reveal,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
    One,
    Sparse,
}

struct StepDef {
    typed_title: &'static str,
    static_title: &'static str,
    subtitle: &'static str,
    options: &'static [(&'static str, &'static str)],
    hint: &'static str,
}

const STEPS: [StepDef; 3] = [
    StepDef {
        typed_title: "Choose your first agent",
        static_title: "Choose your first agent",
        subtitle: "Nori runs it for you. Switch any time with /agent.",
        options: &[
            ("Claude Code", "Anthropic"),
            ("Codex", "OpenAI"),
            ("Antigravity", "Google"),
            ("Grok Build", "xAI"),
            ("OpenCode", "any provider"),
            ("Cursor Agent", "Cursor"),
            ("Hermes Agent", "Nous Research"),
            ("Pi", "any provider"),
        ],
        hint: "↑↓ move  enter choose  q quit",
    },
    StepDef {
        typed_title: "Work in ~/tidepool?",
        static_title: "Work in ~/tidepool?",
        subtitle: "This folder is a git repository.",
        options: &[
            ("Yes, without asking", "edits and commands run freely"),
            ("No, ask first", "approve each edit and command"),
        ],
        hint: "↑↓ move  enter choose  q quit",
    },
    StepDef {
        typed_title: "Compact tool output?",
        static_title: "Compact tool output?",
        subtitle: "Ctrl+O always opens the full transcript.",
        options: &[
            ("Full output", "every command with all of its output"),
            ("Compact", "one line per tool call"),
        ],
        hint: "↑↓ move  enter choose  esc skip",
    },
];

struct App {
    mark: MarkMode,
    flow: Flow,
    step: usize,
    sel: usize,
    step_started_ms: u64,
    done: bool,
}

impl App {
    fn new(mark: MarkMode, flow: Flow) -> Self {
        Self { mark, flow, step: 0, sel: 0, step_started_ms: 0, done: false }
    }

    fn key(&mut self, code: KeyCode, now_ms: u64) {
        if self.done {
            return;
        }
        let n = STEPS[self.step].options.len();
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.sel = (self.sel + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') => self.sel = (self.sel + 1) % n,
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < n {
                    self.sel = i;
                    self.advance(now_ms);
                }
            }
            KeyCode::Enter => self.advance(now_ms),
            KeyCode::Esc if self.step == 2 => self.advance(now_ms),
            _ => {}
        }
    }

    fn advance(&mut self, now_ms: u64) {
        if self.step + 1 < STEPS.len() {
            self.step += 1;
            self.sel = 0;
            self.step_started_ms = now_ms;
        } else {
            self.done = true;
        }
    }

    /// When the typed title for the current step starts, relative to app start.
    fn typing_start(&self) -> Option<u64> {
        match (self.step, self.flow) {
            (0, _) => Some(match self.mark {
                MarkMode::Reveal => REVEAL_MS * 2 / 3,
                MarkMode::Static => 0,
            }),
            (_, Flow::Sparse) => Some(self.step_started_ms),
            (_, Flow::One) => None,
        }
    }

    /// Time of the next visual change after `now_ms`, if any.
    fn next_change(&self, now_ms: u64, frame_ms: u64) -> Option<u64> {
        let mut end = 0;
        if self.mark == MarkMode::Reveal {
            end = end.max(REVEAL_MS);
        }
        if let Some(t0) = self.typing_start() {
            end = end.max(t0 + face_total_ms(self.step, STEPS[self.step].typed_title));
        }
        (now_ms < end).then(|| (now_ms / frame_ms + 1) * frame_ms)
    }
}

fn blink_ms(step: usize) -> u64 {
    if step == 0 { 0 } else { BLINK_FRAME_MS * 4 }
}

fn face_total_ms(step: usize, title: &str) -> u64 {
    FACE_OPEN_MS + blink_ms(step) + LETTER_MS * title.chars().count() as u64 + HOLD_MS
}

fn green() -> Style {
    Style::default().fg(Color::Green)
}
fn dim() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

fn hash(x: u16, y: u16) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![];
    let mut cur = String::new();
    for word in text.split(' ') {
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    lines.push(cur);
    lines
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn put(buf: &mut Buffer, x: u16, y: u16, s: &str, style: Style) {
    if buf.area.contains((x, y).into()) {
        if let Some(c) = buf.cell_mut((x, y)) {
            c.set_symbol(s).set_style(style);
        }
    }
}

/// Which header to draw at this size.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Header {
    Big { version_beside: bool },
    Line,
}

fn header_for(area: Rect) -> Header {
    if area.width >= 2 + MARK_W + 2 + 11 + 1 && area.height >= 18 {
        Header::Big { version_beside: true }
    } else if area.width >= 2 + MARK_W + 1 && area.height >= 18 {
        Header::Big { version_beside: false }
    } else {
        Header::Line
    }
}

fn mark_cell(col: u16, row: u16) -> Option<&'static str> {
    let ch = MARK[row as usize].chars().nth(col as usize)?;
    match ch {
        '▀' => Some("▀"),
        '▄' => Some("▄"),
        '█' => Some("█"),
        _ => None,
    }
}

/// Draw the big wordmark at (x0, y0). `t` is ms since start; reveal runs for
/// REVEAL_MS inside a bounded halo around the mark.
fn draw_mark(buf: &mut Buffer, x0: u16, y0: u16, mode: MarkMode, t: u64, version_beside: bool) {
    let p = if mode == MarkMode::Static { 1.0 } else { (t as f32 / REVEAL_MS as f32).min(1.0) };
    let halo = Rect::new(x0.saturating_sub(2), y0, MARK_W + 2 + 14, 3);
    let (ox, oy) = (x0 + DOT_COL, y0);
    let dmax = 15.0f32;
    let front = smooth(p) * 1.3;

    for y in halo.y..halo.y + halo.height {
        for x in halo.x..halo.x + halo.width {
            let in_mark = x >= x0 && x < x0 + MARK_W;
            let glyph = if in_mark { mark_cell(x - x0, y - y0) } else { None };
            let is_dot = in_mark && x - x0 == DOT_COL && y == y0;
            if is_dot {
                put(buf, x, y, "▀", green());
                continue;
            }
            if p >= 1.0 {
                if let Some(g) = glyph {
                    put(buf, x, y, g, Style::default());
                }
                continue;
            }
            let dx = (x as f32 - ox as f32) * 0.5;
            let dy = y as f32 - oy as f32;
            let d = (dx * dx + dy * dy).sqrt() / dmax;
            let behind = front - d;
            if behind <= 0.0 {
                continue;
            }
            let bayer = BAYER[(y % 4) as usize][(x % 4) as usize] as f32 / 16.0;
            match glyph {
                Some(g) => {
                    let lock = 0.22 + 0.12 * hash(x, y);
                    if behind >= lock {
                        put(buf, x, y, g, Style::default());
                    } else {
                        let k = behind / lock;
                        let (top, bottom) = match g {
                            "▀" => (true, false),
                            "▄" => (false, true),
                            _ => (true, true),
                        };
                        let mut bits = 0u32;
                        for (r, row) in BRAILLE_BITS.iter().enumerate() {
                            let on_row = if r < 2 { top } else { bottom };
                            for (c, bit) in row.iter().enumerate() {
                                let th = BAYER[(r + (y as usize % 2) * 2) % 4][(c + x as usize * 2) % 4] as f32 / 16.0;
                                if on_row && th < k {
                                    bits |= bit;
                                }
                            }
                        }
                        if bits != 0 {
                            let s = char::from_u32(0x2800 + bits).unwrap().to_string();
                            put(buf, x, y, &s, green());
                        }
                    }
                }
                None => {
                    let a = 1.0 - behind / 0.40;
                    if a <= 0.0 {
                        continue;
                    }
                    // Sparse Braille dust just behind the front, thinning to
                    // middle dots, then nothing.
                    let halo = Style::default().fg(Color::Green).add_modifier(Modifier::DIM);
                    let density = a * a * 0.55;
                    let mut bits = 0u32;
                    for (r, row) in BRAILLE_BITS.iter().enumerate() {
                        for (c, bit) in row.iter().enumerate() {
                            let th = hash(x * 2 + c as u16, y * 4 + r as u16);
                            if th < density {
                                bits |= bit;
                            }
                        }
                    }
                    if bits != 0 {
                        let s = char::from_u32(0x2800 + bits).unwrap().to_string();
                        put(buf, x, y, &s, halo);
                    } else if bayer < a * 0.5 {
                        put(buf, x, y, DOTS[1], halo);
                    }
                }
            }
        }
    }
    if p >= 1.0 || p > 0.75 {
        if version_beside {
            buf.set_string(x0 + MARK_W + 2, y0 + 2, "cli", Style::default());
            buf.set_string(x0 + MARK_W + 6, y0 + 2, VERSION, dim());
        } else {
            buf.set_string(x0, y0 + 3, format!("cli {VERSION}"), dim());
        }
    }
}

/// The question title line, with Jiro typing it when `t0` is set.
fn draw_title(buf: &mut Buffer, x: u16, y: u16, title: &str, step: usize, t0: Option<u64>, t: u64) {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let Some(t0) = t0 else {
        buf.set_string(x, y, "〉", green());
        buf.set_string(x + 2, y, title, bold);
        return;
    };
    if t < t0 {
        return;
    }
    let dt = t - t0;
    let n = title.chars().count() as u64;
    let blink = blink_ms(step);
    let face = |buf: &mut Buffer, left: &str, eye: &str, typed: &str| {
        // Every left eye is two cells wide, so the face is always five cells
        // plus whatever has been typed between the eye and the mouth.
        buf.set_string(x, y, left, green());
        buf.set_string(x + 2, y, typed, bold);
        let mx = x + 2 + typed.chars().count() as u16;
        buf.set_string(mx, y, "_", green());
        buf.set_string(mx + 2, y, eye, green());
    };
    if dt < FACE_OPEN_MS {
        face(buf, "〉", "▐", "");
    } else if dt < FACE_OPEN_MS + blink {
        let f = (dt - FACE_OPEN_MS) / BLINK_FRAME_MS;
        let (l, r) = match f {
            0 | 2 => ("› ", "▗"),
            1 => ("- ", "▁"),
            _ => ("〉", "▐"),
        };
        face(buf, l, r, "");
    } else if dt < FACE_OPEN_MS + blink + LETTER_MS * n + HOLD_MS {
        let k = ((dt - FACE_OPEN_MS - blink) / LETTER_MS + 1).min(n) as usize;
        let typed: String = title.chars().take(k).collect();
        face(buf, "〉", "▐", &typed);
    } else {
        buf.set_string(x, y, "〉", green());
        buf.set_string(x + 2, y, title, bold);
    }
}

fn draw_menu(buf: &mut Buffer, x: u16, y: u16, width: u16, def: &StepDef, sel: usize, max_rows: u16) {
    let label_w = def.options.iter().map(|o| o.0.chars().count()).max().unwrap_or(0) as u16;
    for (i, (label, desc)) in def.options.iter().enumerate().take(max_rows as usize) {
        let ry = y + i as u16;
        let focused = i == sel;
        if focused {
            put(buf, x, ry, "▏", green());
            buf.set_string(x + 1, ry, "›", green().add_modifier(Modifier::BOLD));
            buf.set_string(x + 3, ry, *label, Style::default().add_modifier(Modifier::BOLD));
            put(buf, x + width.saturating_sub(1), ry, "▕", green());
        } else {
            buf.set_string(x + 3, ry, *label, Style::default());
        }
        let dx = x + 3 + label_w + 2;
        // Descriptions are dropped as a column rather than truncated.
        if dx + desc.chars().count() as u16 + 2 <= x + width {
            buf.set_string(dx, ry, *desc, dim());
        }
    }
}

fn render(app: &App, t: u64, area: Rect, buf: &mut Buffer) {
    let mx = 2u16;
    let mut y = area.y + 1;
    match header_for(area) {
        Header::Big { version_beside } => {
            draw_mark(buf, area.x + mx, y, app.mark, t, version_beside);
            y += if version_beside { 4 } else { 5 };
        }
        Header::Line => {
            buf.set_string(area.x + mx, y, "〉", green());
            buf.set_string(area.x + mx + 2, y, "nori CLI", Style::default().add_modifier(Modifier::BOLD));
            buf.set_string(area.x + mx + 11, y, VERSION, dim());
            y += 2;
        }
    }
    if app.done {
        buf.set_string(area.x + mx, y, "Setup saved. Opening the chat…", dim());
        return;
    }
    let def = &STEPS[app.step];
    let title = if app.typing_start().is_some() { def.typed_title } else { def.static_title };
    draw_title(buf, area.x + mx, y, title, app.step, app.typing_start(), t);
    y += 1;
    let avail = area.width.saturating_sub(mx * 2);
    let label_w = def.options.iter().map(|o| o.0.chars().count()).max().unwrap_or(0) as u16;
    let desc_w = def.options.iter().map(|o| o.1.chars().count()).max().unwrap_or(0) as u16;
    let full = 3 + label_w + 2 + desc_w + 2;
    let width = if full <= avail { full } else { (3 + label_w + 2).min(avail) };
    for line in wrap(def.subtitle, avail.saturating_sub(2) as usize) {
        buf.set_string(area.x + mx + 2, y, line, dim());
        y += 1;
    }
    y += 1;
    let rows_left = (area.y + area.height).saturating_sub(y + 2);
    draw_menu(buf, area.x + mx, y, width, def, app.sel, rows_left);
    y += (def.options.len() as u16).min(rows_left) + 1;
    if y < area.y + area.height {
        buf.set_string(area.x + mx + 3, y, def.hint, dim());
    }
}

struct Opts {
    mark: MarkMode,
    flow: Flow,
    size: (u16, u16),
    cast: Option<String>,
    bench: bool,
    keys: Vec<(u64, KeyCode)>,
    until: u64,
    frame_ms: u64,
    trace: Option<String>,
    compare: Option<String>,
}

fn parse_key(s: &str) -> KeyCode {
    match s {
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "enter" => KeyCode::Enter,
        "esc" => KeyCode::Esc,
        c => KeyCode::Char(c.chars().next().expect("key")),
    }
}

fn parse_opts() -> Opts {
    let mut o = Opts {
        mark: MarkMode::Reveal,
        flow: Flow::One,
        size: (80, 24),
        cast: None,
        bench: false,
        keys: vec![],
        until: 6000,
        frame_ms: 40,
        trace: None,
        compare: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut v = || args.next().expect("missing value");
        match a.as_str() {
            "--mark" => o.mark = if v() == "static" { MarkMode::Static } else { MarkMode::Reveal },
            "--flow" => o.flow = if v() == "sparse" { Flow::Sparse } else { Flow::One },
            "--size" => {
                let s = v();
                let (w, h) = s.split_once('x').expect("WxH");
                o.size = (w.parse().unwrap(), h.parse().unwrap());
            }
            "--cast" => o.cast = Some(v()),
            "--bench" => o.bench = true,
            "--keys" => {
                o.keys = v()
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|kv| {
                        let (ms, k) = kv.split_once(':').expect("MS:KEY");
                        (ms.parse().unwrap(), parse_key(k))
                    })
                    .collect()
            }
            "--until" => o.until = v().parse().unwrap(),
            "--frame-ms" => o.frame_ms = v().parse().unwrap(),
            "--trace" => o.trace = Some(v()),
            "--compare" => o.compare = Some(v()),
            other => panic!("unknown flag {other}"),
        }
    }
    o
}

/// A writer whose bytes can be taken after each draw.
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

type MemTerm = (Terminal<CrosstermBackend<Sink>>, Sink);

fn mem_terminal(w: u16, h: u16) -> MemTerm {
    let sink = Sink::default();
    let term = Terminal::with_options(
        CrosstermBackend::new(sink.clone()),
        TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, w, h)) },
    )
    .expect("terminal");
    (term, sink)
}

fn draw_mem((term, sink): &mut MemTerm, app: &App, t: u64) -> Vec<u8> {
    term.draw(|f| {
        let area = f.area();
        render(app, t, area, f.buffer_mut());
    })
    .expect("draw");
    std::mem::take(&mut *sink.0.borrow_mut())
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

/// Deterministic asciicast of a scripted run: the exact bytes ratatui emits.
/// With `--compare marks|flows`, two variants run side by side on the same
/// clock and keys.
fn write_cast(o: &Opts, path: &str) {
    let (w, h) = o.size;
    let mut panes: Vec<(App, &str)> = match o.compare.as_deref() {
        Some("marks") => vec![
            (App::new(MarkMode::Static, o.flow), "A  static mark"),
            (App::new(MarkMode::Reveal, o.flow), "B  dot/Braille reveal"),
        ],
        Some("flows") => vec![
            (App::new(o.mark, Flow::One), "A  Jiro types one question"),
            (App::new(o.mark, Flow::Sparse), "B  Jiro on every step"),
        ],
        _ => vec![(App::new(o.mark, o.flow), "")],
    };
    let n = panes.len() as u16;
    let total_w = w * n + (n - 1);
    let total_h = if n > 1 { h + 1 } else { h };
    let mut term = mem_terminal(total_w, total_h);
    let mut f = File::create(path).expect("cast");
    writeln!(f, "{{\"version\": 2, \"width\": {total_w}, \"height\": {total_h}, \"env\": {{\"TERM\": \"xterm-256color\"}}}}").unwrap();
    let mut keys = o.keys.clone();
    keys.sort_by_key(|k| k.0);
    let mut ki = 0;
    let mut t = 0;
    writeln!(f, "[0.0, \"o\", {}]", json_str(b"\x1b[?25l")).unwrap();
    while t <= o.until {
        while ki < keys.len() && keys[ki].0 <= t {
            for (app, _) in panes.iter_mut() {
                app.key(keys[ki].1, keys[ki].0);
            }
            ki += 1;
        }
        let (term_ref, sink) = &mut term;
        term_ref
            .draw(|fr| {
                let buf = fr.buffer_mut();
                for (i, (app, label)) in panes.iter().enumerate() {
                    let x = i as u16 * (w + 1);
                    render(app, t, Rect::new(x, 0, w, h), buf);
                    if n > 1 {
                        buf.set_string(x + 2, h, *label, dim());
                        if i > 0 {
                            for y in 0..total_h {
                                put(buf, x - 1, y, "│", dim());
                            }
                        }
                    }
                }
            })
            .expect("draw");
        let bytes = std::mem::take(&mut *sink.0.borrow_mut());
        if !bytes.is_empty() {
            writeln!(f, "[{:.3}, \"o\", {}]", t as f64 / 1000.0, json_str(&bytes)).unwrap();
        }
        t += o.frame_ms.min(10);
    }
    writeln!(f, "[{:.3}, \"o\", \"\"]", o.until as f64 / 1000.0 + 0.5).unwrap();
}

fn pct(v: &mut [f64], q: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() as f64 - 1.0) * q).round() as usize]
}

/// Headless frame cost: widget render + ratatui diff + crossterm encoding,
/// with no terminal I/O.
fn bench(o: &Opts) {
    println!("size,phase,frames,p50_us,p95_us,max_us,mean_bytes");
    for &(w, h) in &[(40u16, 16u16), (60, 20), (80, 24), (120, 40), (200, 60)] {
        for (phase, mark, flow) in [
            ("reveal+type", MarkMode::Reveal, Flow::One),
            ("static+type", MarkMode::Static, Flow::One),
            ("settled", MarkMode::Static, Flow::One),
        ] {
            let mut samples = vec![];
            let mut bytes = 0usize;
            for _ in 0..200 {
                let mut term = mem_terminal(w, h);
                let app = App::new(mark, flow);
                let (from, to) = if phase == "settled" { (3000, 3400) } else { (0, 1700) };
                let mut first = true;
                let mut t = from;
                while t <= to {
                    let s = Instant::now();
                    let b = draw_mem(&mut term, &app, t);
                    let us = s.elapsed().as_secs_f64() * 1e6;
                    if !first {
                        samples.push(us);
                        bytes += b.len();
                    }
                    first = false;
                    t += o.frame_ms;
                }
            }
            let n = samples.len();
            let max = samples.iter().cloned().fold(0.0, f64::max);
            println!(
                "{w}x{h},{phase},{n},{:.1},{:.1},{:.1},{}",
                pct(&mut samples, 0.5),
                pct(&mut samples, 0.95),
                max,
                bytes / n
            );
        }
    }
}

fn interactive(o: &Opts) -> io::Result<()> {
    let mut trace = o.trace.as_ref().map(|p| File::create(p).expect("trace"));
    terminal::enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, terminal::Clear(terminal::ClearType::All), cursor::Hide)?;
    let mut term = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let start = Instant::now();
    let mut app = App::new(o.mark, o.flow);
    let now = |s: &Instant| s.elapsed().as_millis() as u64;
    let result = (|| -> io::Result<()> {
        loop {
            let t = now(&start);
            let s = Instant::now();
            term.draw(|f| {
                let area = f.area();
                render(&app, t, area, f.buffer_mut());
            })?;
            if let Some(tr) = trace.as_mut() {
                writeln!(tr, "frame t={t} draw_us={}", s.elapsed().as_micros())?;
            }
            let wait = app
                .next_change(t, o.frame_ms)
                .map(|n| Duration::from_millis(n.saturating_sub(now(&start))))
                .unwrap_or(Duration::from_secs(3600));
            if event::poll(wait)? {
                let read_at = Instant::now();
                match event::read()? {
                    Event::Key(k) if k.kind == KeyEventKind::Press => {
                        if k.code == KeyCode::Char('q')
                            || (k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL))
                        {
                            return Ok(());
                        }
                        app.key(k.code, now(&start));
                        let t = now(&start);
                        term.draw(|f| {
                            let area = f.area();
                            render(&app, t, area, f.buffer_mut());
                        })?;
                        if let Some(tr) = trace.as_mut() {
                            writeln!(tr, "key t={t} {:?} key_to_flush_us={}", k.code, read_at.elapsed().as_micros())?;
                        }
                        if app.done {
                            std::thread::sleep(Duration::from_millis(800));
                            return Ok(());
                        }
                    }
                    Event::Resize(..) => term.autoresize()?,
                    _ => {}
                }
            }
        }
    })();
    terminal::disable_raw_mode()?;
    execute!(io::stdout(), cursor::Show)?;
    println!();
    result
}

fn main() -> io::Result<()> {
    let o = parse_opts();
    if o.bench {
        bench(&o);
    } else if let Some(p) = o.cast.clone() {
        write_cast(&o, &p);
    } else {
        interactive(&o)?;
    }
    Ok(())
}
