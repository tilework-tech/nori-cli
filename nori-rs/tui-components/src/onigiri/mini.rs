//! The hand-pixelled onigiri for terminals too short for the drawn ball:
//! 8 by 6 pixels in half blocks, so 8 columns by 3 rows.

use ratatui::style::Style;

use super::ball::Paint;
use super::color::Mat;

pub(crate) const WIDTH: i32 = 8;
pub(crate) const HEIGHT: i32 = 3;

/// `h` highlit rice, `r` rice, `n` nori, `.` empty.
const PIXELS: [&str; 6] = [
    "...hh...", //
    "..hhrr..", //
    ".hrrrrr.", //
    "rrnnnnrr", //
    "rrnnnnrr", //
    ".rnnnnr.", //
];

fn pixel(x: usize, y: usize) -> Option<Mat> {
    match PIXELS[y].as_bytes()[x] {
        b'h' => Some(Mat::Glint),
        b'r' => Some(Mat::Rice),
        b'n' => Some(Mat::Nori),
        _ => None,
    }
}

/// Draw the mini onigiri with its top-left cell at (`x`, `y`).
pub(crate) fn draw(p: &mut Paint, x: i32, y: i32) {
    let shade = |m: Mat| match m {
        Mat::Glint => p.pal.color(Mat::Rice, 1.0),
        Mat::Rice => p.pal.color(Mat::Rice, 0.7),
        _ => p.pal.color(Mat::Nori, 0.5),
    };
    let mut cells = Vec::new();
    for row in 0..HEIGHT as usize {
        for col in 0..WIDTH as usize {
            let (top, bot) = (pixel(col, 2 * row), pixel(col, 2 * row + 1));
            let style = match (top, bot) {
                (None, None) => continue,
                (Some(t), None) => ("▀", Style::default().fg(shade(t))),
                (None, Some(b)) => ("▄", Style::default().fg(shade(b))),
                (Some(t), Some(b)) => ("▀", Style::default().fg(shade(t)).bg(shade(b))),
            };
            cells.push((x + col as i32, y + row as i32, style));
        }
    }
    for (cx, cy, (glyph, style)) in cells {
        p.glyph(cx, cy, glyph, style);
    }
}
