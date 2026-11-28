//! Behaviour of the rounded-line wordmark: pen draw-on, neon ignition, the
//! i dot drop and the shimmer that follows the light.

use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::style::Color;

use super::Tier;
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
    for ms in (1200..3600).step_by(40).chain([6750, 9750]) {
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
    let got = [6750, 9750].map(|ms| shine_offset(&intro(Tier::True, 100, 30, ms)) > 0.0);
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
    // Pen mid-stroke, letters igniting, dot falling, dot squashed, cooled.
    for ms in [1500, 2050, 2550, 2700, 3300] {
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
            format!("word_shimmer_{name}_100x30_6750ms"),
            dump(&intro(tier, 100, 30, 6750))
        );
    }
}
