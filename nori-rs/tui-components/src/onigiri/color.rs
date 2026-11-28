//! Colour tiers. Every surface is computed once as a material plus a
//! brightness in 0..1, then mapped per tier: truecolor interpolates a palette,
//! 256 picks the nearest xterm-256 entry of that same RGB, and ANSI 16 uses
//! hand-picked thresholds so it reads on any theme.

use ratatui::style::Color;

/// How many colours the terminal can show.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    True,
    X256,
    Ansi16,
}

impl Tier {
    /// Guess from `COLORTERM` and `TERM`.
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

    pub fn parse(s: &str) -> Option<Tier> {
        match s {
            "true" | "truecolor" | "24bit" => Some(Tier::True),
            "256" => Some(Tier::X256),
            "16" | "ansi" => Some(Tier::Ansi16),
            _ => None,
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
    /// Wordmark neon tube. Brightness 0 = unlit glass, 1 = fully lit.
    Tube,
    /// Wordmark at rest (0) up through the shimmer's green edge to its white
    /// core (1).
    Shine,
    /// Background glow around lit wordmark strokes. 0 = none.
    Glow,
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
const NORI: [Rgb; 4] = [
    (0x10, 0x16, 0x12),
    (0x17, 0x25, 0x1b),
    (0x1b, 0x3a, 0x27),
    (0x2c, 0x6b, 0x41),
];
const GREEN: Rgb = (0x42, 0xbe, 0x65);
const TUBE: [Rgb; 3] = [(0x2a, 0x33, 0x2e), (0x6f, 0xa8, 0x82), (0xdc, 0xff, 0xe6)];
const SHINE: [Rgb; 3] = [(0xb4, 0xba, 0xc2), (0x8c, 0xe6, 0xa8), (0xf6, 0xff, 0xf8)];
/// Glow fades from the dark background the welcome is designed on.
const GLOW: [Rgb; 2] = [(0x16, 0x18, 0x1c), (0x1e, 0x6e, 0x40)];

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
        Mat::Tube => ramp(&TUBE, b),
        Mat::Shine => ramp(&SHINE, b),
        Mat::Glow => ramp(&GLOW, b),
    }
}

/// Nearest xterm-256 colour (6x6x6 cube or the 24-step grey ramp).
fn to256((r, g, b): Rgb) -> u8 {
    const LV: [i32; 6] = [0, 95, 135, 175, 215, 255];
    let near = |c: u8| {
        (0..6)
            .min_by_key(|&i| (LV[i] - c as i32).abs())
            .unwrap_or(0)
    };
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
        Mat::Tube => {
            if b > 0.8 {
                Color::White
            } else if b > 0.3 {
                Color::Gray
            } else {
                Color::DarkGray
            }
        }
        // The resting wordmark keeps the theme's own foreground.
        Mat::Shine if b < 0.25 => Color::Reset,
        Mat::Shine => Color::Green,
        // Sixteen colours have no shade dark enough to glow with.
        Mat::Glow => Color::Reset,
    }
}

/// Shade steps per material in truecolor and 256 colours. Colours closer
/// than this repaint nothing, which keeps the light loop's output small.
const LEVELS: usize = 64;

/// Colours for one tier, looked up by material and brightness.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    pub tier: Tier,
    rice: [Color; LEVELS],
    nori: [Color; LEVELS],
    tube: [Color; LEVELS],
    shine: [Color; LEVELS],
    glow: [Color; LEVELS],
}

impl Palette {
    pub fn new(tier: Tier) -> Palette {
        let ramp = |m: Mat| std::array::from_fn(|i| color(tier, m, i as f32 / (LEVELS - 1) as f32));
        Palette {
            tier,
            rice: ramp(Mat::Rice),
            nori: ramp(Mat::Nori),
            tube: ramp(Mat::Tube),
            shine: ramp(Mat::Shine),
            glow: ramp(Mat::Glow),
        }
    }

    pub fn color(&self, m: Mat, b: f32) -> Color {
        let i = (b.clamp(0.0, 1.0) * (LEVELS - 1) as f32).round() as usize;
        match (self.tier, m) {
            (Tier::Ansi16, m) => ansi16(m, b),
            (Tier::True | Tier::X256, Mat::Rice | Mat::Glint | Mat::Pit) => self.rice[i],
            (Tier::True | Tier::X256, Mat::Nori) => self.nori[i],
            (Tier::True | Tier::X256, Mat::Tube) => self.tube[i],
            (Tier::True | Tier::X256, Mat::Shine) => self.shine[i],
            (Tier::True | Tier::X256, Mat::Glow) => self.glow[i],
            (Tier::True | Tier::X256, Mat::Green) => color(self.tier, m, b),
        }
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "the welcome art is drawn in its own palette at every colour depth the terminal supports"
)]
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
        // Nearest-match turns the glow grey too; step it through dark greens.
        Tier::X256 if m == Mat::Glow => Color::Indexed(match b {
            b if b < 0.15 => 233,
            b if b < 0.42 => 234,
            _ => 22,
        }),
        Tier::X256 => Color::Indexed(to256(rgb(m, b))),
        Tier::Ansi16 => ansi16(m, b),
    }
}
