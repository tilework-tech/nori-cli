//! Cell-space adaptation of the supplied Nori Motion Suite canvas formulas.
//! Two horizontal and four vertical samples form one terminal braille cell.

use std::f64::consts::TAU;

use ratatui::layout::Rect;

use super::MotionFormation;

const BAYER: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
const BITS: [[i32; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];
const DOTS: [char; 5] = [' ', '·', '∘', '○', '●'];
const ASCII: [char; 5] = [' ', '.', ':', 'o', '*'];

pub(super) fn smooth(value: f64) -> f64 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

fn hash(x: i32, y: i32, z: i32) -> f64 {
    let mut h = x
        .wrapping_mul(374_761_393)
        .wrapping_add(y.wrapping_mul(668_265_263))
        .wrapping_add(z.wrapping_mul(1_274_126_177));
    h = (h ^ ((h as u32 >> 13) as i32)).wrapping_mul(1_274_126_177);
    f64::from((h ^ ((h as u32 >> 16) as i32)) as u32) / f64::from(u32::MAX)
}

fn noise(x: f64, y: f64, t: f64) -> f64 {
    let mut value = 0.0;
    let sx = smooth(x - x.floor());
    let sy = smooth(y - y.floor());
    let st = smooth(t - t.floor());
    for dz in 0..2 {
        for dy in 0..2 {
            for dx in 0..2 {
                value += hash(
                    x.floor() as i32 + dx,
                    y.floor() as i32 + dy,
                    t.floor() as i32 + dz,
                ) * if dx == 0 { 1.0 - sx } else { sx }
                    * if dy == 0 { 1.0 - sy } else { sy }
                    * if dz == 0 { 1.0 - st } else { st };
            }
        }
    }
    value
}

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
            let z = self.phase.min(1.0);
            let scale = if self.phase >= 1.0 { 1.0 } else { self.scale };
            let wx = (fx - self.width / 2.0) / scale + self.time * 0.02;
            let wy = (fy - self.height / 2.0) / scale + self.time * 0.008;
            let phase = hash(wx.floor() as i32, wy.floor() as i32, 0) * TAU;
            let speed = 0.5 + hash(wx.floor() as i32, wy.floor() as i32, 1) * 0.9;
            let t = self.time * 0.5;
            let u = (wx - wx.floor() - 0.5) * 2.0;
            let v = (wy - wy.floor() - 0.5) * 2.0;
            let mx = 0.26 * (t * 0.43 * speed + phase).sin();
            let my = 0.26 * (t * 0.31 * speed + phase * 1.7).cos();
            let window = ((1.0 - u.abs().max(v.abs())) * 2.2).clamp(0.0, 1.0);
            let dots = (1.0 - (u - mx).hypot(v - my) * 0.85).max(0.0)
                * (0.5 + 0.5 * (t * speed + phase).sin())
                * 1.15
                * window;
            let mix = smooth((z - 0.75) / 0.25);
            // Avoid paying for dense formations until the zoom reaches them.
            return if mix == 0.0 {
                dots
            } else {
                dots + (self.formations(fx, fy) - dots) * mix
            };
        }
        let wx = (fx - self.width / 2.0) / self.scale + 0.5;
        let wy = (fy - self.height / 2.0) / self.scale + 0.5;
        let i = wx.floor() as i32;
        let j = wy.floor() as i32;
        let u = (wx - wx.floor() - 0.5) * 2.0;
        let v = (wy - wy.floor() - 0.5) * 2.0;
        let phase = hash(i, j, 0) * TAU;
        let pulse = 0.5 + 0.5 * (self.time * 0.4 + phase).sin();
        let height = if hash(i, j, 4) < 0.18 {
            0.0
        } else {
            (0.45 + 0.25 * hash(i, j, 5))
                * (0.5 + 0.5 * pulse).max(self.lines * (0.85 + 0.15 * pulse))
        };
        let edge = u.abs().max(v.abs());
        let blob = (1.0 - u.hypot(v - 0.25) * 1.8).max(0.0) * (0.35 + 0.65 * pulse);
        let sandbox = if edge > 0.64 && edge < 0.84 {
            blob.max(0.28)
        } else if edge < 0.92 {
            blob.max(0.12)
        } else {
            0.0
        };
        let c = v.clamp(0.18 - height, 0.18);
        let body = (0.46 * u).abs() / 0.38 + (v - c).abs() / 0.34;
        let top = (0.46 * u).abs() / 0.38 + (v - (0.18 - height)).abs() / 0.34;
        let bottom = (0.46 * u).abs() / 0.38 + (v - 0.18).abs() / 0.34;
        let creature = if body <= 1.0 {
            let glow = (1.0 - u.hypot((v - 0.18 + height * 0.22) * 1.5) * 2.3).max(0.0);
            (0.18 + 0.45 * glow * pulse).max(if body > 0.78 || (top - 1.0).abs() < 0.12 {
                0.42
            } else {
                0.0
            })
        } else {
            0.0
        };
        let wire = if (body - 1.0).abs() < 0.085
            || (top - 1.0).abs() < 0.085
            || ((bottom - 1.0).abs() < 0.085 && v >= 0.18)
            || (u.abs() < 0.055 && v > 0.18 - height + 0.34 && v < 0.52)
        {
            0.9
        } else if top < 0.9 && ((u * 0.6 + v) * 12.0).rem_euclid(1.0) < 0.18 {
            0.22
        } else {
            0.0
        };
        let solid = sandbox + (creature - sandbox) * self.shape;
        solid + (wire - solid) * self.lines
    }

    fn formations(&self, x: f64, y: f64) -> f64 {
        let t = self.time;
        let quiet = 0.1
            + 0.3 * (0.5 + 0.5 * (x * 0.01 + y * 0.014 + t * 0.12).sin())
            + 0.2 * noise(x * 0.022, y * 0.013, t * 0.1);
        // Four 16-second formations, each separated by eight seconds of quiet.
        let segment = match self.formation {
            MotionFormation::Cycle => (t / 24.0).floor() as i32,
            MotionFormation::Platoon => 0,
            MotionFormation::Muster => 3,
        };
        let local = t.rem_euclid(24.0) - 8.0;
        let fade = match self.formation {
            MotionFormation::Cycle => smooth(local / 1.6) * smooth((16.0 - local) / 1.6),
            MotionFormation::Platoon | MotionFormation::Muster => 1.0,
        };
        if fade == 0.0 {
            return quiet;
        }
        let formation = match segment.rem_euclid(4) {
            0 => {
                let lane = (y / 22.0).floor() as i32;
                let speed = [-11.0, -5.0, 5.0][lane.rem_euclid(3) as usize];
                let lx = (x - t * speed + f64::from(lane) * 18.0).rem_euclid(36.0);
                let ly = y.rem_euclid(22.0);
                if (2.0..32.0).contains(&lx) && (2.0..18.0).contains(&ly) {
                    if ((ly - 2.0) / 4.0) as i32 % 2 == 0 {
                        0.66
                    } else {
                        0.34
                    }
                } else {
                    quiet * 0.35
                }
            }
            1 => {
                let u = (x * 0.55 + y) / 22.0 + t * 0.03;
                let v = (x * 0.55 - y) / 22.0 - t * 0.13;
                let edge = u
                    .rem_euclid(1.0)
                    .min(1.0 - u.rem_euclid(1.0))
                    .min(v.rem_euclid(1.0).min(1.0 - v.rem_euclid(1.0)));
                if edge < 0.06 {
                    0.66
                } else if (u.floor() as i32 + v.floor() as i32) % 2 == 0 {
                    0.12
                } else {
                    0.46
                }
            }
            2 => {
                let mut value = quiet;
                for k in 0..6 {
                    let angle = f64::from(k) * TAU / 6.0 + t * 0.11;
                    let px = x - self.width * (0.5 + angle.cos() * 0.32);
                    let py = y - self.height * (0.5 + angle.sin() * 0.32);
                    let size = self.width.min(self.height) * 0.06;
                    if (px - size * 0.4).abs().max((py + size * 0.4).abs()) < size
                        || (px + size * 0.4).abs().max((py - size * 0.4).abs()) < size
                    {
                        value = 0.65;
                    }
                }
                value
            }
            _ => {
                let depth = 50.0 + y;
                let row = 14.0 * depth.ln() - t * 0.9;
                let col = (x - self.width / 2.0) * 22.0 / depth;
                if (col.floor() as i32).rem_euclid(7) == 0
                    || (row.floor() as i32).rem_euclid(8) == 0
                {
                    0.05
                } else {
                    0.28 + 0.3
                        * smooth(row.rem_euclid(1.0) * 3.0)
                        * smooth(col.rem_euclid(1.0) * 3.0)
                }
            }
        };
        quiet + (formation - quiet) * fade
    }

    fn eye(&self, x: i32, y: i32) -> Option<char> {
        if self.shape < 0.8 {
            return None;
        }
        let wx = (f64::from(x * 2 + 1) - self.width / 2.0) / self.scale + 0.5;
        let wy = (f64::from(y * 4 + 2) - self.height / 2.0) / self.scale + 0.5;
        let i = wx.floor() as i32;
        let j = wy.floor() as i32;
        if hash(i, j, 4) < 0.18 {
            return None;
        }
        let phase = hash(i, j, 0) * TAU;
        let pulse = 0.5 + 0.5 * (self.time * 0.4 + phase).sin();
        let height = (0.45 + 0.25 * hash(i, j, 5)) * (0.5 + 0.5 * pulse);
        let ey = ((f64::from(j) * self.scale
            + (0.1 - height * 0.22) * self.scale / 2.0
            + self.height / 2.0
            - 2.0)
            / 4.0)
            .round() as i32;
        let look = (self.time * 0.25 + phase).sin().round() * 0.1;
        let left =
            ((f64::from(i) * self.scale + (-0.17 + look) * self.scale / 2.0 + self.width / 2.0
                - 1.0)
                / 2.0)
                .round() as i32;
        let right =
            ((f64::from(i) * self.scale + (0.17 + look) * self.scale / 2.0 + self.width / 2.0
                - 1.0)
                / 2.0)
                .round() as i32;
        if y == ey && (x == left || x == right) {
            Some(
                if (self.time * 0.25 + hash(i, j, 7)).rem_euclid(1.0) < 0.07 {
                    '-'
                } else {
                    'o'
                },
            )
        } else {
            None
        }
    }
}
