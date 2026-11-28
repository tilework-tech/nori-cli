//! The `nori` wordmark in rounded box-drawing lines.
//!
//! The intro writes each letter with a pen that leaves unlit glass behind;
//! each finished letter then ignites like a neon tube, glows and cools to
//! the resting wordmark. The i dot falls in last, squashes, bounces and
//! sends a ripple down the stem. After that a shimmer runs across the
//! letters and back, positioned by the same light that circles the rice, so
//! it makes one round trip per turn of the light.
//!
//! The strokes, pen order and glow field are baked when the wordmark is
//! placed; a frame only works out colours. Timings and strengths live in
//! `motion`.

mod letters;
mod motion;

use ratatui::style::Modifier;
use ratatui::style::Style;

use super::ball::Paint;
use super::color::Mat;
use super::color::Tier;
use super::lighting::Light;
use super::shape::clamp01;
use super::shape::hash;
use motion::BOUNCE;
pub use motion::DOT_AT;
use motion::DOT_GLOW;
use motion::DOT_LAND;
pub use motion::DRAW_END;
use motion::GLOW_MAX;
use motion::GLOW_R;
use motion::GLOW_REST;
use motion::IGNITE_DELAY;
use motion::IGNITE_DUR;
use motion::LIFT_STEPS;
use motion::RIPPLE_ROW;
use motion::SHIMMER_GLOW;
use motion::SLANT;
use motion::SPLASH;
use motion::SQUASH;
use motion::TAIL_W;
use motion::TRAIL;
pub use motion::VERSION_AT;
pub use motion::WORD_AT;
use motion::cooled;
use motion::fall;
use motion::glow;
use motion::ignite;
use motion::shimmer_amp;

/// Letter height as a fraction of the ball height.
const LETTER_RATIO: f32 = 0.28;
const SPARKS: [&str; 5] = ["·", "˙", "∙", "⁺", "*"];

/// Letter height in rows next to a ball `rows` tall.
pub fn letter_rows(rows: f32) -> i32 {
    ((rows * LETTER_RATIO).round() as i32).max(2)
}

/// Width in cells of the wordmark next to a ball `rows` tall.
pub fn width_for(rows: f32) -> f32 {
    letters::width(letter_rows(rows)) as f32
}

struct Stroke {
    x: i32,
    y: i32,
    glyph: &'static str,
    letter: usize,
    /// Pen step that shows the half stub, if the pen enters from a side.
    stub: Option<(i32, &'static str)>,
    /// Pen step that completes the cell.
    full: i32,
}

/// A cell near the strokes and how much glow each letter casts on it.
struct GlowCell {
    x: i32,
    y: i32,
    field: [f32; 4],
}

pub struct Word {
    h: i32,
    left: i32,
    top: i32,
    width: i32,
    strokes: Vec<Stroke>,
    glow: Vec<GlowCell>,
    /// The cell the pen is on at each step, `None` while it is lifted.
    pen: Vec<Option<(i32, i32)>>,
    step_ms: f32,
    ignite_at: [f32; 4],
    stem_x: i32,
    /// The i dot: left cell, width in cells (and height in half cells), and
    /// the half cell its top rests on.
    dot_x: i32,
    dot_w: i32,
    dot_top: i32,
}

impl Word {
    /// The wordmark beside a ball `rows` tall, its left edge at cell `left`
    /// and its baseline on row `bottom`.
    pub fn new(left: i32, bottom: i32, rows: f32) -> Word {
        let h = letter_rows(rows);
        let top = bottom - h + 1;
        let mut strokes = Vec::new();
        let mut pen = Vec::new();
        let mut done = [0; 4];
        let mut stem_x = left;
        for (k, (x0, letter)) in letters::nori(h).into_iter().enumerate() {
            if k > 0 {
                pen.extend((0..LIFT_STEPS).map(|_| None));
            }
            for (i, (x, y)) in letter.path.iter().enumerate() {
                let (x, y) = (left + x0 + x, top + y);
                let stub = letter.stub(i).map(|s| {
                    pen.push(Some((x, y)));
                    (pen.len() as i32 - 1, s)
                });
                pen.push(Some((x, y)));
                strokes.push(Stroke {
                    x,
                    y,
                    glyph: letter.glyph(i),
                    letter: k,
                    stub,
                    full: pen.len() as i32 - 1,
                });
            }
            done[k] = pen.len();
            stem_x = left + x0;
        }
        let step_ms = (DRAW_END - WORD_AT) / pen.len() as f32;
        let ignite_at = done.map(|d| WORD_AT + d as f32 * step_ms + IGNITE_DELAY);
        let dot_w = ((h as f32 / 4.0).round() as i32).max(1);
        let width = letters::width(h);
        let mut word = Word {
            h,
            left,
            top,
            width,
            strokes,
            glow: Vec::new(),
            pen,
            step_ms,
            ignite_at,
            stem_x,
            dot_x: stem_x - (dot_w - 1) / 2,
            dot_w,
            dot_top: 2 * (top - 1) + 2 - dot_w,
        };
        word.glow = word.bake_glow();
        word
    }

