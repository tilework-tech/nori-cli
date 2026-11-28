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

fn caption() -> Caption {
    Caption { version: "v0.42.0".to_string(), cwd: "~/tidepool".to_string() }
}

fn frame(scene: &mut Onigiri, w: u16, h: u16, ms: u64) -> Buffer {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    scene.render(Duration::from_millis(ms), area, &mut buf);
    buf
}

fn intro(tier: Tier, w: u16, h: u16, ms: u64) -> Buffer {
    frame(&mut Onigiri::new(tier, caption()), w, h, ms)
}

fn still(tier: Tier, w: u16, h: u16) -> Buffer {
    frame(&mut Onigiri::new(tier, caption()).settled(), w, h, 0)
}

/// Symbols, then a two-character style id per cell, then the style legend.
fn dump(buf: &Buffer) -> String {
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
            let key = format!("{:?}", Style::default().fg(c.fg).bg(c.bg).add_modifier(c.modifier));
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
    let got: Vec<(u16, u16, Layout)> = [(160, 50), (100, 30), (80, 24), (60, 20), (40, 16), (30, 10), (24, 6), (20, 4)]
        .into_iter()
        .map(|(w, h)| (w, h, layout(w, h)))
        .collect();
    assert_eq!(
        got,
        vec![
            (160, 50, Layout::Side { rows: 27.0 }),
            (100, 30, Layout::Side { rows: 16.0 }),
            (80, 24, Layout::Side { rows: 12.0 }),
            (60, 20, Layout::Stack { rows: 13.0 }),
            (40, 16, Layout::Stack { rows: 10.0 }),
            (30, 10, Layout::Stack { rows: 5.0 }),
            (24, 6, Layout::Line),
            (20, 4, Layout::Line),
        ]
    );
}

#[test]
fn settled_ball_at_each_size_ansi16() {
    for (w, h) in [(100, 30), (60, 20), (40, 16), (30, 10), (24, 6)] {
        assert_snapshot!(format!("settled_ansi16_{w}x{h}"), dump(&still(Tier::Ansi16, w, h)));
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
    for ms in [300, 700, 950, 1200, 1500, 2000] {
        assert_snapshot!(format!("intro_true_100x30_{ms}ms"), dump(&intro(Tier::True, 100, 30, ms)));
    }
}

#[test]
fn light_circles_the_rice_after_the_intro() {
    for ms in [4000, 7000] {
        assert_snapshot!(format!("loop_true_100x30_{ms}ms"), dump(&intro(Tier::True, 100, 30, ms)));
    }
}
