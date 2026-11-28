//! The onigiri ball: geometry baked once per size, shaded every frame.
//!
//! Everything about a cell that does not depend on time (coverage, surface
//! normals, which Braille dots are grains and how they tilt) is computed when
//! the ball is placed. A frame then costs a few dot products per cell. Only
//! the 490 ms while the ball is squashed by the press are computed live.

use std::f32::consts::TAU;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;

use super::color::Mat;
use super::color::Palette;
use super::color::Tier;
use super::geometry::Geo;
use super::geometry::Place;
use super::geometry::coverage;
use super::geometry::press;
use super::grains::Grains;
use super::lighting::Light;
use super::lighting::V3;
use super::lighting::nori_shade;
use super::lighting::norm;
use super::lighting::normal;
use super::lighting::shade;
use super::shape::BRAILLE_BITS;
use super::shape::QUAD;
use super::shape::braille;
use super::shape::ease_out;
use super::shape::hash;
use super::shape::sd_ball;
use super::shape::smooth;
use super::timeline::NORI_AT;
use super::timeline::NORI_DUR;
use super::timeline::PRESS_AT;
use super::timeline::PRESS_BOUNCE;
use super::timeline::PRESS_DOWN;
use super::timeline::PRESS_FRONT;

const GRAIN_DENSITY: f32 = 0.50;
/// A grain starts to glint once it is this much brighter than the rice
/// around it.
const GLINT_AT: f32 = 0.06;
/// A grain starts to show as a pit once it is this much darker.
const PIT_AT: f32 = 0.08;
/// Brightness margin over which a grain fades from invisible to full. Full
/// strength lands where grains used to switch on outright, so the texture
/// reads as strongly as before. ANSI 16 cannot fade, so there grains only
/// show at full strength.
const GRAIN_FADE: f32 = 0.14;

/// Where the cell grid starts and the paint target for one frame.
pub struct Paint<'a> {
    pub buf: &'a mut Buffer,
    pub pal: &'a Palette,
    /// The area being rendered, within the buffer; nothing is drawn outside
    /// it.
    pub clip: Rect,
}

impl Paint<'_> {
    /// The cell (`x`, `y`) if it lies inside the clip area.
    fn inside(&self, x: i32, y: i32) -> Option<(u16, u16)> {
        let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
            return None;
        };
        self.clip.contains((x, y).into()).then_some((x, y))
    }

    /// Draw `s` starting at cell (`x`, `y`), clipped to the area.
    pub fn put(&mut self, x: i32, y: i32, s: &str, style: Style) {
        if let Some((x, y)) = self.inside(x, y) {
            let room = usize::from(self.clip.right() - x);
            self.buf.set_stringn(x, y, s, room, style);
        }
    }

    /// Draw one single-width glyph at cell (`x`, `y`), clipped to the area.
    pub fn glyph(&mut self, x: i32, y: i32, s: &'static str, style: Style) {
        if let Some(c) = self.inside(x, y).and_then(|at| self.buf.cell_mut(at)) {
            c.set_symbol(s).set_style(style);
        }
    }

    /// Set only the background of cell (`x`, `y`), clipped to the area.
    pub fn tint(&mut self, x: i32, y: i32, bg: Color) {
        if let Some(c) = self.inside(x, y).and_then(|at| self.buf.cell_mut(at)) {
            c.set_bg(bg);
        }
    }

    pub fn tier(&self) -> Tier {
        self.pal.tier
    }

    fn c(&self, m: Mat, b: f32) -> Color {
        self.pal.color(m, b)
    }
}

/// One Braille dot that belongs to a grain, and the grain's surface normal.
#[derive(Debug, Clone, Copy, Default)]
struct Grain {
    bit: i32,
    n: V3,
}

/// The time-independent part of a rice cell.
#[derive(Debug, Clone, Copy)]
enum Rice {
    /// Partly covered: a quadrant glyph in the rice colour.
    Edge { mask: i32, n: V3 },
    /// Fully covered on a ball too small for texture.
    Smooth { n: V3 },
    /// Fully covered with grain texture. `top`/`bot` shade the two halves
    /// when no grain shows.
    Textured {
        n: V3,
        top: V3,
        bot: V3,
        grains: [Grain; 8],
        count: i32,
    },
}

