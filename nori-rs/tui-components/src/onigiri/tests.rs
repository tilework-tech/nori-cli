use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::Duration;

use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::Caption;
use super::Layout;
use super::Onigiri;
use super::Tier;
use super::layout;

pub(super) fn caption() -> Caption {
    Caption {
        version: "v0.42.0".to_string(),
        cwd: "~/tidepool".to_string(),
    }
}

pub(super) fn frame(scene: &mut Onigiri, w: u16, h: u16, ms: u64) -> Buffer {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    scene.render(real(ms as f32), area, &mut buf);
    buf
}

/// The wall-clock time at which the scene reaches `scene_ms` on its own
/// timeline, so tests can name moments by the timeline's constants.
pub(super) fn real(scene_ms: f32) -> Duration {
    Duration::from_secs_f32(super::timeline::real_ms(scene_ms) / 1000.0)
}

pub(super) fn intro(tier: Tier, w: u16, h: u16, ms: u64) -> Buffer {
    frame(&mut Onigiri::new(tier, caption()), w, h, ms)
}

pub(super) fn still(tier: Tier, w: u16, h: u16) -> Buffer {
    frame(&mut Onigiri::new(tier, caption()).settled(), w, h, 0)
}

/// Symbols, then a two-character style id per cell, then the style legend.
pub(super) fn dump(buf: &Buffer) -> String {
    const IDS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let area = buf.area;
    let mut styles: BTreeMap<String, String> = BTreeMap::new();
    let mut ids: Vec<String> = Vec::new();
    let mut out = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out.push('\n');
    for y in 0..area.height {
        for x in 0..area.width {
            let c = &buf[(x, y)];
            let key = format!(
                "{:?}",
                Style::default().fg(c.fg).bg(c.bg).add_modifier(c.modifier)
            );
            let next = ids.len();
            let id = styles
                .entry(key)
                .or_insert_with(|| {
                    let id = format!(
                        "{}{}",
                        IDS[next / IDS.len()] as char,
                        IDS[next % IDS.len()] as char
                    );
                    ids.push(id.clone());
                    id
                })
                .clone();
            out.push_str(&id);
        }
        out.push('\n');
    }
    let mut legend: Vec<(String, String)> = styles.into_iter().map(|(k, v)| (v, k)).collect();
    legend.sort();
    for (id, style) in legend {
        let _ = writeln!(out, "{id} {style}");
    }
    out
}

#[test]
fn layout_follows_the_terminal_shape() {
    let got: Vec<(u16, u16, Layout)> = [
        (160, 50),
        (100, 30),
        (80, 24),
        (60, 20),
        (40, 16),
        (30, 10),
        (24, 6),
        (60, 6),
        (80, 3),
        (80, 2),
        (20, 4),
        (57, 42),
        (90, 60),
        (40, 30),
    ]
    .into_iter()
    .map(|(w, h)| (w, h, layout(w, h)))
    .collect();
    assert_eq!(
        got,
        vec![
            (160, 50, Layout::Side { rows: 31.0 }),
            (100, 30, Layout::Side { rows: 18.0 }),
            (80, 24, Layout::Side { rows: 14.0 }),
            (60, 20, Layout::Side { rows: 10.0 }),
            (40, 16, Layout::Stack { rows: 8.0 }),
            (30, 10, Layout::Mini),
            (24, 6, Layout::Line),
            (60, 6, Layout::Mini),
            (80, 3, Layout::Mini),
            (80, 2, Layout::Line),
            (20, 4, Layout::Line),
            (57, 42, Layout::Stack { rows: 20.0 }),
            (90, 60, Layout::Stack { rows: 33.0 }),
            (40, 30, Layout::Stack { rows: 13.0 }),
        ]
    );
}

#[test]
fn settled_ball_at_each_size_ansi16() {
    for (w, h) in [(100, 30), (60, 20), (40, 16), (30, 10), (24, 6)] {
        assert_snapshot!(
            format!("settled_ansi16_{w}x{h}"),
            dump(&still(Tier::Ansi16, w, h))
        );
    }
}

