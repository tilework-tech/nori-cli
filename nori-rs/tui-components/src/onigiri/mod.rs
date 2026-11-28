//! Onigiri: the animated first-launch welcome.
//!
//! The ball sizes itself to the area. With room for it (a ball at least 11
//! rows tall) the full welcome plays: grains spring together, get
//! pressed, the ball hops and lands with a wobble, the nori band wraps, then a light circles the rice. Smaller areas
//! get the settled ball, and the light keeps circling while the ball is big
//! enough to carry grain texture. Areas too small for a ball get a one-line
//! text wordmark. Beside the ball, the `nori` wordmark is drawn in rounded
//! box-drawing lines: a pen writes it, each letter ignites like neon, the i
//! dot drops in, and a shimmer then follows the light (see `word`). Colour
//! adapts to truecolor, 256 and ANSI 16 ([`Tier::detect`]).
//!
//! The caller owns the frame loop: call [`Onigiri::render`] with the time since
//! the welcome started and schedule the next frame from
//! [`Onigiri::next_frame_in`]. It returns 60 fps until the ball settles, then
//! the time to the light's next 30 Hz tick, and `None` once nothing moves.
//!
//! Rendering is built to be cheap. Everything that depends only on the area
//! (coverage, surface normals, grain tilts, the intro grain paths) is baked
//! once into a cached `Scene` and rebuilt only when the area changes, so a
//! loop frame computes lighting alone. The light moves on a fixed tick, so a
//! given tick always draws the same frame, and brightness is quantized through
//! a palette lookup so fewer cells change between frames. Only the short press
//! squash is computed live.
//!
//! The demo runner (`cargo run --release -p nori-tui-components --example
//! onigiri -- --live | --bench | --cast FILE`) lives in `demo/` and is not
//! part of the library.

mod ball;
mod color;
mod geometry;
mod grains;
mod layout;
mod lighting;
mod shape;
mod timeline;
mod word;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod word_tests;

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;

use ball::Ball;
use ball::Paint;
use color::Mat;
use color::Palette;
pub use color::Tier;
use geometry::Place;
use layout::INTRO_ROWS;
pub use layout::Layout;
use layout::TEXTURE_ROWS;
pub use layout::layout;
use lighting::Light;
use timeline::INTRO_FRAME;
use timeline::LAND_AT;
use timeline::LIGHT_AT;
use timeline::LIGHT_TICK;
use timeline::SETTLED_AT;
use timeline::TICK_SLACK;
use timeline::real_ms;
use timeline::scene_ms;
use word::Word;

/// Placeholder status text drawn next to the wordmark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caption {
    pub version: String,
    pub cwd: String,
}

/// Everything about the scene that depends only on the area it fills.
enum Scene {
    Side {
        rows: f32,
        ball: Ball,
        word: Box<Word>,
        left: i32,
        base: i32,
    },
    Stack {
        rows: f32,
        ball: Ball,
        cx: f32,
        base: f32,
    },
    Line,
}

impl Scene {
    fn new(area: Rect, intro: bool) -> Scene {
        match layout(area.width, area.height) {
            Layout::Side { rows } => {
                let s = Place::size_for(rows);
                let ball_w = Place::width(s);
                let total = ball_w + 0.2 * rows + 3.0 + word::width_for(rows);
                let ox = f32::from(area.x) + (f32::from(area.width) - total) / 2.0;
                let top = i32::from(area.y)
                    + ((f32::from(area.height) - rows - 2.0) / 2.0).round() as i32;
                let place = Place::with_rows(ox + ball_w / 2.0, top, rows);
                let base = place.base().round() as i32;
                let left = (ox + ball_w + 0.2 * rows + 3.0).round() as i32;
                let word = Box::new(Word::new(left, (base - 1).div_euclid(2), rows));
                Scene::Side {
                    rows,
                    ball: Ball::new(place, rows >= TEXTURE_ROWS, intro && rows >= INTRO_ROWS),
                    word,
                    left,
                    base,
                }
            }
            Layout::Stack { rows } => {
                let cx = f32::from(area.x) + f32::from(area.width) / 2.0;
                let top = i32::from(area.y)
                    + ((f32::from(area.height) - rows - 3.0) / 2.0).round() as i32;
                let place = Place::with_rows(cx, top, rows);
                Scene::Stack {
                    rows,
                    ball: Ball::new(place, rows >= TEXTURE_ROWS, intro && rows >= INTRO_ROWS),
                    cx,
                    base: place.base(),
                }
            }
            Layout::Line => Scene::Line,
        }
    }
}

/// The welcome animation. Render it every frame with the time since it
/// started; [`Onigiri::next_frame_in`] says when the next frame is due.
pub struct Onigiri {
    pal: Palette,
    intro: bool,
    caption: Caption,
    scene: Option<(Rect, Scene)>,
}