impl Rice {
    fn at(g: &Geo, cx: i32, cy: i32, squash: f32, texture: bool) -> Option<Rice> {
        let mask = coverage(g, cx, cy, squash);
        if mask == 0 {
            return None;
        }
        let at = |dy: f32| {
            let (u, v) = g.local(cx as f32 + 0.5, 2.0 * cy as f32 + dy, squash);
            normal(u, v)
        };
        let n = at(1.0);
        if mask != 15 {
            return Some(Rice::Edge { mask, n });
        }
        if !texture {
            return Some(Rice::Smooth { n });
        }
        let mut grains = [Grain::default(); 8];
        let mut count = 0;
        let (lx, ly) = (g.cols.0 * 2, g.rows.0 * 4);
        for (r, row) in BRAILLE_BITS.iter().enumerate() {
            for (c, bit) in row.iter().enumerate() {
                let bx = cx * 2 + c as i32 - lx;
                let by = cy * 4 + r as i32 - ly;
                // Grains are staggered pairs of dots sharing one tilt.
                let id = (bx, (by + (bx & 1)) >> 1);
                if hash(id.0 * 3 + 11, id.1 * 7 + 5) > GRAIN_DENSITY {
                    continue;
                }
                let a = hash(id.0 + 101, id.1 - 33) * TAU;
                let tilt = 0.55 + 0.35 * hash(id.0 - 9, id.1 + 71);
                grains[count as usize] = Grain {
                    bit: *bit,
                    n: norm([n[0] + tilt * a.cos(), n[1] + tilt * a.sin(), n[2]]),
                };
                count += 1;
            }
        }
        Some(Rice::Textured {
            n,
            top: at(0.5),
            bot: at(1.5),
            grains,
            count,
        })
    }

    fn draw(&self, p: &mut Paint, cx: i32, cy: i32, light: &Light) {
        match *self {
            Rice::Edge { mask, n } => {
                let fg = p.c(Mat::Rice, shade(n, light));
                p.glyph(cx, cy, QUAD[mask as usize], Style::default().fg(fg));
            }
            Rice::Smooth { n } => {
                let bg = p.c(Mat::Rice, shade(n, light));
                p.glyph(cx, cy, " ", Style::default().bg(bg));
            }
            Rice::Textured {
                n,
                top,
                bot,
                grains,
                count,
            } => {
                let b = shade(n, light);
                let bg = p.c(Mat::Rice, b);
                let fades = p.tier() != Tier::Ansi16;
                let (glint_at, pit_at) = if fades {
                    (GLINT_AT, PIT_AT)
                } else {
                    (GLINT_AT + GRAIN_FADE, f32::INFINITY)
                };
                let (mut glint, mut pit) = (0, 0);
                let (mut glint_k, mut pit_k) = (0.0f32, 0.0f32);
                for grain in &grains[..count as usize] {
                    let d = shade(grain.n, light) - b;
                    if d > glint_at {
                        glint |= grain.bit;
                        glint_k = glint_k.max(smooth((d - GLINT_AT) / GRAIN_FADE));
                    } else if -d > pit_at {
                        pit |= grain.bit;
                        pit_k = pit_k.max(smooth((-d - PIT_AT) / GRAIN_FADE));
                    }
                }
                let (glints, pitted) = (glint.count_ones(), pit.count_ones());
                if glints >= pitted && glint != 0 {
                    let to = (b + 0.35).min(1.0);
                    let fg = p.c(Mat::Glint, b + (to - b) * glint_k);
                    p.glyph(cx, cy, braille(glint), Style::default().fg(fg).bg(bg));
                } else if pit != 0 {
                    let to = (b - 0.20).max(0.0);
                    let fg = p.c(Mat::Pit, b + (to - b) * pit_k);
                    p.glyph(cx, cy, braille(pit), Style::default().fg(fg).bg(bg));
                } else {
                    // No grain here: split the cell so shading gets two rows
                    // of detail.
                    let top = p.c(Mat::Rice, shade(top, light));
                    let bot = p.c(Mat::Rice, shade(bot, light));
                    p.glyph(cx, cy, "▀", Style::default().fg(top).bg(bot));
                }
            }
        }
    }
}

/// The time-independent part of a nori band cell.
#[derive(Debug, Clone, Copy)]
enum Nori {
    Edge { mask: i32, n: V3 },
    Full { top: V3, bot: V3 },
}

impl Nori {
    fn at(g: &Geo, cx: i32, cy: i32) -> Option<Nori> {
        let mask = coverage(g, cx, cy, 0.0);
        let at = |dy: f32| {
            let (u, v) = g.local(cx as f32 + 0.5, 2.0 * cy as f32 + dy, 0.0);
            normal(u, v)
        };
        match mask {
            0 => None,
            15 => Some(Nori::Full {
                top: at(0.5),
                bot: at(1.5),
            }),
            mask => Some(Nori::Edge { mask, n: at(1.0) }),
        }
    }

