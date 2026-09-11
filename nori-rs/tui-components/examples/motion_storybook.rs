//! Bare full-screen scene 1 <-> 2 zoom. No text or UI overlays.
#[path = "motion_storybook/profile.rs"]
mod profile;
#[path = "motion_storybook/story.rs"]
mod story;
#[allow(dead_code)]
mod support;

use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use crossterm::event;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use profile::Options;
use profile::Sample;
use story::Story;
use support::StorybookTerminal;

fn main() -> Result<()> {
    let options = Options::parse()?;
    if options.bench {
        return profile::bench(&options);
    }
    let mut terminal = StorybookTerminal::enter()?;
    let mut story = Story::new(options.muster);
    let mut paused = options.still;
    let mut previous = Instant::now();
    let period = Duration::from_secs_f64(1.0 / options.fps);
    let mut dirty = true;
    let mut frame_number = 0;
    let mut samples = Vec::new();
    loop {
        let now = Instant::now();
        let delta = now.duration_since(previous);
        if dirty || (!paused && delta >= period) {
            if !paused {
                story.advance(delta);
            }
            previous = now;
            let mut sample = Sample {
                frame: frame_number,
                phase: story.phase(),
                zooming_in: story.zooming_in,
                interval: delta,
                ..Sample::default()
            };
            let start = Instant::now();
            terminal.terminal.draw(|frame| {
                let area = frame.area();
                sample.width = area.width;
                sample.height = area.height;
                let start = Instant::now();
                frame.render_widget(story.widget(), area);
                sample.widget = start.elapsed();
            })?;
            sample.total = start.elapsed();
            sample.output = sample.total.saturating_sub(sample.widget);
            if options.output.is_some() {
                samples.push(sample);
            }
            frame_number += 1;
            dirty = false;
            if options.frames.is_some_and(|limit| frame_number >= limit) {
                break;
            }
        }
        // Preserve the previous example's start-to-start cadence for measurement.
        let wait = if paused {
            Duration::from_millis(250)
        } else {
            period.saturating_sub(previous.elapsed())
        };
        if !event::poll(wait)? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    break;
                }
                match key.code {
                    KeyCode::Char(' ') => paused = !paused,
                    KeyCode::Char('s') => {
                        story = Story::new(options.muster);
                        story.advance(Duration::from_millis(2400));
                        paused = true;
                    }
                    _ => continue,
                }
                previous = Instant::now();
                dirty = true;
            }
            Event::Resize(_, _) => dirty = true,
            _ => {}
        }
    }
    drop(terminal); // Restore the alternate screen before writing profiling data.
    profile::save(&options, &samples)
}
