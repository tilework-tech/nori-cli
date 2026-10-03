//! Dot islands and formation fields for the zoom.

use std::f64::consts::TAU;

use super::Field;
use super::math::hash;
use super::math::smooth;

#[derive(Clone, Copy)]
pub(super) struct Island {
    x: i32,
    y: i32,
    drift_x: f64,
    drift_y: f64,
    pulse: f64,
}

#[derive(Clone, Copy)]
pub(super) struct MusterRow {
    y: f64,
    depth: f64,
    row: f64,
}

impl Field {
    pub(super) fn zoom(&mut self, fx: f64, fy: f64) -> f64 {
        let mix = self.zoom_mix;
        if mix == 1.0 {
            return self.formations(fx, fy);
        }
        let scale = if self.phase >= 1.0 { 1.0 } else { self.scale };
        let wx = (fx - self.width / 2.0) / scale + self.time * 0.02;
        let wy = (fy - self.height / 2.0) / scale + self.time * 0.008;
        let ix = wx.floor() as i32;
        let iy = wy.floor() as i32;
        let slot = (ix.rem_euclid(16) + iy.rem_euclid(4) * 16) as usize;
        let island = match self.islands[slot] {
            Some(island) if island.x == ix && island.y == iy => island,
            _ => {
                let phase = hash(ix, iy, 0) * TAU;
                let speed = 0.5 + hash(ix, iy, 1) * 0.9;
                let t = self.time * 0.5;
                let island = Island {
                    x: ix,
                    y: iy,
                    drift_x: 0.26 * (t * 0.43 * speed + phase).sin(),
                    drift_y: 0.26 * (t * 0.31 * speed + phase * 1.7).cos(),
                    pulse: 0.5 + 0.5 * (t * speed + phase).sin(),
                };
                self.islands[slot] = Some(island);
                island
            }
        };
        let u = (wx - wx.floor() - 0.5) * 2.0;
        let v = (wy - wy.floor() - 0.5) * 2.0;
        let window = ((1.0 - u.abs().max(v.abs())) * 2.2).clamp(0.0, 1.0);
        let dots = (1.0 - (u - island.drift_x).hypot(v - island.drift_y) * 0.85).max(0.0)
            * island.pulse
            * 1.15
            * window;
        // Avoid paying for dense formations until the zoom reaches them.
        if mix == 0.0 {
            dots
        } else {
            dots + (self.formations(fx, fy) - dots) * mix
        }
    }

    fn quiet(&mut self, x: f64, y: f64) -> f64 {
        0.1 + 0.3 * (0.5 + 0.5 * (x * 0.01 + y * 0.014 + self.time * 0.12).sin())
            + 0.2 * self.noise.sample(x * 0.022, y * 0.013)
    }

    fn formations(&mut self, x: f64, y: f64) -> f64 {
        let t = self.time;
        let fade = self.formation_fade;
        if fade == 0.0 {
            return self.quiet(x, y);
        }
        let mut quiet = None;
        let formation = match self.formation_segment {
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
                    let value = self.quiet(x, y);
                    quiet = Some(value);
                    value * 0.35
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
                let mut occupied = false;
                for k in 0..6 {
                    let angle = f64::from(k) * TAU / 6.0 + t * 0.11;
                    let px = x - self.width * (0.5 + angle.cos() * 0.32);
                    let py = y - self.height * (0.5 + angle.sin() * 0.32);
                    let size = self.width.min(self.height) * 0.06;
                    if (px - size * 0.4).abs().max((py + size * 0.4).abs()) < size
                        || (px + size * 0.4).abs().max((py - size * 0.4).abs()) < size
                    {
                        occupied = true;
                    }
                }
                if occupied {
                    0.65
                } else {
                    let value = self.quiet(x, y);
                    quiet = Some(value);
                    value
                }
            }
            _ => {
                let slot = (y as i32).rem_euclid(4) as usize;
                let cached = match self.muster_rows[slot] {
                    Some(cached) if cached.y == y => cached,
                    _ => {
                        let depth = 50.0 + y;
                        let cached = MusterRow {
                            y,
                            depth,
                            row: 14.0 * depth.ln() - t * 0.9,
                        };
                        self.muster_rows[slot] = Some(cached);
                        cached
                    }
                };
                let depth = cached.depth;
                let row = cached.row;
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
        // An opaque pattern does not need its invisible ambient field.
        if fade == 1.0 {
            formation
        } else {
            let quiet = quiet.unwrap_or_else(|| self.quiet(x, y));
            quiet + (formation - quiet) * fade
        }
    }
}
