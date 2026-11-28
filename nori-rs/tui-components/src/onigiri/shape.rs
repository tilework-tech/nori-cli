//! Signed distance fields and small math helpers.

pub const BRAILLE_BITS: [[i32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
pub const QUAD: [&str; 16] = [
    " ", "▘", "▝", "▀", "▖", "▌", "▞", "▛", "▗", "▚", "▐", "▜", "▄", "▙", "▟", "█",
];
/// One half cell is this many cell widths tall (cells are about 2.3:1).
pub const ASP: f32 = 1.16;

const R_TRI: f32 = 0.37;
const RAD: f32 = 0.19;
const PUFF: f32 = 0.14;
/// Flat base so the ball sits on its baseline.
pub const BASE_V: f32 = 0.39;

pub fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

pub fn clamp01(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}

pub fn smooth(t: f32) -> f32 {
    let t = clamp01(t);
    t * t * (3.0 - 2.0 * t)
}

pub fn ease_out(t: f32) -> f32 {
    let t = 1.0 - clamp01(t);
    1.0 - t * t * t
}

/// All 256 Braille patterns in bit order; each is three bytes of UTF-8.
const BRAILLE: &str = "⠀⠁⠂⠃⠄⠅⠆⠇⠈⠉⠊⠋⠌⠍⠎⠏⠐⠑⠒⠓⠔⠕⠖⠗⠘⠙⠚⠛⠜⠝⠞⠟⠠⠡⠢⠣⠤⠥⠦⠧⠨⠩⠪⠫⠬⠭⠮⠯⠰⠱⠲⠳⠴⠵⠶⠷⠸⠹⠺⠻⠼⠽⠾⠿⡀⡁⡂⡃⡄⡅⡆⡇⡈⡉⡊⡋⡌⡍⡎⡏⡐⡑⡒⡓⡔⡕⡖⡗⡘⡙⡚⡛⡜⡝⡞⡟⡠⡡⡢⡣⡤⡥⡦⡧⡨⡩⡪⡫⡬⡭⡮⡯⡰⡱⡲⡳⡴⡵⡶⡷⡸⡹⡺⡻⡼⡽⡾⡿⢀⢁⢂⢃⢄⢅⢆⢇⢈⢉⢊⢋⢌⢍⢎⢏⢐⢑⢒⢓⢔⢕⢖⢗⢘⢙⢚⢛⢜⢝⢞⢟⢠⢡⢢⢣⢤⢥⢦⢧⢨⢩⢪⢫⢬⢭⢮⢯⢰⢱⢲⢳⢴⢵⢶⢷⢸⢹⢺⢻⢼⢽⢾⢿⣀⣁⣂⣃⣄⣅⣆⣇⣈⣉⣊⣋⣌⣍⣎⣏⣐⣑⣒⣓⣔⣕⣖⣗⣘⣙⣚⣛⣜⣝⣞⣟⣠⣡⣢⣣⣤⣥⣦⣧⣨⣩⣪⣫⣬⣭⣮⣯⣰⣱⣲⣳⣴⣵⣶⣷⣸⣹⣺⣻⣼⣽⣾⣿";

/// The Braille glyph with the given dots (bit order as BRAILLE_BITS).
pub fn braille(bits: i32) -> &'static str {
    let i = (bits & 0xff) as usize * 3;
    &BRAILLE[i..i + 3]
}

/// Equilateral triangle, point up, y up (Inigo Quilez).
fn sd_tri(mut px: f32, mut py: f32, r: f32) -> f32 {
    let k = 3f32.sqrt();
    px = px.abs() - r;
    py += r / k;
    if px + k * py > 0.0 {
        let (nx, ny) = ((px - k * py) / 2.0, (-k * px - py) / 2.0);
        px = nx;
        py = ny;
    }
    px -= px.clamp(-2.0 * r, 0.0);
    -(px * px + py * py).sqrt() * py.signum()
}

/// Rounded, slightly puffy triangle. u right, v down, origin at the
/// triangle's centroid, in units of the ball size.
pub fn sd_ball(u: f32, v: f32) -> f32 {
    let dt = sd_tri(u, -v, R_TRI) - RAD;
    let dc = (u * u + (v - 0.04).powi(2)).sqrt() - 0.47;
    (dt * (1.0 - PUFF) + dc * PUFF).max(v - BASE_V)
}

/// Vertical extent of the unit ball, (top, bottom) in v.
pub fn ball_extent() -> (f32, f32) {
    use std::sync::OnceLock;
    static E: OnceLock<(f32, f32)> = OnceLock::new();
    *E.get_or_init(|| {
        let (mut lo, mut hi) = (1.0f32, -1.0f32);
        for j in 0..400 {
            let v = -1.0 + j as f32 / 200.0;
            if (0..200).any(|i| sd_ball(-0.7 + i as f32 / 142.0, v) < 0.0) {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        (lo, hi)
    })
}
