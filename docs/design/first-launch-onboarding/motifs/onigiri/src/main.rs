//! Onigiri motif for the Nori CLI first launch, v2.
//!
//! The ball sizes itself to the terminal. With room for it (a ball at least
//! INTRO_ROWS tall) the full welcome plays: grains gather, get pressed, the
//! nori band wraps, then a light circles the rice indefinitely. Smaller
//! terminals get the settled ball, and the light keeps circling while the
//! ball is big enough to carry grain texture. Colour adapts to truecolor,
//! 256 and ANSI 16.
//!
//! Usage:
//!   motif-onigiri --cast FILE [--size WxH] [--until MS] [--static]
//!                 [--tier true|256|16] [--at MS]

mod ball;
mod color;
mod shape;
mod word;

use std::cell::RefCell;
use std::fs::File;
use std::io::{self, Write};
use std::rc::Rc;

use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::{Terminal, TerminalOptions, Viewport};

use ball::{draw_ball, Paint, Place};
use color::{color, Mat, Tier};

const VERSION: &str = "v0.42.0";
/// Smallest ball (rows) that plays the full intro with particles.
const INTRO_ROWS: f32 = 11.0;
/// Smallest ball (rows) that carries grain texture and the circling light.
const TEXTURE_ROWS: f32 = 7.0;
/// Smallest ball (rows) worth drawing at all; below it, a one-line header.
const MIN_ROWS: f32 = 4.0;
const MAX_ROWS: f32 = 34.0;
/// Wordmark x-height as a fraction of the ball height.
const WORD_RATIO: f32 = 1.0 / 13.6;

#[derive(Debug, Clone, Copy)]
enum Layout {
    /// Ball left, block wordmark right on the ball's baseline.
    Side { rows: f32 },
    /// Ball centred, text wordmark line under it.
    Stack { rows: f32 },
    Line,
}

fn layout(w: u16, h: u16) -> Layout {
    let (w, h) = (w as f32, h as f32);
    let ball_w = |r: f32| Place::width(Place::size_for(r));
    let word_w = |r: f32| word::WORD_W * r * WORD_RATIO;
    // Largest ball that fits each arrangement, by bisection on rows.
    let fit = |ok: &dyn Fn(f32) -> bool| {
        let (mut lo, mut hi) = (0.0f32, MAX_ROWS);
        for _ in 0..30 {
            let mid = (lo + hi) / 2.0;
            if ok(mid) { lo = mid } else { hi = mid }
        }
        lo.floor()
    };
    let side = fit(&|r| ball_w(r) + 0.2 * r + 3.0 + word_w(r) <= w - 10.0 && r <= 0.62 * h && r + 3.0 <= h - 2.0);
    let stack = fit(&|r| ball_w(r) <= w - 4.0 && r <= 0.66 * h && r + 4.0 <= h - 1.0);
    // Side by side reads best; stack only when it buys a much bigger ball.
    if side >= MIN_ROWS && (side >= 14.0 || side * 1.6 >= stack) {
        Layout::Side { rows: side }
    } else if stack >= MIN_ROWS {
        Layout::Stack { rows: stack }
    } else {
        Layout::Line
    }
}

