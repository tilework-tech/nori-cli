//! Onigiri: the animated first-launch welcome.
//!
//! The ball sizes itself to the area. With room for it (a ball at least
//! [`layout::INTRO_ROWS`] tall) the full welcome plays: grains gather, get
//! pressed, the nori band wraps, then a light circles the rice. Smaller areas
//! get the settled ball, and the light keeps circling while the ball is big
//! enough to carry grain texture. Colour adapts to truecolor, 256 and ANSI 16.
//!
//! The demo runner (`cargo run --release -p nori-tui-components --example
//! onigiri -- --help`) lives in `demo/` and is not part of the library.

mod ball;
mod color;
mod layout;
mod shape;
mod word;

#[cfg(test)]
mod tests;

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;

use ball::Paint;
use ball::Place;
use ball::draw_ball;
pub use color::Tier;
use color::Mat;
use color::color;
pub use layout::Layout;
pub use layout::layout;
use layout::INTRO_ROWS;
use layout::TEXTURE_ROWS;
use layout::WORD_RATIO;

/// Placeholder status text drawn next to the wordmark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caption {
    pub version: String,
    pub cwd: String,
}

/// The welcome animation. Render it every frame with the time since it
/// started.
#[derive(Debug, Clone)]
pub struct Onigiri {
    tier: Tier,
    intro: bool,
    caption: Caption,
}

impl Onigiri {
    pub fn new(tier: Tier, caption: Caption) -> Self {
        Self { tier, intro: true, caption }
    }

    /// Skip the intro and draw the settled ball with a still light.
    pub fn settled(mut self) -> Self {
        self.intro = false;
        self
    }

    pub fn render(&mut self, elapsed: Duration, area: Rect, buf: &mut Buffer) {
        let t = elapsed.as_secs_f32() * 1000.0;
        let settled = !self.intro;
        let dim = Style::default().add_modifier(Modifier::DIM);
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let caption = &self.caption;
        let mut p = Paint { buf, tier: self.tier };
        match layout(area.width, area.height) {
            Layout::Side { rows } => {
                let intro = !settled && rows >= INTRO_ROWS;
                let tw = if intro { t } else { 1.0e6 };
                let s = Place::size_for(rows);
                let ball_w = Place::width(s);
                let scale = rows * WORD_RATIO;
                let total = ball_w + 0.2 * rows + 3.0 + word::WORD_W * scale;
                let ox = f32::from(area.x) + (f32::from(area.width) - total) / 2.0;
                let top = i32::from(area.y) + ((f32::from(area.height) - rows - 2.0) / 2.0).round() as i32;
                let place = Place::with_rows(ox + ball_w / 2.0, top, rows);
                draw_ball(&mut p, place, t, intro, !settled && rows >= TEXTURE_ROWS, rows >= TEXTURE_ROWS);
                let base = place.base().round() as i32;
                let left = (ox + ball_w + 0.2 * rows + 3.0).round() as i32;
                word::draw_word(&mut p, left, base - (9.0 * scale).round() as i32, tw, scale);
                if tw >= word::VERSION_AT {
                    let y = base / 2 + 1;
                    p.put(left, y, "cli", Style::default());
                    p.put(left + 4, y, &caption.version, dim);
                    p.put(left + 6 + caption.version.chars().count() as i32, y, &caption.cwd, dim);
                }
            }
            Layout::Stack { rows } => {
                let intro = !settled && rows >= INTRO_ROWS;
                let tw = if intro { t } else { 1.0e6 };
                let cx = f32::from(area.x) + f32::from(area.width) / 2.0;
                let top = i32::from(area.y) + ((f32::from(area.height) - rows - 3.0) / 2.0).round() as i32;
                let place = Place::with_rows(cx, top, rows);
                draw_ball(&mut p, place, t, intro, !settled && rows >= TEXTURE_ROWS, rows >= TEXTURE_ROWS);
                if tw >= word::WORD_AT {
                    let y = (place.base() / 2.0).round() as i32 + 1;
                    let x = (cx - 8.0).round() as i32;
                    text_wordmark(&mut p, x, y, tw >= word::DOT_AT);
                    p.put(x + 5, y, "cli", Style::default());
                    p.put(x + 9, y, &caption.version, dim);
                }
            }
            Layout::Line => {
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
        let green: Color = color(p.tier, Mat::Green, 1.0);
        p.put(x + 3, y, "i", bold.fg(green));
    } else {
        p.put(x + 3, y, "ı", bold);
    }
}
