//! Cell-space adaptation of the supplied Nori Motion Suite canvas formulas.
//! Two horizontal and four vertical samples form one terminal braille cell.

mod math;
mod tiles;
mod zoom;

pub(super) use math::smooth;

use ratatui::layout::Rect;

use super::MotionFormation;

const BAYER: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
const BITS: [[i32; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];
const DOTS: [char; 5] = [' ', '·', '∘', '○', '●'];
const ASCII: [char; 5] = [' ', '.', ':', 'o', '*'];

pub(super) struct Sample {
    pub glyph: char,
    pub intensity: f64,
    pub highlight: bool,
}

pub(super) struct Field {
    width: f64,
    height: f64,
    time: f64,
    phase: f64,
    scale: f64,
    shape: f64,
    lines: f64,
    formation: MotionFormation,
}

impl Field {
    pub fn new(area: Rect, time: f64, phase: f64, formation: MotionFormation) -> Self {
        let morph = if phase < 3.0 {
            (phase - 2.0).max(0.0) * 0.55
        } else {
            0.55 + (phase - 3.0) * 0.45
        };
        Self {
            width: f64::from(area.width) * 2.0,
            height: f64::from(area.height) * 4.0,
            time,
            phase,
            scale: if phase < 1.0 {
                28.0_f64.powf(1.0 - phase)
            } else {
                28.0 + 8.0 * smooth(morph / 0.55)
            },
            shape: smooth(morph / 0.6),
            lines: smooth((morph - 0.55) / 0.45),
            formation,
        }
    }

    pub fn cell(&self, x: i32, y: i32, ascii: bool) -> Sample {
        let threshold =
            (f64::from(BAYER[y.rem_euclid(4) as usize][x.rem_euclid(4) as usize]) + 0.5) / 16.0;
        let braille = !ascii
            && ((self.phase < 1.5 && threshold < smooth((self.phase.min(1.0) - 0.45) / 0.55))
                || self.lines > 0.5);
        let mut sum = 0.0;
        let glyph = if braille {
            let mut bits = 0;
            for sy in 0..4 {
                for sx in 0..2 {
                    let fx = x * 2 + sx;
                    let fy = y * 4 + sy;
                    let value = self.value(f64::from(fx), f64::from(fy));
                    sum += value / 8.0;
                    let threshold =
                        (f64::from(BAYER[fy.rem_euclid(4) as usize][fx.rem_euclid(4) as usize])
                            + 0.5)
                            / 16.0;
                    if value > threshold {
                        bits |= BITS[sy as usize][sx as usize];
                    }
                }
            }
            if bits == 0 {
                ' '
            } else {
                char::from_u32(0x2800 + bits as u32).unwrap_or(' ')
            }
        } else {
            sum = self.value(f64::from(x * 2 + 1), f64::from(y * 4 + 2));
            let level = (sum.clamp(0.0, 0.999) * 4.0 + threshold).floor() as usize;
            if ascii { ASCII[level] } else { DOTS[level] }
        };
        let fade = if (1.0..2.0).contains(&self.phase) {
            1.0 - smooth(1.0 - (2.0 * (self.phase - 1.0) - 1.0).abs())
        } else {
            1.0
        };
        let eye = if self.phase >= 2.0 && self.lines < 0.7 {
            self.eye(x, y)
        } else {
            None
        };
        Sample {
            glyph: eye.unwrap_or(glyph),
            intensity: if eye.is_some() {
                0.8 * fade
            } else {
                (0.15 + sum * 0.35) * fade
            },
            highlight: eye.is_some() || self.lines > 0.5,
        }
    }

    fn value(&self, fx: f64, fy: f64) -> f64 {
        if self.phase < 1.5 {
            self.zoom(fx, fy)
        } else {
            self.tiles(fx, fy)
        }
    }
}