#[test]
fn settled_ball_in_256_colours() {
    assert_snapshot!(dump(&still(Tier::X256, 60, 20)));
}

#[test]
fn settled_ball_in_truecolor() {
    assert_snapshot!(dump(&still(Tier::True, 60, 20)));
}

#[test]
fn intro_timeline_truecolor() {
    // Gather, press, hop, band wrap.
    for ms in [1000, 1620, 1800, 2600] {
        assert_snapshot!(
            format!("intro_true_100x30_{ms}ms"),
            dump(&intro(Tier::True, 100, 30, ms))
        );
    }
}

#[test]
fn light_circles_the_rice_after_the_intro() {
    for ms in [4000, 7000] {
        assert_snapshot!(
            format!("loop_true_100x30_{ms}ms"),
            dump(&intro(Tier::True, 100, 30, ms))
        );
    }
}

#[test]
fn intro_draws_at_60fps_and_waits_for_the_light_without_an_intro() {
    let scene = Onigiri::new(Tier::True, caption());
    let at = Duration::from_millis;
    let (big, small) = (Rect::new(0, 0, 100, 30), Rect::new(0, 0, 40, 16));
    let got = [
        scene.next_frame_in(at(500), big),
        scene.next_frame_in(at(1999), big),
        scene.next_frame_in(at(500), small),
    ]
    .map(|d| d.map(|d| d.as_millis()));
    let wait = (real(super::timeline::LIGHT_AT) - at(500)).as_millis();
    assert_eq!(got, [Some(16), Some(16), Some(wait)]);
}

#[test]
fn every_scheduled_loop_frame_moves_the_light() {
    let mut scene = Onigiri::new(Tier::True, caption());
    let area = Rect::new(0, 0, 100, 30);
    let mut t = Duration::from_millis(3000);
    let mut prev = frame(&mut scene, 100, 30, 3000);
    let mut stalls = Vec::new();
    while t < Duration::from_millis(9000) {
        let step = scene.next_frame_in(t, area).unwrap_or_default();
        assert!(
            step <= Duration::from_millis(34),
            "loop frame {step:?} apart at {t:?}"
        );
        t += step;
        let mut next = Buffer::empty(area);
        scene.render(t, area, &mut next);
        if next == prev {
            stalls.push(t);
        }
        prev = next;
    }
    assert_eq!(stalls, Vec::<Duration>::new());
}

#[test]
fn tiny_areas_draw_inside_their_bounds() {
    let mut scene = Onigiri::new(Tier::True, caption());
    for area in [
        Rect::new(0, 0, 0, 0),
        Rect::new(0, 0, 1, 1),
        Rect::new(0, 0, 10, 3),
    ] {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 5));
        scene.render(Duration::from_millis(5000), area, &mut buf);
        let outside: Vec<(u16, u16)> = buf
            .area
            .positions()
            .filter(|p| !area.contains(*p) && buf[*p].symbol() != " ")
            .map(|p| (p.x, p.y))
            .collect();
        assert_eq!(outside, Vec::<(u16, u16)>::new(), "{area:?}");
    }
}

#[test]
fn flying_grains_stay_inside_a_sub_area() {
    let area = Rect::new(10, 5, 100, 30);
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 40));
    Onigiri::new(Tier::True, caption()).render(Duration::from_millis(1000), area, &mut buf);
    let outside = buf
        .area
        .positions()
        .filter(|p| !area.contains(*p) && buf[*p].symbol() != " ")
        .count();
    assert_eq!(outside, 0);
}

#[test]
fn still_scenes_ask_for_no_more_frames() {
    let at = Duration::from_millis(500);
    let settled = Onigiri::new(Tier::True, caption()).settled();
    let animated = Onigiri::new(Tier::True, caption());
    assert_eq!(
        [
            settled.next_frame_in(at, Rect::new(0, 0, 100, 30)),
            animated.next_frame_in(at, Rect::new(0, 0, 30, 10)),
            animated.next_frame_in(at, Rect::new(0, 0, 24, 6)),
        ],
        [None, None, None]
    );
}

