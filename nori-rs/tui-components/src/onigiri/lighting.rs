//! The rice surface and how one light shades it.
//!
//! Rice is shaded as a puffy dome under one light that circles the ball
//! after the intro. Each Braille dot may belong to a rice grain with its own
//! tilt; grains that tilt toward the light glint and grains in shadow show as
//! pits, so the texture moves with the light while the shape stays still.

use std::f32::consts::PI;
use std::f32::consts::TAU;

use super::shape::clamp01;
use super::shape::sd_ball;
use super::timeline::LIGHT_AT;
use super::timeline::LIGHT_PERIOD;
use super::timeline::LIGHT_RAMP;
use super::timeline::LIGHT_TICK;

/// Resting light: upper left.
const LIGHT_REST: f32 = -0.75 * PI;
const LIGHT_ELEV: f32 = 0.70;

pub type V3 = [f32; 3];

/// Light direction in screen space (x right, y down, z toward the viewer),
/// and the half vector toward the viewer for highlights.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    pub l: V3,
    pub h: V3,
}

impl Light {
    pub fn at(t: f32, animate: bool) -> Light {
        let t = (t / LIGHT_TICK).floor() * LIGHT_TICK;
        let tau = if animate {
            (t - LIGHT_AT).max(0.0)
        } else {
            0.0
        };
        // Constant acceleration over the ramp, then constant speed.
        let w = TAU / LIGHT_PERIOD;
        let phase = if tau < LIGHT_RAMP {
            w * tau * tau / (2.0 * LIGHT_RAMP)
        } else {
            w * (tau - LIGHT_RAMP / 2.0)
        };
        let th = LIGHT_REST + phase;
        let (ce, se) = (LIGHT_ELEV.cos(), LIGHT_ELEV.sin());
        let l = [ce * th.cos(), ce * th.sin(), se];
        Light {
            l,
            h: norm([l[0], l[1], l[2] + 1.0]),
        }
    }
}

pub fn norm(v: V3) -> V3 {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / l, v[1] / l, v[2] / l]
}

pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Height of the rice surface: a rounded rim falling off at the outline,
/// on top of a gentle dome.
fn pillow(u: f32, v: f32) -> f32 {
    // Rounded profile from the outline to the deepest interior point, so the
    // whole body is one surface.
    const DEPTH: f32 = 0.36;
    let s = clamp01(-sd_ball(u, v) / DEPTH);
    (1.0 - (1.0 - s) * (1.0 - s)).sqrt() * DEPTH * 0.8
}

/// The pillow blurred a little so the triangle's medial creases soften.
fn height(u: f32, v: f32) -> f32 {
    const R: f32 = 0.045;
    const TAPS: [(f32, f32); 7] = [
        (0.0, 0.0),
        (1.0, 0.0),
        (-1.0, 0.0),
        (0.5, 0.87),
        (-0.5, 0.87),
        (0.5, -0.87),
        (-0.5, -0.87),
    ];
    TAPS.iter()
        .map(|(a, b)| pillow(u + a * R, v + b * R))
        .sum::<f32>()
        / TAPS.len() as f32
}

pub fn normal(u: f32, v: f32) -> V3 {
    let e = 0.006;
    let dx = (height(u + e, v) - height(u - e, v)) / (2.0 * e);
    let dy = (height(u, v + e) - height(u, v - e)) / (2.0 * e);
    norm([-dx, -dy, 1.0])
}

/// Brightness of rice with normal `n`.
pub fn shade(n: V3, light: &Light) -> f32 {
    let diffuse = dot(n, light.l).max(0.0);
    let spec = dot(n, light.h).max(0.0).powi(48);
    0.20 + 0.66 * diffuse + 0.32 * spec
}

/// Brightness of the nori sheet, which follows the rice but is only faintly
/// glossy.
pub fn nori_shade(n: V3, light: &Light) -> f32 {
    let gloss = dot(n, light.h).max(0.0).powi(16);
    0.32 + 0.20 * (dot(n, light.l) - 0.6) + 0.35 * gloss
}
