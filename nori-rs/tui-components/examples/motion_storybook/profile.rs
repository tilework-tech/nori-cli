//! Example-only instrumentation. No per-cell timers or logging in the renderer.
use std::fs::File;
use std::hint::black_box;
use std::io;
use std::io::BufWriter;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use ratatui::backend::Backend;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use super::story::Story;

pub struct Options {
    pub muster: bool,
    pub still: bool,
    pub bench: bool,
    pub output: Option<PathBuf>,
    pub frames: Option<i32>,
    pub width: u16,
    pub height: u16,
    pub fps: f64,
}

impl Options {
    pub fn parse() -> Result<Self> {
        let mut options = Self {
            muster: false,
            still: false,
            bench: false,
            output: None,
            frames: None,
            width: 120,
            height: 40,
            fps: 25.0,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--muster" => options.muster = true,
                "--still" => options.still = true,
                "--bench" => options.bench = true,
                "--profile" => {
                    options.output = Some(args.next().context("--profile needs a CSV path")?.into())
                }
                "--frames" => {
                    options.frames = Some(args.next().context("--frames needs a count")?.parse()?)
                }
                "--fps" => options.fps = args.next().context("--fps needs a rate")?.parse()?,
                "--size" => {
                    let size = args.next().context("--size needs WIDTHxHEIGHT")?;
                    let (width, height) =
                        size.split_once('x').context("--size needs WIDTHxHEIGHT")?;
                    options.width = width.parse()?;
                    options.height = height.parse()?;
                }
                _ => anyhow::bail!(
                    "Unknown option {arg}. Options: --muster --still --fps N --frames N --profile FILE --bench --size WIDTHxHEIGHT"
                ),
            }
        }
        ensure!(
            options.fps.is_finite() && (1.0..=240.0).contains(&options.fps),
            "FPS must be between 1 and 240"
        );
        ensure!(
            options.width > 0 && options.height > 0,
            "Viewport must be nonempty"
        );
        ensure!(
            options.frames.is_none_or(|count| count > 0),
            "Frame count must be positive"
        );
        ensure!(
            options.output.is_none() || options.frames.is_some() || options.bench,
            "Live profiling requires --frames to bound recording memory"
        );
        ensure!(
            !options.bench || options.output.is_some(),
            "--bench requires --profile FILE"
        );
        ensure!(
            !options.still || options.frames.is_none(),
            "--still cannot be combined with --frames"
        );
        Ok(options)
    }
}

#[derive(Default)]
pub struct Sample {
    pub frame: i32,
    pub width: u16,
    pub height: u16,
    pub phase: f64,
    pub zooming_in: bool,
    pub interval: Duration,
    pub widget: Duration,
    /// Live: draw minus widget; bench: actual lazy diff + Crossterm encoding.
    pub output: Duration,
    pub reset: Duration,
    pub total: Duration,
    /// Extra scan for attribution only; excluded from total.
    pub diff_probe: Option<Duration>,
    pub changed: Option<i32>,
    pub bytes: Option<i64>,
    pub writes: Option<i64>,
}

pub fn save(options: &Options, samples: &[Sample]) -> Result<()> {
    let Some(path) = &options.output else {
        return Ok(());
    };
    let mut file =
        BufWriter::new(File::create(path).with_context(|| format!("create {}", path.display()))?);
    writeln!(
        file,
        "mode,variant,build,fps,frame,width,height,phase,direction,interval_us,widget_us,output_us,reset_us,total_us,diff_probe_us,changed_cells,ansi_bytes,write_calls"
    )?;
    let mode = if options.bench { "bench" } else { "live" };
    let variant = if options.muster { "muster" } else { "platoon" };
    let build = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    for sample in samples {
        let direction = if sample.zooming_in { "in" } else { "out" };
        let us = |duration: Duration| duration.as_secs_f64() * 1e6;
        writeln!(
            file,
            "{mode},{variant},{build},{},{},{},{},{:.6},{direction},{:.3},{:.3},{:.3},{:.3},{:.3},{},{},{},{}",
            options.fps,
            sample.frame,
            sample.width,
            sample.height,
            sample.phase,
            us(sample.interval),
            us(sample.widget),
            us(sample.output),
            us(sample.reset),
            us(sample.total),
            sample
                .diff_probe
                .map(|d| format!("{:.3}", us(d)))
                .unwrap_or_default(),
            sample.changed.map(|n| n.to_string()).unwrap_or_default(),
            sample.bytes.map(|n| n.to_string()).unwrap_or_default(),
            sample.writes.map(|n| n.to_string()).unwrap_or_default()
        )?;
    }
    file.flush()?;
    Ok(())
}

#[derive(Default)]
struct CountingSink {
    bytes: i64,
    writes: i64,
}

impl Write for CountingSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes += bytes.len() as i64;
        self.writes += 1;
        black_box(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Uses the real widget, lazy Ratatui diff iterator, and Crossterm encoder.
/// The sink counts bytes and Write calls; it deliberately does no OS I/O.
pub fn bench(options: &Options) -> Result<()> {
    let area = Rect::new(0, 0, options.width, options.height);
    let mut previous = Buffer::empty(area);
    let mut current = Buffer::empty(area);
    let mut sink = CountingSink::default();
    let step = Duration::from_secs_f64(1.0 / options.fps);
    let frames = options.frames.unwrap_or(750);
    let mut samples = Vec::with_capacity(frames as usize);
    let mut story = Story::new(options.muster);
    // One full loop warms code, allocator and buffers. Reset the simulated clock
    // afterward so measured traces have identical phases across runs/builds.
    let warmup = (options.fps * 6.0).ceil() as i32;
    for frame in -warmup..frames {
        if frame == 0 {
            story = Story::new(options.muster);
            previous.reset();
            current.reset();
        }
        let mut sample = Sample {
            frame,
            width: area.width,
            height: area.height,
            phase: story.phase(),
            zooming_in: story.zooming_in,
            ..Sample::default()
        };
        let start = Instant::now();
        story.widget().render(area, &mut current);
        sample.widget = start.elapsed();
        let start = Instant::now();
        let mut changed = 0;
        let bytes = sink.bytes;
        let writes = sink.writes;
        {
            let mut backend = CrosstermBackend::new(&mut sink);
            backend.draw(previous.diff_iter(&current).inspect(|_| changed += 1))?;
            Backend::flush(&mut backend)?;
        }
        sample.output = start.elapsed();
        sample.changed = Some(changed);
        sample.bytes = Some(sink.bytes - bytes);
        sample.writes = Some(sink.writes - writes);
        // A separate, warm-cache diff-only scan estimates its share of output.
        // Do not subtract it and present the remainder as an exact encoder cost.
        let start = Instant::now();
        black_box(previous.diff_iter(&current).count());
        sample.diff_probe = Some(start.elapsed());
        let start = Instant::now();
        previous.reset();
        std::mem::swap(&mut previous, &mut current);
        sample.reset = start.elapsed();
        sample.total = sample.widget + sample.output + sample.reset;
        if frame >= 0 {
            samples.push(sample);
        }
        story.advance(step);
    }
    save(options, &samples)
}