#[test]
fn redraws_between_light_ticks_change_nothing() {
    let mut scene = Onigiri::new(Tier::True, caption());
    let tick = frame(&mut scene, 160, 50, 6001);
    let changed: Vec<u64> = (6002..6033)
        .filter(|ms| frame(&mut scene, 160, 50, *ms) != tick)
        .collect();
    assert_eq!(changed, Vec::<u64>::new());
    assert_ne!(frame(&mut scene, 160, 50, 6034), tick);
}

#[test]
fn reusing_a_scene_across_resizes_matches_a_fresh_scene() {
    let mut reused = Onigiri::new(Tier::True, caption());
    let cases = [
        (Rect::new(0, 0, 100, 30), 5000),
        (Rect::new(0, 0, 80, 24), 5100),
        (Rect::new(3, 2, 160, 50), 5200),
        (Rect::new(0, 0, 40, 16), 5250),
        (Rect::new(0, 0, 24, 6), 5260),
        (Rect::new(0, 0, 100, 30), 5300),
        (Rect::new(0, 0, 100, 30), 900),
    ];
    for (area, ms) in cases {
        let mut fresh = Buffer::empty(area);
        Onigiri::new(Tier::True, caption()).render(Duration::from_millis(ms), area, &mut fresh);
        let mut got = Buffer::empty(area);
        reused.render(Duration::from_millis(ms), area, &mut got);
        assert_eq!(got, fresh, "{area:?} at {ms}ms");
    }
}

/// Largest channel difference between a cell's glyph colour and its
/// background, i.e. how visible the glyph is.
fn glyph_contrast(buf: &Buffer, x: u16, y: u16) -> i32 {
    use ratatui::style::Color;
    match (buf[(x, y)].fg, buf[(x, y)].bg) {
        (Color::Rgb(r, g, b), Color::Rgb(r2, g2, b2)) => [(r, r2), (g, g2), (b, b2)]
            .into_iter()
            .map(|(a, c)| (i32::from(a) - i32::from(c)).abs())
            .max()
            .unwrap_or(0),
        _ => 0,
    }
}

