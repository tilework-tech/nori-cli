//! Behaviour of the rounded-line wordmark: pen draw-on, neon ignition, the
//! i dot drop and the shimmer that follows the light.

use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::style::Color;

use super::Tier;
use super::color::Mat;
use super::color::color;
use super::tests::dump;
use super::tests::intro;
use super::tests::still;
use super::word;

const BOX: &str = "╭╮╰╯│─╵╷╴╶";

fn strokes(buf: &Buffer) -> Vec<(u16, u16)> {
    buf.area
        .positions()
        .filter(|p| {
            let s = buf[*p].symbol();
            !s.is_empty() && s.chars().all(|c| BOX.contains(c))
        })
        .map(|p| (p.x, p.y))
        .collect()
}

fn letter_rows(buf: &Buffer) -> usize {
    let mut ys: Vec<u16> = strokes(buf).iter().map(|p| p.1).collect();
    ys.dedup();
    ys.len()
}

#[test]
fn letters_grow_with_the_ball() {
    let got = [(80, 24), (100, 30), (160, 50)].map(|(w, h)| letter_rows(&still(Tier::True, w, h)));
    assert_eq!(got, [4, 5, 9]);
}

#[test]
fn pen_writes_the_letters_left_to_right() {
    let settled = strokes(&still(Tier::True, 100, 30)).len();
    assert_eq!(
        strokes(&intro(Tier::True, 100, 30, word::WORD_AT as u64 - 1)).len(),
        0
    );
    let mut prev = (0, 0);
    for ms in (word::WORD_AT as u64..word::DRAW_END as u64).step_by(40) {
        let s = strokes(&intro(Tier::True, 100, 30, ms));
        let now = (s.len(), s.iter().map(|p| p.0).max().unwrap_or(0));
        assert!(
            now.0 >= prev.0 && now.1 >= prev.1,
            "{prev:?} -> {now:?} at {ms}ms"
        );
        prev = now;
    }
    assert_eq!(
        strokes(&intro(Tier::True, 100, 30, word::DRAW_END as u64 + 1)).len(),
        settled
    );
}

#[test]
fn all_four_pens_are_writing_almost_at_once() {
    let right = |buf: &Buffer| strokes(buf).iter().map(|p| p.0).max();
    let early = word::WORD_AT as u64 + 3 * word::PEN_STAGGER as u64 + 40;
    assert_eq!(
        right(&intro(Tier::True, 100, 30, early)),
        right(&still(Tier::True, 100, 30))
    );
}

#[test]
fn dot_falls_while_the_ball_hops_and_lands_with_it() {
    use super::timeline::HOP_AT;
    use super::timeline::LAND_AT;
    let has = |ms: f32, glyph: &str| {
        let buf = intro(Tier::True, 100, 30, ms as u64);
        let green = color(Tier::True, Mat::Green, 1.0);
        buf.area
            .positions()
            .any(|p| buf[p].symbol() == glyph && buf[p].fg == green)
    };
    let falling = |ms: f32| has(ms, "▀") || has(ms, "▄");
    assert_eq!(
        [
            falling(HOP_AT - 20.0),
            falling(HOP_AT + 100.0),
            has(LAND_AT + 10.0, "▬")
        ],
        [false, true, true]
    );
}

#[test]
fn resting_wordmark_keeps_a_faint_glow() {
    let buf = intro(Tier::True, 100, 30, 4000);
    let s = strokes(&buf);
    let (x0, x1) = (
        s.iter().map(|p| p.0).min().unwrap_or(0),
        s.iter().map(|p| p.0).max().unwrap_or(0),
    );
    let glowing = buf
        .area
        .positions()
        .filter(|p| (x0..=x1).contains(&p.x) && matches!(buf[*p].bg, Color::Rgb(..)))
        .count();
    assert!(
        glowing > s.len(),
        "only {glowing} glowing cells for {} strokes",
        s.len()
    );
}

#[test]
fn dot_settles_just_above_the_i() {
    let buf = still(Tier::True, 100, 30);
    let s = strokes(&buf);
    let stem_top = s
        .iter()
        .filter(|p| buf[**p].symbol() == "╷")
        .max_by_key(|p| p.0)
        .copied();
    let dot = buf
        .area
        .positions()
        .find(|p| buf[*p].symbol() == "■")
        .map(|p| (p.x, p.y + 1));
    assert_eq!(dot, stem_top);
}

#[test]
fn ansi16_wordmark_paints_no_background() {
    let left = strokes(&still(Tier::Ansi16, 100, 30))
        .iter()
        .map(|p| p.0)
        .min()
        .unwrap_or(0)
        - 2;
    let mut painted = Vec::new();
    for ms in (100..3600).step_by(40).chain([7300, 10_300]) {
        let buf = intro(Tier::Ansi16, 100, 30, ms);
        painted.extend(
            buf.area
                .positions()
                .filter(|p| p.x >= left && buf[*p].bg != Color::Reset)
                .map(|p| (ms, p.x, p.y)),
        );
    }
    assert_eq!(painted, Vec::new());
}

