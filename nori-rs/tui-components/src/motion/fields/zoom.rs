//! Dot islands and formation fields for the zoom.

use std::f64::consts::TAU;

use super::super::MotionFormation;
use super::Field;
use super::math::hash;
use super::math::noise;
use super::math::smooth;

impl Field {
    pub(super) fn zoom(&self, fx: f64, fy: f64) -> f64 {
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
        if mix == 0.0 {
            dots
        } else {
            dots + (self.formations(fx, fy) - dots) * mix
        }
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
}
