//! Sandbox, creature, and blueprint geometry.

use std::f64::consts::TAU;

use super::Field;
use super::math::hash;

impl Field {
    pub(super) fn tiles(&self, fx: f64, fy: f64) -> f64 {
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

    pub(super) fn eye(&self, x: i32, y: i32) -> Option<char> {
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