fn render(t: u64, area: Rect, buf: &mut Buffer, settled: bool, tier: Tier) {
    let t = t as f32;
    let dim = Style::default().add_modifier(Modifier::DIM);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let mut p = Paint { buf, tier };
    match layout(area.width, area.height) {
        Layout::Side { rows } => {
            let intro = !settled && rows >= INTRO_ROWS;
            let tw = if intro { t } else { 1.0e6 };
            let s = Place::size_for(rows);
            let ball_w = Place::width(s);
            let scale = rows * WORD_RATIO;
            let total = ball_w + 0.2 * rows + 3.0 + word::WORD_W * scale;
            let ox = area.x as f32 + (area.width as f32 - total) / 2.0;
            let top = area.y as i32 + ((area.height as f32 - rows - 2.0) / 2.0).round() as i32;
            let place = Place::with_rows(ox + ball_w / 2.0, top, rows);
            draw_ball(&mut p, place, t, intro, !settled && rows >= TEXTURE_ROWS, rows >= TEXTURE_ROWS);
            let base = place.base().round() as i32;
            let left = (ox + ball_w + 0.2 * rows + 3.0).round() as i32;
            word::draw_word(&mut p, left, base - (9.0 * scale).round() as i32, tw, scale);
            if tw >= word::VERSION_AT {
                let y = base / 2 + 1;
                p.put(left, y, "cli", Style::default());
                p.buf.set_string((left + 4) as u16, y as u16, VERSION, dim);
                p.buf.set_string((left + 13) as u16, y as u16, "~/tidepool", dim);
            }
        }
        Layout::Stack { rows } => {
            let intro = !settled && rows >= INTRO_ROWS;
            let tw = if intro { t } else { 1.0e6 };
            let cx = area.x as f32 + area.width as f32 / 2.0;
            let top = area.y as i32 + ((area.height as f32 - rows - 3.0) / 2.0).round() as i32;
            let place = Place::with_rows(cx, top, rows);
            draw_ball(&mut p, place, t, intro, !settled && rows >= TEXTURE_ROWS, rows >= TEXTURE_ROWS);
            if tw >= word::WORD_AT {
                let y = (place.base() / 2.0).round() as u16 + 1;
                let x = (cx - 8.0).round() as u16;
                text_wordmark(&mut p, x, y, tw >= word::DOT_AT);
                p.buf.set_string(x + 5, y, "cli", Style::default());
                p.buf.set_string(x + 9, y, VERSION, dim);
            }
        }
        Layout::Line => {
            let y = area.y + area.height / 2;
            let x = area.x + 1;
            text_wordmark(&mut p, x, y, true);
            p.buf.set_string(x + 5, y, "cli", bold);
            p.buf.set_string(x + 9, y, VERSION, dim);
        }
    }
}

/// `nori` in bold with a green i dot.
fn text_wordmark(p: &mut Paint, x: u16, y: u16, dot: bool) {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    p.buf.set_string(x, y, "nor", bold);
    if dot {
        let green: Color = color(p.tier, Mat::Green, 1.0);
        p.buf.set_string(x + 3, y, "i", bold.fg(green));
    } else {
        p.buf.set_string(x + 3, y, "ı", bold);
    }
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

struct Opts {
    size: (u16, u16),
    from: u64,
    until: u64,
    settled: bool,
    tier: Tier,
    frame: u64,
}

fn write_cast(path: &str, o: &Opts) {
    let (w, h) = o.size;
    let sink = Sink::default();
    let mut term = Terminal::with_options(
        CrosstermBackend::new(sink.clone()),
        TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, w, h)) },
    )
    .expect("terminal");
    let mut f = File::create(path).expect("cast");
    writeln!(f, "{{\"version\": 2, \"width\": {w}, \"height\": {h}, \"env\": {{\"TERM\": \"xterm-256color\"}}}}")
        .expect("write");
    writeln!(f, "[0.0, \"o\", {}]", json_str(b"\x1b[?25l")).expect("write");
    let mut t = o.from;
    while t <= o.until {
        term.draw(|fr| {
            let area = fr.area();
            render(t, area, fr.buffer_mut(), o.settled, o.tier);
        })
        .expect("draw");
        let bytes = std::mem::take(&mut *sink.0.borrow_mut());
        if !bytes.is_empty() {
            writeln!(f, "[{:.3}, \"o\", {}]", (t - o.from) as f64 / 1000.0, json_str(&bytes)).expect("write");
        }
        t += o.frame;
    }
    writeln!(f, "[{:.3}, \"o\", \"\"]", (o.until - o.from) as f64 / 1000.0 + 0.5).expect("write");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut cast = None;
    let mut o = Opts { size: (100, 30), from: 0, until: 8000, settled: false, tier: Tier::detect(), frame: 40 };
    let mut at = None;
    let mut i = 1;
    while i < args.len() {
        let val = || args.get(i + 1).cloned().expect("value");
        match args[i].as_str() {
            "--cast" => cast = Some(val()),
            "--size" => {
                let v = val();
                let (a, b) = v.split_once('x').expect("WxH");
                o.size = (a.parse().expect("w"), b.parse().expect("h"));
            }
            "--until" => o.until = val().parse().expect("ms"),
            "--from" => o.from = val().parse().expect("ms"),
            "--frame" => o.frame = val().parse().expect("ms"),
            "--at" => at = Some(val().parse::<u64>().expect("ms")),
            "--tier" => o.tier = Tier::parse(&val()),
            "--static" => {
                o.settled = true;
                i -= 1;
            }
            other => panic!("unknown arg {other}"),
        }
        i += 2;
    }
    if o.settled {
        o.until = o.from;
    }
    if let Some(at) = at {
        o.from = at;
        o.until = at;
    }
    write_cast(&cast.expect("--cast FILE"), &o);
}