    fn bake_glow(&self) -> Vec<GlowCell> {
        let r = GLOW_R.0 + GLOW_R.1 * self.h as f32;
        let (rx, ry) = ((2.5 * r).ceil() as i32, (1.25 * r).ceil() as i32);
        let dot = (self.stem_x, self.top - 1);
        let mut out = Vec::new();
        for y in self.top - 1 - ry..=self.top + self.h + ry {
            for x in self.left - rx.min(2)..=self.left + self.width + rx {
                let mut field = [0.0; 4];
                let sources = self.strokes.iter().map(|s| (s.x, s.y, s.letter));
                for (sx, sy, k) in sources.chain([(dot.0, dot.1, 3)]) {
                    let d = ((x - sx) as f32).hypot(2.0 * (y - sy) as f32) / r;
                    field[k] += (-d * d).exp();
                }
                if field.iter().any(|f| *f > 0.02) {
                    out.push(GlowCell { x, y, field });
                }
            }
        }
        out
    }

    /// Draw the wordmark at `t` ms. Without `intro` it is drawn at rest;
    /// with `shimmer` the band follows `light`.
    pub fn draw(&self, p: &mut Paint, t: f32, intro: bool, light: &Light, shimmer: bool) {
        let ti = if intro { t } else { f32::INFINITY };
        let ansi = p.tier() == Tier::Ansi16;
        let style = |m: Mat, b: f32, p: &Paint| {
            let s = Style::default().fg(p.pal.color(m, b));
            if ansi && b > 0.7 {
                s.add_modifier(Modifier::BOLD)
            } else {
                s
            }
        };

        // Shimmer band: the light's left/right position mapped across the
        // word, trailing behind the way it moves.
        let amp = if shimmer { shimmer_amp(t) } else { 0.0 };
        let (hf, mid) = (self.h as f32, self.top as f32 + self.h as f32 / 2.0);
        let margin = TAIL_W.0 * hf + TAIL_W.1;
        let side = light.l[0] / light.l[0].hypot(light.l[1]).max(1e-6);
        let pos =
            self.left as f32 - margin + (0.5 + 0.5 * side) * (self.width as f32 + 2.0 * margin);
        let moving_right = light.l[1] < 0.0;
        let shine = |x: i32, y: i32| {
            if amp <= 0.0 {
                return 0.0;
            }
            let dx = x as f32 + SLANT * (y as f32 - mid) - pos;
            amp * motion::shimmer(dx, moving_right, hf)
        };

        let pen_step = ((ti - WORD_AT) / self.step_ms).floor();
        let step = if pen_step.is_finite() {
            pen_step as i64
        } else {
            i64::MAX
        };
        let glow_of = |k: usize| {
            let dt = ti - self.ignite_at[k];
            match ignite(dt) {
                None => 0.0,
                Some(lv) if dt < IGNITE_DUR => lv,
                Some(_) => {
                    let c = cooled(dt);
                    1.0 - c + c * GLOW_REST
                }
            }
        };

        if !ansi {
            let gl = [0, 1, 2, 3].map(glow_of);
            for g in &self.glow {
                let field: f32 = g.field.iter().zip(gl).map(|(f, l)| f * l).sum();
                let near = glow(g.field.iter().sum()) / GLOW_MAX;
                let b = glow(field).max(SHIMMER_GLOW * near * shine(g.x, g.y));
                if b > 0.03 {
                    p.tint(g.x, g.y, p.pal.color(Mat::Glow, b));
                }
            }
        }

        let head = usize::try_from(step)
            .ok()
            .and_then(|s| self.pen.get(s).copied().flatten());
        if let Some((hx, hy)) = head {
            for j in 0..3 {
                let r = hash(step as i32 * 3 + j, 17);
                let dx = (hash(step as i32, j * 7 + 1) * 5.0) as i32 - 2;
                let dy = (hash(j + 3, step as i32 * 5) * 3.0) as i32 - 1;
                if (dx, dy) != (0, 0) {
                    let (m, b) = if r < 0.5 {
                        (Mat::Green, 1.0)
                    } else {
                        (Mat::Tube, 0.9)
                    };
                    let glyph = SPARKS[(r * 4.99) as usize];
                    p.glyph(hx + dx, hy + dy, glyph, style(m, b, p));
                }
            }
        }

        for s in &self.strokes {
            if i64::from(s.full) > step {
                if let Some((at, stub)) = s.stub
                    && i64::from(at) <= step
                {
                    p.glyph(s.x, s.y, stub, style(Mat::Tube, 1.0, p));
                }
                continue;
            }
            let dt = ti - self.ignite_at[s.letter];
            let (m, b) = match ignite(dt) {
                None => {
                    let age = ti - (WORD_AT + s.full as f32 * self.step_ms);
                    (Mat::Tube, 1.0 - 0.9 * clamp01(age / TRAIL))
                }
                Some(lv) if dt < IGNITE_DUR => (Mat::Tube, 0.1 + 0.9 * lv),
                Some(_) => (Mat::Shine, (1.0 - cooled(dt)).max(shine(s.x, s.y))),
            };
            let mut st = style(m, b, p);
            if head == Some((s.x, s.y)) {
                st = style(Mat::Tube, 1.0, p).add_modifier(Modifier::BOLD);
            }
            p.glyph(s.x, s.y, s.glyph, st);
        }
        if let Some((hx, hy)) = head
            && !ansi
        {
            p.tint(hx, hy, p.pal.color(Mat::Glow, 0.9));
        }

        self.draw_dot(p, ti, ansi);
    }