    fn draw(&self, p: &mut Paint, cx: i32, cy: i32, light: &Light) {
        match *self {
            Nori::Full { top, bot } => {
                let fg = p.c(Mat::Nori, nori_shade(top, light));
                let bg = p.c(Mat::Nori, nori_shade(bot, light));
                p.glyph(cx, cy, "▀", Style::default().fg(fg).bg(bg));
            }
            Nori::Edge { mask, n } => {
                let fg = p.c(Mat::Nori, nori_shade(n, light));
                p.glyph(cx, cy, QUAD[mask as usize], Style::default().fg(fg));
            }
        }
    }
}

struct Cell {
    x: i32,
    y: i32,
    /// The rice here once the ball has settled; the box includes empty
    /// cells.
    rice: Option<Rice>,
    /// The band here; only cells in the band's columns have one.
    nori: Option<Nori>,
}

/// A placed ball with its geometry baked.
pub struct Ball {
    g: Geo,
    texture: bool,
    cells: Vec<Cell>,
    grains: Grains,
}

/// The press front: how far out from the centre the pressed rice reaches.
fn front(t: f32) -> f32 {
    if t >= PRESS_AT {
        0.25 + 0.85 * ease_out((t - PRESS_AT) / PRESS_FRONT)
    } else {
        -1.0
    }
}

/// When the press front and the squash stop changing.
const PRESS_DONE: f32 = PRESS_AT + PRESS_DOWN + PRESS_BOUNCE;

impl Ball {
    /// `flies` bakes the intro's flying grains, which only balls that play
    /// the intro need.
    pub fn new(place: Place, texture: bool, flies: bool) -> Ball {
        let g = Geo::new(place);
        let settled = front(PRESS_DONE);
        let mut cells = Vec::new();
        for cy in g.rows.0..g.rows.1 {
            for cx in g.cols.0..g.cols.1 {
                let rice = if pressed(&g, cx, cy, settled) {
                    Rice::at(&g, cx, cy, 0.0, texture)
                } else {
                    None
                };
                let nori = if g.in_band_cols(cx) && cy >= g.band_top {
                    Nori::at(&g, cx, cy)
                } else {
                    None
                };
                // Every cell in the box is kept: the squashed ball during the
                // press can cover cells the settled ball does not.
                cells.push(Cell {
                    x: cx,
                    y: cy,
                    rice,
                    nori,
                });
            }
        }
        Ball {
            g,
            texture,
            cells,
            grains: if flies {
                Grains::new(&g)
            } else {
                Grains::none()
            },
        }
    }

    /// Draw the ball at time `t`. `intro` plays gather/press/wrap;
    /// otherwise the ball is settled.
    pub fn draw(&mut self, p: &mut Paint, t: f32, intro: bool, light: &Light) {
        let g = self.g;
        let t_shape = if intro { t } else { 1.0e6 };
        let front = front(t_shape);
        let steps =
            ((t_shape - NORI_AT) / NORI_DUR * (g.rows.1 - 1 - g.band_top) as f32).ceil() as i32;
        let band_top = (g.rows.1 - 1 - steps.max(0)).max(g.band_top);
        let banded = t_shape >= NORI_AT;

        if front < 1.09 {
            let tier = p.tier();
            let landed = match tier {
                Tier::Ansi16 => Style::default(),
                Tier::True | Tier::X256 => Style::default().fg(p.c(Mat::Rice, 0.80)),
            };
            let flying = match tier {
                Tier::Ansi16 => Style::default().add_modifier(Modifier::DIM),
                Tier::True | Tier::X256 => Style::default().fg(p.c(Mat::Rice, 0.42)),
            };
            for (cx, cy, bits, settled) in self.grains.at(t_shape) {
                if cy >= g.rows.1 + 2 || pressed(&g, cx, cy, front) {
                    continue;
                }
                p.glyph(cx, cy, braille(bits), if settled { landed } else { flying });
            }
        }
        if front < 0.0 {
            return;
        }
        let live = t_shape < PRESS_DONE;
        let squash = press(t_shape);
        for cell in &self.cells {
            let (cx, cy) = (cell.x, cell.y);
            if banded && g.in_band_cols(cx) && cy >= band_top {
                if let Some(nori) = cell.nori {
                    nori.draw(p, cx, cy, light);
                }
            } else if live {
                if pressed(&g, cx, cy, front)
                    && let Some(rice) = Rice::at(&g, cx, cy, squash, self.texture)
                {
                    rice.draw(p, cx, cy, light);
                }
            } else if let Some(rice) = cell.rice {
                rice.draw(p, cx, cy, light);
            }
        }
    }
}

/// Whether the press front has reached the centre of cell (`cx`, `cy`).
fn pressed(g: &Geo, cx: i32, cy: i32, front: f32) -> bool {
    let (u, v) = g.local(cx as f32 + 0.5, 2.0 * cy as f32 + 1.0, 0.0);
    front > 0.0 && sd_ball(u / front.max(0.01), v / front.max(0.01)) < 0.0
}
