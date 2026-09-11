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
    formation_segment: i32,
    formation_fade: f64,
    zoom_mix: f64,
    braille_mix: f64,
    fade: f64,
    sandbox_zoom: Option<f64>,
    noise: math::Noise,
    islands: [Option<zoom::Island>; 64],
    muster_rows: [Option<zoom::MusterRow>; 4],
}

impl Field {
    pub fn new(area: Rect, time: f64, phase: f64, formation: MotionFormation) -> Self {
        let morph = if phase < 3.0 {
            (phase - 2.0).max(0.0) * 0.55
        } else {
            0.55 + (phase - 3.0) * 0.45
        };
        let segment = match formation {
            MotionFormation::Cycle => (time / 24.0).floor() as i32,
            MotionFormation::Platoon => 0,
            MotionFormation::Muster => 3,
        };
        let local = time.rem_euclid(24.0) - 8.0;
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
            formation_segment: segment.rem_euclid(4),
            formation_fade: match formation {
                MotionFormation::Cycle => smooth(local / 1.6) * smooth((16.0 - local) / 1.6),
                MotionFormation::Platoon | MotionFormation::Muster => 1.0,
            },
            zoom_mix: smooth((phase.min(1.0) - 0.75) / 0.25),
            braille_mix: smooth((phase.min(1.0) - 0.45) / 0.55),
            fade: if (1.0..2.0).contains(&phase) {
                1.0 - smooth(1.0 - (2.0 * (phase - 1.0) - 1.0).abs())
            } else {
                1.0
            },
            sandbox_zoom: None,
            noise: math::Noise::new(time * 0.1),
            islands: [None; 64],
            muster_rows: [None; 4],
        }
    }

    pub fn sandbox_zoom(area: Rect, time: f64, progress: f64, formation: MotionFormation) -> Self {
        let mut field = Self::new(area, time, 1.0, formation);
        field.sandbox_zoom = Some(progress);
        field.scale = 28.0_f64.powf(1.0 - progress);
        field.braille_mix = smooth((progress - 0.25) / 0.65);
        field
    }

    pub fn cell(&mut self, x: i32, y: i32, ascii: bool) -> Sample {
        let threshold =
            (f64::from(BAYER[y.rem_euclid(4) as usize][x.rem_euclid(4) as usize]) + 0.5) / 16.0;
        let braille =
            !ascii && ((self.phase < 1.5 && threshold < self.braille_mix) || self.lines > 0.5);
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
        let fade = self.fade;
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

    fn value(&mut self, fx: f64, fy: f64) -> f64 {
        if let Some(progress) = self.sandbox_zoom {
            let mix = smooth((progress - 0.35) / 0.65);
            if mix == 1.0 {
                return self.zoom(fx, fy);
            }
            let sandbox = self.tiles(fx, fy);
            if mix == 0.0 {
                return sandbox;
            }
            sandbox + (self.zoom(fx, fy) - sandbox) * mix
        } else if self.phase < 1.5 {
            self.zoom(fx, fy)
        } else {
            self.tiles(fx, fy)
        }
    }
}
