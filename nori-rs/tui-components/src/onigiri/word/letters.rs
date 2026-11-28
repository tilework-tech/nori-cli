//! The rounded-line letters `n`, `o`, `r`, `i`, built from box-drawing
//! glyphs at any letter height, with the order a pen would trace them.

const UP: i32 = 1;
const DOWN: i32 = 2;
const LEFT: i32 = 4;
const RIGHT: i32 = 8;

/// One letter as the cells a pen visits, in order, relative to the
/// letter's top-left cell.
pub struct Letter {
    pub path: Vec<(i32, i32)>,
    /// The pen ends where it started (the `o`).
    closed: bool,
    /// The last cell is drawn as a full line rather than a half stub (the
    /// end of the `r`).
    full_end: bool,
}

/// Letter, gap and total widths in cells for letters `h` rows tall.
fn widths(h: i32) -> (i32, i32, i32) {
    let wn = ((1.6 * h as f32).round() as i32).max(3);
    let wr = ((0.6 * wn as f32).round() as i32).max(2);
    let gap = ((h as f32 / 3.0).round() as i32).max(1);
    (wn, wr, gap)
}

/// Width of `nori` in cells for letters `h` rows tall.
pub fn width(h: i32) -> i32 {
    let (wn, wr, gap) = widths(h);
    2 * wn + wr + 3 * gap + 1
}

/// The four letters and the x offset of each.
pub fn nori(h: i32) -> [(i32, Letter); 4] {
    let (wn, wr, gap) = widths(h);
    let b = h - 1;
    let n = (0..h)
        .rev()
        .map(|y| (0, y))
        .chain((1..wn).map(|x| (x, 0)))
        .chain((1..h).map(|y| (wn - 1, y)))
        .collect();
    let o = (0..wn - 1)
        .rev()
        .map(|x| (x, 0))
        .chain((1..h).map(|y| (0, y)))
        .chain((1..wn).map(|x| (x, b)))
        .chain((0..b).rev().map(|y| (wn - 1, y)))
        .collect();
    let r = (0..h)
        .rev()
        .map(|y| (0, y))
        .chain((1..wr).map(|x| (x, 0)))
        .collect();
    let i = (0..h).map(|y| (0, y)).collect();
    let letter = |path, closed, full_end| Letter {
        path,
        closed,
        full_end,
    };
    [
        (0, letter(n, false, false)),
        (wn + gap, letter(o, true, false)),
        (2 * wn + 2 * gap, letter(r, false, true)),
        (2 * wn + wr + 3 * gap, letter(i, false, false)),
    ]
}

fn toward(from: (i32, i32), to: (i32, i32)) -> i32 {
    match (to.0 - from.0, to.1 - from.1) {
        (0, -1) => UP,
        (0, 1) => DOWN,
        (-1, 0) => LEFT,
        (1, 0) => RIGHT,
        _ => 0,
    }
}

fn glyph(dirs: i32) -> &'static str {
    match dirs {
        d if d == UP | DOWN => "│",
        d if d == LEFT | RIGHT => "─",
        d if d == DOWN | RIGHT => "╭",
        d if d == DOWN | LEFT => "╮",
        d if d == UP | RIGHT => "╰",
        d if d == UP | LEFT => "╯",
        UP => "╵",
        DOWN => "╷",
        LEFT => "╴",
        RIGHT => "╶",
        _ => " ",
    }
}

impl Letter {
    fn prev(&self, i: usize) -> Option<(i32, i32)> {
        match i {
            0 if self.closed => self.path.last().copied(),
            0 => None,
            _ => Some(self.path[i - 1]),
        }
    }

    fn next(&self, i: usize) -> Option<(i32, i32)> {
        match self.path.get(i + 1) {
            Some(c) => Some(*c),
            None if self.closed => self.path.first().copied(),
            None => None,
        }
    }

    /// The finished glyph of the `i`th cell of the path.
    pub fn glyph(&self, i: usize) -> &'static str {
        let at = self.path[i];
        if self.full_end && i + 1 == self.path.len() {
            return "─";
        }
        let d =
            self.prev(i).map_or(0, |p| toward(at, p)) | self.next(i).map_or(0, |n| toward(at, n));
        glyph(d)
    }

    /// The half glyph shown while the pen is still entering cell `i`, or
    /// `None` for the cell where the stroke starts.
    pub fn stub(&self, i: usize) -> Option<&'static str> {
        (i > 0).then(|| glyph(toward(self.path[i], self.path[i - 1])))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::nori;
    use super::width;

    fn draw(h: i32) -> Vec<String> {
        let mut rows = vec![vec![" "; width(h) as usize]; h as usize];
        for (x0, letter) in nori(h) {
            for (i, (x, y)) in letter.path.iter().enumerate() {
                rows[*y as usize][(x0 + x) as usize] = letter.glyph(i);
            }
        }
        rows.into_iter().map(|r| r.concat()).collect()
    }

    #[test]
    fn letters_at_two_three_and_five_rows() {
        assert_eq!(draw(2), vec!["╭─╮ ╭─╮ ╭─ ╷", "╵ ╵ ╰─╯ ╵  ╵"]);
        assert_eq!(
            draw(3),
            vec![
                "╭───╮ ╭───╮ ╭── ╷",
                "│   │ │   │ │   │",
                "╵   ╵ ╰───╯ ╵   ╵"
            ]
        );
        assert_eq!(
            draw(5),
            vec![
                "╭──────╮  ╭──────╮  ╭────  ╷",
                "│      │  │      │  │      │",
                "│      │  │      │  │      │",
                "│      │  │      │  │      │",
                "╵      ╵  ╰──────╯  ╵      ╵",
            ]
        );
    }
}
