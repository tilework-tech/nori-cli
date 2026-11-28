//! Placeholder wordmark from the first draft. It will be redrawn; it is kept
//! only so the layout has the right footprint. Each cell's glyphs are
//! baked when the wordmark is placed; only the i dot's drop is computed live.

use ratatui::style::Modifier;
use ratatui::style::Style;

use super::ball::Paint;
use super::color::Mat;
use super::shape::ASP;
use super::shape::BRAILLE_BITS;
use super::shape::QUAD;
use super::shape::braille;
use super::shape::clamp01;
use super::shape::ease_out;
use super::shape::hash;
use super::shape::rbox;

pub const WORD_AT: f32 = 1320.0;
const WORD_SWEEP: f32 = 380.0;
const WORD_FLASH: f32 = 100.0;
pub const DOT_AT: f32 = 1820.0;
const DOT_DUR: f32 = 160.0;
pub const VERSION_AT: f32 = WORD_AT + WORD_SWEEP + 120.0;
/// Width in letter units; height is 9 half cells per unit of scale.
pub const WORD_W: f32 = 35.0;
/// Horizontal extent of the i dot, in letter units.
const DOT_X: (f32, f32) = (32.0, 35.0);

#[derive(Clone, Copy, PartialEq, Eq)]
enum K {
    Empty,
    Ink,
    Green,
}

/// Wordmark in letter units (x right, y down, x-height top at 0, baseline H).
fn word_class(x: f32, y: f32) -> K {
    const H: f32 = 9.0 * ASP;
    const S: f32 = 3.0;
    if y > H {
        return K::Empty;
    }
    let (a, w) = (0.0, 10.0);
    if rbox(x, y, a, 0.0, a + w, H + 6.0, [1.0, 5.0, 0.0, 0.0]) < 0.0
        && rbox(x, y, a + S, S, a + w - S, H + 12.0, [0.5, 2.0, 0.0, 0.0]) > 0.0
    {
        return K::Ink;
    }
    let (a, w) = (11.5, 10.5);
    if rbox(x, y, a, 0.0, a + w, H, [4.2; 4]) < 0.0
        && rbox(x, y, a + S, S, a + w - S, H - S, [1.6; 4]) > 0.0
    {
        return K::Ink;
    }
    let (a, w) = (23.5, 7.0);
    if rbox(x, y, a, 0.0, a + w, H + 6.0, [1.0, 4.5, 0.0, 0.0]) < 0.0
        && rbox(x, y, a + S, S, a + 40.0, H + 40.0, [0.5, 0.0, 0.0, 0.0]) > 0.0
    {
        return K::Ink;
    }
    let a = 32.0;
    if rbox(x, y, a, 0.0, a + S, H + 6.0, [0.6, 0.6, 0.0, 0.0]) < 0.0 {
        return K::Ink;
    }
    if rbox(x, y, a, -3.0 * ASP, a + S, -ASP, [0.7; 4]) < 0.0 {
        return K::Green;
    }
    K::Empty
}

fn quad_mask(f: &dyn Fn(f32, f32) -> K, want: K, cx: i32, cy: i32) -> i32 {
    let mut mask = 0;
    for q in 0..4 {
        let (qx, qy) = ((q % 2) as f32, (q / 2) as f32);
        let mut n = 0;
        for j in 0..3 {
            for i in 0..3 {
                let x = cx as f32 + qx * 0.5 + (i as f32 + 0.5) / 6.0;
                let y = 2.0 * cy as f32 + qy + (j as f32 + 0.5) / 3.0;
                if f(x, y) == want {
                    n += 1;
                }
            }
        }
        if n >= 5 {
            mask |= 1 << q;
        }
    }
    mask
}

struct Cell {
    x: i32,
    y: i32,
    /// When the cell's letter sweeps in.
    at: f32,
    ink: i32,
    /// Braille dots shown while the letter flashes in.
    flash: i32,
    /// The i dot once it has dropped into place.
    green: i32,
    /// Whether the dropping i dot can pass through this cell.
    dot_column: bool,
}

/// The wordmark with its x-height top at half-cell `top`, left edge at
/// `left`.
pub struct Word {
    left: i32,
    top: i32,
    scale: f32,
    cells: Vec<Cell>,
}

impl Word {
    pub fn new(left: i32, top: i32, scale: f32) -> Word {
        let word = Word {
            left,
            top,
            scale,
            cells: Vec::new(),
        };
        let f = |x: f32, y: f32| word.class(x, y, 0.0);
        let w = (WORD_W * scale).ceil() as i32;
        let y0 = top.div_euclid(2) - 3;
        let y1 = (top + (10.0 * scale) as i32).div_euclid(2) + 1;
        let mut cells = Vec::new();
        for cy in y0..y1 {
            for cx in left..left + w + 1 {
                let xw = (cx - left) as f32 / scale;
                let letter = [11.0, 23.0, 31.5].iter().filter(|b| xw >= **b).count() as f32;
                let mut flash = 0;
                for (r, row) in BRAILLE_BITS.iter().enumerate() {
                    for (c, bit) in row.iter().enumerate() {
                        let x = cx as f32 + (c as f32 + 0.5) / 2.0;
                        let y = 2.0 * cy as f32 + (r as f32 + 0.5) / 2.0;
                        if f(x, y) == K::Ink {
                            flash |= bit;
                        }
                    }
                }
                cells.push(Cell {
                    x: cx,
                    y: cy,
                    at: WORD_AT + WORD_SWEEP / 3.0 * letter + 25.0 * hash(cx, cy),
                    ink: quad_mask(&f, K::Ink, cx, cy),
                    flash,
                    green: quad_mask(&f, K::Green, cx, cy),
                    dot_column: xw <= DOT_X.1 && (cx + 1 - left) as f32 / scale >= DOT_X.0,
                });
            }
        }
        Word { cells, ..word }
    }

    /// What the wordmark shows at cell-space point (`x`, `y`) with the i dot
    /// raised by `lift` half cells (letters are raised too, but only the dot
    /// is read with a lift).
    fn class(&self, x: f32, y: f32, lift: f32) -> K {
        word_class(
            (x - self.left as f32) / self.scale,
            (y + lift - self.top as f32) * ASP / self.scale,
        )
    }

    pub fn draw(&self, p: &mut Paint, t: f32) {
        let green = p.pal.color(Mat::Green, 1.0);
        let lift = if t < DOT_AT + DOT_DUR {
            (1.0 - ease_out(clamp01((t - DOT_AT) / DOT_DUR))) * 5.0
        } else {
            0.0
        };
        for c in &self.cells {
            if t >= DOT_AT {
                let m = if lift > 0.0 && c.dot_column {
                    quad_mask(&|x, y| self.class(x, y, lift), K::Green, c.x, c.y)
                } else if lift > 0.0 {
                    0
                } else {
                    c.green
                };
                if m != 0 {
                    p.glyph(c.x, c.y, QUAD[m as usize], Style::default().fg(green));
                    continue;
                }
            }
            if t < c.at {
                continue;
            }
            if t < c.at + WORD_FLASH {
                if c.flash != 0 {
                    p.glyph(
                        c.x,
                        c.y,
                        braille(c.flash),
                        Style::default().add_modifier(Modifier::DIM),
                    );
                }
                continue;
            }
            if c.ink != 0 {
                p.glyph(c.x, c.y, QUAD[c.ink as usize], Style::default());
            }
        }
    }
}
