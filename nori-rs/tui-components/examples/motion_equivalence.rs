//! Deterministic glyph/style fingerprints for before/after performance experiments.
//! This is deliberately separate from timed runs. Compare stdout from both builds.
use std::fmt::Write;
use std::time::Duration;

use nori_tui_components::MotionBackground;
use nori_tui_components::MotionFormation;
use nori_tui_components::MotionPalette;
use nori_tui_components::MotionScene;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

fn fingerprint(widget: MotionBackground, area: Rect, clip: Rect) -> i64 {
    let mut buffer = Buffer::empty(clip);
    widget.render(area, &mut buffer);
    let mut hash = -3_750_763_034_362_895_579_i64;
    let mut encoded = String::new();
    for cell in buffer.content {
        encoded.clear();
        write!(&mut encoded, "{cell:?}").unwrap();
        for byte in encoded.bytes() {
            hash = (hash ^ i64::from(byte)).wrapping_mul(1_099_511_628_211);
        }
    }
    hash
}

fn main() {
    for (width, height) in [(36, 20), (120, 40)] {
        let area = Rect::new(0, 0, width, height);
        for formation in [MotionFormation::Platoon, MotionFormation::Muster] {
            for frame in 0..150 {
                let leg = f64::from(frame) / 75.0;
                let progress = if leg <= 1.0 { leg } else { 2.0 - leg };
                let widget = MotionBackground::new(
                    MotionScene::Dots,
                    Duration::from_millis((12000 + frame * 40).try_into().unwrap()),
                )
                .transition_to(MotionScene::Formations, progress)
                .formation(formation)
                .palette(MotionPalette::nori());
                println!(
                    "zoom {width}x{height} {formation:?} {frame} {:016x}",
                    fingerprint(widget, area, area)
                );
            }
        }
    }
    let area = Rect::new(3, 2, 48, 24);
    for scene in MotionScene::ALL {
        for time in [0, 12, 36, 60, 84, 94, 86399] {
            for progress in [0.0, 0.37, 0.8, 1.0] {
                for policy in 0..4 {
                    let mut widget = MotionBackground::new(scene, Duration::from_secs(time))
                        .transition_to(MotionScene::Blueprint, progress);
                    let clip = if policy == 3 {
                        Rect::new(10, 7, 25, 14)
                    } else {
                        area
                    };
                    match policy {
                        0 => {}
                        1 => widget = widget.palette(MotionPalette::nori()),
                        2 => widget = widget.ascii(true).palette(MotionPalette::nori()),
                        3 => {
                            widget = widget
                                .quiet_area(Rect::new(16, 9, 12, 8))
                                .palette(MotionPalette::nori())
                        }
                        _ => unreachable!(),
                    }
                    println!(
                        "suite {scene:?} {time} {progress} {policy} {:016x}",
                        fingerprint(widget, area, clip)
                    );
                }
            }
        }
    }
}