/// Brightness-weighted mean x of the strokes minus their plain mean x:
/// negative when the shimmer sits on the left, positive on the right.
fn shine_offset(buf: &Buffer) -> f32 {
    let s = strokes(buf);
    let lum = |p: &(u16, u16)| match buf[*p].fg {
        Color::Rgb(r, g, b) => (f32::from(r) + f32::from(g) + f32::from(b)) / 765.0,
        _ => 0.0,
    };
    let plain = s.iter().map(|p| f32::from(p.0)).sum::<f32>() / s.len() as f32;
    let weights: Vec<f32> = s.iter().map(|p| lum(p).powi(8)).collect();
    let total: f32 = weights.iter().sum();
    s.iter()
        .zip(&weights)
        .map(|(p, w)| f32::from(p.0) * w)
        .sum::<f32>()
        / total
        - plain
}

#[test]
fn shimmer_sits_on_the_side_the_light_is_on() {
    // Light at 120 degrees (lower left) and at -60 degrees (upper right).
    let got = [7300, 10_300].map(|ms| shine_offset(&intro(Tier::True, 100, 30, ms)) > 0.0);
    assert_eq!(got, [false, true]);
}

#[test]
fn shimmer_repeats_with_every_turn_of_the_light() {
    assert_eq!(
        intro(Tier::True, 100, 30, 7000),
        intro(Tier::True, 100, 30, 13_000)
    );
}

#[test]
fn wordmark_intro_truecolor() {
    // Pens mid-stroke, letters igniting, dot falling, dot squashed, cooled.
    for ms in [600, 1100, 1800, 1960, 3300] {
        assert_snapshot!(
            format!("word_intro_true_100x30_{ms}ms"),
            dump(&intro(Tier::True, 100, 30, ms))
        );
    }
}

#[test]
fn wordmark_shimmer_in_each_tier() {
    for (tier, name) in [
        (Tier::True, "true"),
        (Tier::X256, "256"),
        (Tier::Ansi16, "ansi16"),
    ] {
        assert_snapshot!(
            format!("word_shimmer_{name}_100x30_7300ms"),
            dump(&intro(tier, 100, 30, 7300))
        );
    }
}

#[test]
fn the_i_dot_hops_with_a_bump() {
    use super::tests::caption;
    use super::timeline::BUMP_CROUCH;
    use super::timeline::BUMP_HOP;
    use ratatui::layout::Rect;
    use std::time::Duration;
    let frame = |bump: bool, ms: u64, (w, h): (u16, u16)| {
        let mut scene = super::Onigiri::new(Tier::True, caption());
        if bump {
            scene.bump(Duration::from_millis(6000));
        }
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        scene.render(Duration::from_millis(ms), area, &mut buf);
        buf
    };
    let green = color(Tier::True, Mat::Green, 1.0);
    let dot_row = |buf: &Buffer| {
        buf.area
            .positions()
            .filter(|p| buf[*p].fg == green && "■▀▄█".contains(buf[*p].symbol()))
            .map(|p| p.y)
            .min()
    };
    let mid = 6000 + (BUMP_CROUCH + BUMP_HOP / 2.0) as u64;
    // Beside the ball, and under it in a portrait area, where the hop
    // must show in front of the ball.
    for size in [(100, 30), (57, 42)] {
        let up = dot_row(&frame(true, mid, size));
        let rest = dot_row(&frame(false, mid, size));
        assert!(
            up.is_some() && up < rest,
            "{size:?}: dot row {up:?} mid-bump vs {rest:?} at rest"
        );
    }
}

#[test]
fn portrait_area_stacks_the_block_wordmark_under_the_ball() {
    let (w, h) = (57, 42);
    let buf = still(Tier::True, w, h);
    let marks = strokes(&buf);
    assert!(
        letter_rows(&buf) >= 3,
        "block letters, not the text wordmark"
    );
    let top = marks.iter().map(|p| p.1).min().unwrap_or(0);
    let bottom = marks.iter().map(|p| p.1).max().unwrap_or(0);
    let (left, right) = (
        marks.iter().map(|p| p.0).min().unwrap_or(0),
        marks.iter().map(|p| p.0).max().unwrap_or(0),
    );
    assert!(top > h / 2, "the word sits under the ball, from row {top}");
    assert!(
        (i32::from(left + right) - i32::from(w - 1)).abs() <= 2,
        "the word is centred: cols {left}..={right}"
    );
    let row = |y: u16| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>();
    let caption = (bottom + 1..h).find(|y| row(*y).contains("cli v0.42.0"));
    assert!(caption.is_some(), "the caption sits under the word");
}

#[test]
fn portrait_wordmark_truecolor() {
    assert_snapshot!(dump(&still(Tier::True, 57, 42)));
}
