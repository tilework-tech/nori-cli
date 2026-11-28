//! Per-frame cost and output size at a range of terminal sizes.

use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use nori_tui_components::onigiri::Tier;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::Opts;
use crate::cast::terminal;

const SIZES: [(u16, u16); 5] = [(160, 50), (120, 40), (100, 30), (80, 24), (60, 20)];
/// (name, from ms, until ms). The loop phase runs long enough to cover two
/// turns of the light.
const PHASES: [(&str, u64, u64); 2] = [("intro", 0, 2000), ("loop", 3000, 15_000)];

struct Stats {
    avg: f64,
    p99: f64,
    max: f64,
}

fn stats(mut v: Vec<f64>) -> Stats {
    v.sort_by(f64::total_cmp);
    let avg = v.iter().sum::<f64>() / v.len().max(1) as f64;
    let at = |i: usize| {
        v.get(i.min(v.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0.0)
    };
    Stats {
        avg,
        p99: at(v.len() * 99 / 100),
        max: at(v.len()),
    }
}

pub fn run(o: &Opts) -> Result<()> {
    println!(
        "{:>8} {:>6} {:>6} | {:>6} {:>9} {:>9} {:>9} | {:>9} | {:>8} {:>8} {:>10}",
        "size",
        "tier",
        "phase",
        "fps",
        "render_us",
        "p99_us",
        "max_us",
        "frame_us",
        "bytes/f",
        "cells/f",
        "KB/s"
    );
    let tiers = if std::env::args().any(|a| a == "--tier") {
        vec![o.tier]
    } else {
        vec![Tier::True, Tier::X256, Tier::Ansi16]
    };
    for tier in tiers {
        for (w, h) in SIZES {
            for (phase, from, until) in PHASES {
                let opts = Opts {
                    tier,
                    settled: false,
                    size: (w, h),
                    from,
                    until,
                    frame: None,
                };
                row(&opts, phase)?;
            }
        }
    }
    Ok(())
}

fn row(o: &Opts, phase: &str) -> Result<()> {
    let (w, h) = o.size;
    let area = Rect::new(0, 0, w, h);
    let (mut term, sink) = terminal(w, h)?;
    // Separate scenes so the offscreen render and the full frame each keep
    // their own cache, as a real caller would.
    let (mut scene, mut drawn) = (o.scene(), o.scene());
    let (mut render, mut frame) = (Vec::new(), Vec::new());
    let (mut bytes, mut cells, mut frames) = (0, 0, 0);
    let mut prev = Buffer::empty(area);
    let (from, until) = (
        Duration::from_millis(o.from),
        Duration::from_millis(o.until),
    );
    let mut elapsed = from;
    while elapsed <= until {
        let mut buf = Buffer::empty(area);
        let start = Instant::now();
        scene.render(elapsed, area, &mut buf);
        render.push(start.elapsed().as_secs_f64() * 1e6);
        cells += prev.diff(&buf).len();
        prev = buf;

        let start = Instant::now();
        term.draw(|fr| drawn.render(elapsed, fr.area(), fr.buffer_mut()))?;
        frame.push(start.elapsed().as_secs_f64() * 1e6);
        bytes += sink.take().len();
        frames += 1;
        let Some(step) = scene.next_frame_in(elapsed, area) else {
            break;
        };
        elapsed += step;
    }
    let secs = (elapsed.min(until) - from).as_secs_f64().max(0.001);
    let fps = frames as f64 / secs;
    let (r, f) = (stats(render), stats(frame));
    println!(
        "{:>8} {:>6} {:>6} | {:>6.0} {:>9.1} {:>9.1} {:>9.1} | {:>9.1} | {:>8} {:>8} {:>10.1}",
        format!("{w}x{h}"),
        format!("{:?}", o.tier),
        phase,
        fps,
        r.avg,
        r.p99,
        r.max,
        f.avg,
        bytes / frames.max(1),
        cells / frames.max(1),
        bytes as f64 / secs / 1024.0,
    );
    Ok(())
}
