//! The `nori` wordmark in rounded box-drawing lines.
//!
//! The intro writes all four letters at once, each with its own pen that
//! leaves unlit glass behind;
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
use super::timeline::BUMP_CROUCH;
use super::timeline::BUMP_HOP;
use super::timeline::BUMP_LAND;
use motion::BOUNCE;
pub use motion::DOT_AT;
use motion::DOT_GLOW;
use motion::DOT_LAND;
#[cfg(test)]
pub use motion::DRAW_END;
use motion::GLOW_MAX;
use motion::GLOW_R;
use motion::GLOW_REST;
use motion::IGNITE_DELAY;
use motion::IGNITE_DUR;
use motion::LETTER_DUR;
use motion::RIPPLE_ROW;
use motion::SHIMMER_GLOW;
use motion::SLANT;
use motion::SPLASH;
use motion::SQUASH;
#[cfg(test)]
pub use motion::STAGGER as PEN_STAGGER;
use motion::STAGGER;
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
    /// When the half stub shows, if the pen enters from a side.
    stub: Option<(f32, &'static str)>,
    /// When the pen completes the cell.
    full_at: f32,
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
    /// Each letter's pen: the cell it is on at each step, and when it
    /// starts.
    pen: [Vec<(i32, i32)>; 4],
    pen_at: [f32; 4],
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
        let letters = letters::nori(h);
        let steps = |path: &[(i32, i32)]| 2 * path.len() - 1;
        let longest = letters
            .iter()
            .map(|(_, l)| steps(&l.path))
            .max()
            .unwrap_or(1);
        let step_ms = LETTER_DUR / longest as f32;
        let pen_at = [0, 1, 2, 3].map(|k| WORD_AT + k as f32 * STAGGER);
        let mut strokes = Vec::new();
        let mut pen: [Vec<(i32, i32)>; 4] = Default::default();
        let mut stem_x = left;
        for (k, (x0, letter)) in letters.iter().enumerate() {
            let at = |step: usize| pen_at[k] + step as f32 * step_ms;
            for (i, (x, y)) in letter.path.iter().enumerate() {
                let (x, y) = (left + x0 + x, top + y);
                let stub = letter.stub(i).map(|s| {
                    pen[k].push((x, y));
                    (at(pen[k].len() - 1), s)
                });
                pen[k].push((x, y));
                strokes.push(Stroke {
                    x,
                    y,
                    glyph: letter.glyph(i),
                    letter: k,
                    stub,
                    full_at: at(pen[k].len() - 1),
                });
            }
            stem_x = left + x0;
        }
        let ignite_at =
            [0, 1, 2, 3].map(|k| pen_at[k] + pen[k].len() as f32 * step_ms + IGNITE_DELAY);
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
            pen_at,
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
    /// with `shimmer` the band follows `light`. `bump` is how far into a
    /// bump the scene is, if one plays; the i dot hops with the ball.
    pub fn draw(
        &self,
        p: &mut Paint,
        t: f32,
        intro: bool,
        bump: Option<f32>,
        light: &Light,
        shimmer: bool,
    ) {
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

        // Where each pen is now, with a seed for its sparks.
        let heads: Vec<(i32, i32, i32)> = (0..4)
            .filter_map(|k| {
                let step = ((ti - self.pen_at[k]) / self.step_ms).floor();
                let i = usize::try_from(step as i64)
                    .ok()
                    .filter(|_| step.is_finite())?;
                let (x, y) = *self.pen[k].get(i)?;
                Some((x, y, k as i32 * 1000 + i as i32))
            })
            .collect();
        for &(hx, hy, seed) in &heads {
            for j in 0..2 {
                let r = hash(seed * 3 + j, 17);
                let dx = (hash(seed, j * 7 + 1) * 5.0) as i32 - 2;
                let dy = (hash(j + 3, seed * 5) * 3.0) as i32 - 1;
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
            if ti < s.full_at {
                if let Some((at, stub)) = s.stub
                    && ti >= at
                {
                    p.glyph(s.x, s.y, stub, style(Mat::Tube, 1.0, p));
                }
                continue;
            }
            let dt = ti - self.ignite_at[s.letter];
            let (m, b) = match ignite(dt) {
                None => (Mat::Tube, 1.0 - 0.9 * clamp01((ti - s.full_at) / TRAIL)),
                Some(lv) if dt < IGNITE_DUR => (Mat::Tube, 0.1 + 0.9 * lv),
                Some(_) => (Mat::Shine, (1.0 - cooled(dt)).max(shine(s.x, s.y))),
            };
            let mut st = style(m, b, p);
            if heads.iter().any(|h| (h.0, h.1) == (s.x, s.y)) {
                st = style(Mat::Tube, 1.0, p).add_modifier(Modifier::BOLD);
            }
            p.glyph(s.x, s.y, s.glyph, st);
        }
        if !ansi {
            for &(hx, hy, _) in &heads {
                p.tint(hx, hy, p.pal.color(Mat::Glow, 0.9));
            }
        }

        self.draw_dot(p, ti, bump, ansi);
    }

    fn draw_dot(&self, p: &mut Paint, t: f32, bump: Option<f32>, ansi: bool) {
        if t < DOT_AT {
            return;
        }
        let green = Style::default().fg(p.pal.color(Mat::Green, 1.0));
        let since = t - DOT_LAND;
        let start = self.dot_top - 2 * (self.h + 3);
        if since < 0.0 {
            let top = start + ((self.dot_top - start) as f32 * fall(t)).round() as i32;
            self.half_rect(p, self.dot_x, self.dot_w, top, top + self.dot_w - 1, green);
            return;
        }
        match bump {
            // The dot crouches with the ball, hops and lands with it, then
            // ripples and bounces as it did when it first landed, without
            // the splash and glow.
            Some(b) if since >= SQUASH + BOUNCE => {
                if b < BUMP_CROUCH {
                    self.squashed_dot(p, green);
                } else if b < BUMP_LAND {
                    let x = (b - BUMP_CROUCH) / BUMP_HOP;
                    let lift = ((2 * self.dot_w + 2) as f32 * 4.0 * x * (1.0 - x)).round() as i32;
                    let top = self.dot_top - lift;
                    self.half_rect(p, self.dot_x, self.dot_w, top, top + self.dot_w - 1, green);
                } else {
                    self.landed_dot(p, b - BUMP_LAND, false, ansi, green);
                }
            }
            _ => self.landed_dot(p, since, true, ansi, green),
        }
    }

    fn squashed_dot(&self, p: &mut Paint, green: Style) {
        if self.dot_w == 1 {
            p.glyph(self.dot_x, self.top - 1, "▬", green);
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
    }

    /// The dot `since` ms after landing: a ripple down the stem, a squash
    /// and a bounce, with a splash and a glow when `splash`.
    fn landed_dot(&self, p: &mut Paint, since: f32, splash: bool, ansi: bool, green: Style) {
        let rest_row = self.top - 1;
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
        if splash && since < SPLASH {
            let x = since / SPLASH;
            if x < 0.75 {
                let reach = self.dot_w + (2.0 * x * self.dot_w as f32).round() as i32;
                let y = rest_row - i32::from(x < 0.5);
                p.glyph(self.dot_x - reach, y, "·", green);
                p.glyph(self.dot_x + self.dot_w - 1 + reach, y, "·", green);
            }
        }
        if since < SQUASH {
            self.squashed_dot(p, green);
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
        if splash && !ansi && lift == 0 {
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