impl Onigiri {
    pub fn new(tier: Tier, caption: Caption) -> Self {
        Self {
            pal: Palette::new(tier),
            intro: true,
            caption,
            scene: None,
        }
    }

    /// Skip the intro and draw the settled ball with a still light.
    pub fn settled(mut self) -> Self {
        self.intro = false;
        self
    }

    /// How long until the next frame is due, or `None` once nothing moves.
    pub fn next_frame_in(&self, elapsed: Duration, area: Rect) -> Option<Duration> {
        let rows = match layout(area.width, area.height) {
            Layout::Side { rows } | Layout::Stack { rows } => rows,
            Layout::Line => return None,
        };
        let now = elapsed.as_secs_f32() * 1000.0;
        let t = scene_ms(now);
        let until = |at: f32| {
            Some(Duration::from_secs_f32((real_ms(at) - now).max(0.0) / 1000.0) + TICK_SLACK)
        };
        if !self.intro || rows < TEXTURE_ROWS {
            None
        } else if rows >= INTRO_ROWS && t < SETTLED_AT {
            Some(INTRO_FRAME)
        } else if t < LIGHT_AT {
            until(LIGHT_AT)
        } else {
            until(((t / LIGHT_TICK).floor() + 1.0) * LIGHT_TICK)
        }
    }

    /// When the ball and the i dot land, if the intro plays in `area`.
    pub fn landed_at(&self, area: Rect) -> Option<Duration> {
        let rows = match layout(area.width, area.height) {
            Layout::Side { rows } | Layout::Stack { rows } => rows,
            Layout::Line => return None,
        };
        (self.intro && rows >= INTRO_ROWS)
            .then(|| Duration::from_secs_f32(real_ms(LAND_AT) / 1000.0))
    }

    pub fn render(&mut self, elapsed: Duration, area: Rect, buf: &mut Buffer) {
        if self.scene.as_ref().is_none_or(|(a, _)| *a != area) {
            self.scene = Some((area, Scene::new(area, self.intro)));
        }
        let Some((_, scene)) = self.scene.as_mut() else {
            return;
        };
        let t = scene_ms(elapsed.as_secs_f32() * 1000.0);
        let settled = !self.intro;
        let dim = Style::default().add_modifier(Modifier::DIM);
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let caption = &self.caption;
        let clip = area.intersection(buf.area);
        let mut p = Paint {
            buf,
            pal: &self.pal,
            clip,
        };
        match scene {
            Scene::Side {
                rows,
                ball,
                word,
                left,
                base,
            } => {
                let intro = !settled && *rows >= INTRO_ROWS;
                let tw = if intro { t } else { 1.0e6 };
                let animate = !settled && *rows >= TEXTURE_ROWS;
                let light = Light::at(t, animate);
                ball.draw(&mut p, t, intro, &light);
                // Past the intro the wordmark, like the light, only changes
                // on the light's tick.
                let tick = if t >= SETTLED_AT {
                    (t / LIGHT_TICK).floor() * LIGHT_TICK
                } else {
                    t
                };
                word.draw(&mut p, tick, intro, &light, animate);
                if tw >= word::VERSION_AT {
                    let (left, y) = (*left, *base / 2 + 1);
                    p.put(left, y, "cli", Style::default());
                    p.put(left + 4, y, &caption.version, dim);
                    p.put(
                        left + 6 + caption.version.chars().count() as i32,
                        y,
                        &caption.cwd,
                        dim,
                    );
                }
            }
            Scene::Stack {
                rows,
                ball,
                cx,
                base,
            } => {
                let intro = !settled && *rows >= INTRO_ROWS;
                let tw = if intro { t } else { 1.0e6 };
                let light = Light::at(t, !settled && *rows >= TEXTURE_ROWS);
                ball.draw(&mut p, t, intro, &light);
                if tw >= word::WORD_AT {
                    let y = (*base / 2.0).round() as i32 + 1;
                    let x = (*cx - 8.0).round() as i32;
                    text_wordmark(&mut p, x, y, tw >= word::DOT_AT);
                    p.put(x + 5, y, "cli", Style::default());
                    p.put(x + 9, y, &caption.version, dim);
                }
            }
            Scene::Line => {
                let y = i32::from(area.y + area.height / 2);
                let x = i32::from(area.x) + 1;
                text_wordmark(&mut p, x, y, true);
                p.put(x + 5, y, "cli", bold);
                p.put(x + 9, y, &caption.version, dim);
            }
        }
    }
}

/// `nori` in bold with a green i dot.
fn text_wordmark(p: &mut Paint, x: i32, y: i32, dot: bool) {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    p.put(x, y, "nor", bold);
    if dot {
        let green = p.pal.color(Mat::Green, 1.0);
        p.put(x + 3, y, "i", bold.fg(green));
    } else {
        p.put(x + 3, y, "ı", bold);
    }
}
