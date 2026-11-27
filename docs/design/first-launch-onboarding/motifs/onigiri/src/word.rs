//! Placeholder wordmark from the first draft. It will be redrawn; it is kept
//! only so the layout has the right footprint.

use ratatui::style::{Modifier, Style};

use crate::ball::Paint;
use crate::color::{color, Mat};
use crate::shape::*;

pub const WORD_AT: f32 = 1320.0;
const WORD_SWEEP: f32 = 380.0;
const WORD_FLASH: f32 = 100.0;
pub const DOT_AT: f32 = 1820.0;
const DOT_DUR: f32 = 160.0;
pub const VERSION_AT: f32 = WORD_AT + WORD_SWEEP + 120.0;
/// Width in letter units; height is 9 half cells per unit of scale.
pub const WORD_W: f32 = 35.0;

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
    if rbox(x, y, a, 0.0, a + w, H, [4.2; 4]) < 0.0 && rbox(x, y, a + S, S, a + w - S, H - S, [1.6; 4]) > 0.0 {
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

fn quad_mask(f: &dyn Fn(f32, f32) -> K, want: K, cx: i32, cy: i32) -> u32 {
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

/// Wordmark with its x-height top at half-cell `top`, left edge at `left`.
pub fn draw_word(p: &mut Paint, left: i32, top: i32, t: f32, scale: f32) {
    let f = |x: f32, y: f32| word_class((x - left as f32) / scale, (y - top as f32) * ASP / scale);
    let w = (WORD_W * scale).ceil() as i32;
    let y0 = top.div_euclid(2) - 3;
    let y1 = (top + (10.0 * scale) as i32).div_euclid(2) + 1;
    let green = color(p.tier, Mat::Green, 1.0);
    for cy in y0..y1 {
        for cx in left..left + w + 1 {
            let xw = (cx - left) as f32 / scale;
            let letter = [11.0, 23.0, 31.5].iter().filter(|b| xw >= **b).count() as f32;
            let at = WORD_AT + WORD_SWEEP / 3.0 * letter + 25.0 * hash(cx, cy);
            if t >= DOT_AT {
                let k = clamp01((t - DOT_AT) / DOT_DUR);
                let lift = (1.0 - ease_out(k)) * 5.0;
                let g = |x: f32, y: f32| f(x, y + lift);
                let m = quad_mask(&g, K::Green, cx, cy);
                if m != 0 {
                    p.put(cx, cy, QUAD[m as usize], Style::default().fg(green));
                    continue;
                }
            }
            if t < at {
                continue;
            }
            if t < at + WORD_FLASH {
                let mut bits = 0;
                for (r, row) in BRAILLE_BITS.iter().enumerate() {
                    for (c, bit) in row.iter().enumerate() {
                        let x = cx as f32 + (c as f32 + 0.5) / 2.0;
                        let y = 2.0 * cy as f32 + (r as f32 + 0.5) / 2.0;
                        if f(x, y) == K::Ink {
                            bits |= bit;
                        }
                    }
                }
                if bits != 0 {
                    p.put(cx, cy, &braille(bits), Style::default().add_modifier(Modifier::DIM));
                }
                continue;
            }
            let m = quad_mask(&f, K::Ink, cx, cy);
            if m != 0 {
                p.put(cx, cy, QUAD[m as usize], Style::default());
            }
        }
    }
}