fn is_braille(s: &str) -> bool {
    s.chars()
        .next()
        .is_some_and(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
}

#[test]
fn grains_fade_in_and_out_instead_of_popping() {
    let mut scene = Onigiri::new(Tier::True, caption());
    let mut worst = (0, String::new());
    let mut prev = frame(&mut scene, 100, 30, 4000);
    for ms in (4033..10_000).step_by(99) {
        let next = frame(&mut scene, 100, 30, ms);
        for y in 0..30 {
            for x in 0..100 {
                let (was, now) = (
                    is_braille(prev[(x, y)].symbol()),
                    is_braille(next[(x, y)].symbol()),
                );
                let pop = match (was, now) {
                    (false, true) => glyph_contrast(&next, x, y),
                    (true, false) => glyph_contrast(&prev, x, y),
                    (true, true) | (false, false) => 0,
                };
                if pop > worst.0 {
                    worst = (pop, format!("({x}, {y}) at {ms}ms"));
                }
            }
        }
        prev = next;
    }
    // About a tenth of the channel range: a glyph this faint against its
    // cell reads as a fade, not a pop.
    assert!(
        worst.0 <= 24,
        "grain popped with contrast {} at {}",
        worst.0,
        worst.1
    );
}

fn top_row(buf: &Buffer) -> Option<u16> {
    buf.area
        .positions()
        .find(|p| buf[*p].symbol() != " ")
        .map(|p| p.y)
}

#[test]
fn ball_hops_after_the_press_and_lands() {
    use super::timeline::HOP;
    use super::timeline::HOP_AT;
    let at = |ms: f32| top_row(&intro(Tier::True, 100, 30, ms as u64));
    let (up, rest) = (at(HOP_AT + HOP / 2.0), at(3500.0));
    assert!(up < rest, "top row {up:?} mid-hop vs {rest:?} at rest");
}

#[test]
fn intro_plays_faster_than_its_timeline_and_the_light_keeps_its_period() {
    use super::timeline::LAND_AT;
    use super::timeline::LIGHT_PERIOD;
    use super::timeline::SETTLED_AT;
    use super::timeline::SPEED;
    let ms = |d: Duration| d.as_secs_f32() * 1000.0;
    assert!((ms(real(LAND_AT)) - LAND_AT / SPEED).abs() < 1.0);
    let turn = ms(real(SETTLED_AT + 2.0 * LIGHT_PERIOD)) - ms(real(SETTLED_AT + LIGHT_PERIOD));
    assert!(
        (turn - LIGHT_PERIOD).abs() < 1.0,
        "one light turn took {turn} ms"
    );
}

#[test]
fn landing_time_is_reported_only_when_the_intro_plays() {
    use super::timeline::LAND_AT;
    let animated = Onigiri::new(Tier::True, caption());
    let settled = Onigiri::new(Tier::True, caption()).settled();
    assert_eq!(
        [
            animated.landed_at(Rect::new(0, 0, 100, 30)),
            animated.landed_at(Rect::new(0, 0, 40, 16)),
            settled.landed_at(Rect::new(0, 0, 100, 30)),
        ],
        [Some(real(LAND_AT)), None, None]
    );
}

#[test]
fn mini_ball_in_each_tier() {
    for (tier, name) in [
        (Tier::True, "true"),
        (Tier::X256, "256"),
        (Tier::Ansi16, "ansi16"),
    ] {
        assert_snapshot!(format!("mini_{name}_60x5"), dump(&still(tier, 60, 5)));
    }
}

#[test]
fn intro_fits_only_where_the_full_intro_plays() {
    use super::intro_fits;
    let got =
        [(100, 30), (60, 20), (40, 16), (60, 6)].map(|(w, h)| intro_fits(Rect::new(0, 0, w, h)));
    assert_eq!(got, [true, false, false, false]);
}

fn bumped(bump: Option<u64>, ms: u64) -> Buffer {
    let mut scene = Onigiri::new(Tier::True, caption());
    if let Some(at) = bump {
        scene.bump(Duration::from_millis(at));
    }
    let area = Rect::new(0, 0, 100, 30);
    let mut buf = Buffer::empty(area);
    scene.render(Duration::from_millis(ms), area, &mut buf);
    buf
}

#[test]
fn a_bump_hops_the_ball_and_lands_it_back_where_it_was() {
    use super::timeline::BUMP_CROUCH;
    use super::timeline::BUMP_DUR;
    use super::timeline::BUMP_HOP;
    let at = 6000;
    let mid = at + (BUMP_CROUCH + BUMP_HOP / 2.0) as u64;
    let done = at + BUMP_DUR as u64 + 20;
    assert!(
        top_row(&bumped(Some(at), mid)) < top_row(&bumped(None, mid)),
        "the ball is up mid-bump"
    );
    assert_eq!(
        [
            bumped(Some(at), at - 20) == bumped(None, at - 20),
            bumped(Some(at), done) == bumped(None, done),
        ],
        [true, true]
    );
}

#[test]
fn a_bump_during_the_intro_waits_for_the_wobble_to_end() {
    use super::timeline::PRESS_DONE;
    let wobbling = real(PRESS_DONE - 20.0).as_millis() as u64;
    assert!(bumped(Some(wobbling - 100), wobbling) == bumped(None, wobbling));
}

#[test]
fn frames_come_at_60fps_while_a_bump_plays() {
    use super::timeline::BUMP_DUR;
    use super::timeline::INTRO_FRAME;
    let area = Rect::new(0, 0, 100, 30);
    let mut scene = Onigiri::new(Tier::True, caption()).settled();
    scene.bump(Duration::from_millis(1000));
    let at = |ms: f32| scene.next_frame_in(Duration::from_secs_f32(ms / 1000.0), area);
    assert_eq!(
        [at(900.0), at(1100.0), at(1000.0 + BUMP_DUR + 10.0)],
        [None, Some(INTRO_FRAME), None]
    );
}
