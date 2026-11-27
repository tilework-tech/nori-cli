//! Colour tiers. Every surface is computed once as a material plus a
//! brightness in 0..1, then mapped per tier: truecolor interpolates a palette,
//! 256 picks the nearest xterm-256 entry of that same RGB, and ANSI 16 uses
//! hand-picked thresholds so it reads on any theme.

use ratatui::style::Color;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    True,
    X256,
    Ansi16,
}

impl Tier {
    pub fn detect() -> Tier {
        let colorterm = std::env::var("COLORTERM").unwrap_or_default();
        let term = std::env::var("TERM").unwrap_or_default();
        if colorterm == "truecolor" || colorterm == "24bit" {
            Tier::True
        } else if term.contains("256color") {
            Tier::X256
        } else {
            Tier::Ansi16
        }
    }

    pub fn parse(s: &str) -> Tier {
        match s {
            "true" | "truecolor" | "24bit" => Tier::True,
            "256" => Tier::X256,
            "16" | "ansi" => Tier::Ansi16,
            other => panic!("unknown colour tier {other}"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mat {
    /// Rice body. Brightness 0 = core shadow, 1 = specular.
    Rice,
    /// Grain glint drawn over rice.
    Glint,
    /// Grain pit drawn over rice in shadow.
    Pit,
    /// Nori band.
    Nori,
    /// Brand accent.
    Green,
}

type Rgb = (u8, u8, u8);

/// Rice ramp, cool greys from the Nori palette up to paper white.
const RICE: [Rgb; 5] = [
    (0x52, 0x56, 0x5e),
    (0x8a, 0x8f, 0x98),
    (0xc1, 0xc7, 0xcd),
    (0xe4, 0xe8, 0xed),
    (0xff, 0xff, 0xff),
];
/// Nori ramp, the verdant greens from the palette.
const NORI: [Rgb; 4] = [(0x10, 0x16, 0x12), (0x17, 0x25, 0x1b), (0x1b, 0x3a, 0x27), (0x2c, 0x6b, 0x41)];
const GREEN: Rgb = (0x42, 0xbe, 0x65);

fn ramp(stops: &[Rgb], b: f32) -> Rgb {
    let b = b.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let i = (b.floor() as usize).min(stops.len() - 2);
    let f = b - i as f32;
    let l = |a: u8, c: u8| (a as f32 + (c as f32 - a as f32) * f).round() as u8;
    let (a, c) = (stops[i], stops[i + 1]);
    (l(a.0, c.0), l(a.1, c.1), l(a.2, c.2))
}

fn rgb(m: Mat, b: f32) -> Rgb {
    match m {
        Mat::Rice | Mat::Glint | Mat::Pit => ramp(&RICE, b),
        Mat::Nori => ramp(&NORI, b),
        Mat::Green => GREEN,
    }
}

/// Nearest xterm-256 colour (6x6x6 cube or the 24-step grey ramp).
fn to256((r, g, b): Rgb) -> u8 {
    const LV: [i32; 6] = [0, 95, 135, 175, 215, 255];
    let near = |c: u8| (0..6).min_by_key(|&i| (LV[i] - c as i32).abs()).unwrap_or(0);
    let (ci, cj, ck) = (near(r), near(g), near(b));
    let cube = (LV[ci], LV[cj], LV[ck]);
    let avg = (r as i32 + g as i32 + b as i32) / 3;
    let gi = ((avg - 8 + 5) / 10).clamp(0, 23);
    let gv = 8 + gi * 10;
    let d = |(x, y, z): (i32, i32, i32)| {
        (x - r as i32).pow(2) + (y - g as i32).pow(2) + (z - b as i32).pow(2)
    };
    if d((gv, gv, gv)) < d(cube) {
        232 + gi as u8
    } else {
        16 + 36 * ci as u8 + 6 * cj as u8 + ck as u8
    }
}

fn ansi16(m: Mat, b: f32) -> Color {
    match m {
        Mat::Rice => {
            if b > 0.86 {
                Color::White
            } else if b > 0.24 {
                Color::Gray
            } else {
                Color::DarkGray
            }
        }
        Mat::Glint => Color::White,
        Mat::Pit => Color::DarkGray,
        Mat::Nori => Color::DarkGray,
        Mat::Green => Color::Green,
    }
}

pub fn color(tier: Tier, m: Mat, b: f32) -> Color {
    match tier {
        Tier::True => {
            let (r, g, bl) = rgb(m, b);
            Color::Rgb(r, g, bl)
        }
        // Nearest-match drops nori to plain grey; hand-pick a dark green ramp.
        Tier::X256 if m == Mat::Nori => Color::Indexed(match b {
            b if b < 0.30 => 233,
            b if b < 0.45 => 234,
            b if b < 0.62 => 236,
            _ => 22,
        }),
        Tier::X256 => Color::Indexed(to256(rgb(m, b))),
        Tier::Ansi16 => ansi16(m, b),
    }
}