    fn draw_dot(&self, p: &mut Paint, t: f32, ansi: bool) {
        if t < DOT_AT {
            return;
        }
        let green = Style::default().fg(p.pal.color(Mat::Green, 1.0));
        let rest_row = self.top - 1;
        let since = t - DOT_LAND;
        let start = self.dot_top - 2 * (self.h + 3);
        if since < 0.0 {
            let top = start + ((self.dot_top - start) as f32 * fall(t)).round() as i32;
            self.half_rect(p, self.dot_x, self.dot_w, top, top + self.dot_w - 1, green);
            return;
        }
        // Ripple down the stem, then a splash either side.
        let front = since / RIPPLE_ROW;
        if front <= (self.h + 1) as f32 {
            for y in self.top..self.top + self.h {
                if (front - (y - self.top) as f32).abs() < 1.0 {
                    let glyph = self.strokes.iter().find(|s| s.x == self.stem_x && s.y == y);
                    if let Some(s) = glyph {
                        p.glyph(s.x, s.y, s.glyph, green);
                    }
                }
            }
        }
        if since < SPLASH {
            let x = since / SPLASH;
            if x < 0.75 {
                let reach = self.dot_w + (2.0 * x * self.dot_w as f32).round() as i32;
                let y = rest_row - i32::from(x < 0.5);
                p.glyph(self.dot_x - reach, y, "·", green);
                p.glyph(self.dot_x + self.dot_w - 1 + reach, y, "·", green);
            }
        }
        if since < SQUASH {
            if self.dot_w == 1 {
                p.glyph(self.dot_x, rest_row, "▬", green);
            } else {
                let bottom = self.dot_top + self.dot_w - 1;
                let h = (self.dot_w / 2).max(1);
                self.half_rect(
                    p,
                    self.dot_x - 1,
                    self.dot_w + 2,
                    bottom - h + 1,
                    bottom,
                    green,
                );
            }
            return;
        }
        let x = clamp01((since - SQUASH) / BOUNCE);
        let lift = ((self.dot_w + 1) as f32 * 4.0 * x * (1.0 - x)).round() as i32;
        if lift > 0 || self.dot_w > 1 {
            let top = self.dot_top - lift;
            self.half_rect(p, self.dot_x, self.dot_w, top, top + self.dot_w - 1, green);
        } else {
            p.glyph(self.dot_x, rest_row, "■", green);
        }
        if !ansi && lift == 0 {
            let b = 0.8 * (-(since - SQUASH - BOUNCE).max(0.0) / DOT_GLOW).exp();
            if b > 0.03 {
                for y in self.dot_top.div_euclid(2)..=(self.dot_top + self.dot_w - 1).div_euclid(2)
                {
                    for x in self.dot_x..self.dot_x + self.dot_w {
                        p.tint(x, y, p.pal.color(Mat::Glow, b));
                    }
                }
            }
        }
    }

    /// Fill half cells `top..=bottom` (in half-cell rows) across `w` cells.
    fn half_rect(&self, p: &mut Paint, x0: i32, w: i32, top: i32, bottom: i32, style: Style) {
        for row in top.div_euclid(2)..=bottom.div_euclid(2) {
            let upper = (top..=bottom).contains(&(2 * row));
            let lower = (top..=bottom).contains(&(2 * row + 1));
            let glyph = match (upper, lower) {
                (true, true) => "█",
                (true, false) => "▀",
                (false, true) => "▄",
                (false, false) => continue,
            };
            for x in x0..x0 + w {
                p.glyph(x, row, glyph, style);
            }
        }
    }
}
