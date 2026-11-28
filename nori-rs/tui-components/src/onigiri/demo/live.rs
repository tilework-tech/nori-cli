//! Plays the welcome in this terminal with a performance overlay.

use std::cell::Cell;
use std::io;
use std::io::Stdout;
use std::io::Write;
use std::rc::Rc;
use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use crossterm::SynchronizedUpdate;
use crossterm::cursor;
use crossterm::event;
use crossterm::event::Event;
use crossterm::event::KeyEventKind;
use crossterm::execute;
use crossterm::terminal;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::style::Modifier;
use ratatui::style::Style;

use crate::Opts;

/// How often the overlay's numbers refresh.
const WINDOW: Duration = Duration::from_millis(500);
/// How often a still scene checks for keys and resizes.
const IDLE_POLL: Duration = Duration::from_millis(250);

/// Stdout that counts the bytes written through it.
struct Counted {
    out: Stdout,
    bytes: Rc<Cell<usize>>,
}

impl Write for Counted {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let n = self.out.write(b)?;
        self.bytes.set(self.bytes.get() + n);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

/// Puts the terminal back however the loop ends.
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), terminal::LeaveAlternateScreen, cursor::Show);
    }
}

#[derive(Default)]
struct Window {
    frames: i32,
    bytes: usize,
    render_us: f64,
    frame_us: f64,
    max_frame_us: f64,
}

pub fn run(o: &Opts) -> Result<()> {
    terminal::enable_raw_mode()?;
    let _restore = Restore;
    execute!(io::stdout(), terminal::EnterAlternateScreen, cursor::Hide)?;
    let bytes = Rc::new(Cell::new(0));
    let mut term = Terminal::new(CrosstermBackend::new(Counted {
        out: io::stdout(),
        bytes: Rc::clone(&bytes),
    }))?;
    term.clear()?;
    let mut scene = o.scene();
    let start = Instant::now();
    let (mut window, mut window_at) = (Window::default(), Instant::now());
    let mut hud = String::from("measuring...");
    loop {
        let elapsed = start.elapsed();
        let began = Instant::now();
        let mut render_us = 0.0;
        let area = term.size()?;
        let area = ratatui::layout::Rect::new(0, 0, area.width, area.height);
        io::stdout().sync_update(|_| {
            term.draw(|fr| {
                let r = Instant::now();
                scene.render(elapsed, fr.area(), fr.buffer_mut());
                render_us = r.elapsed().as_secs_f64() * 1e6;
                let full = fr.area();
                let dim = Style::default().add_modifier(Modifier::DIM);
                fr.buffer_mut().set_stringn(
                    0,
                    full.bottom().saturating_sub(1),
                    &hud,
                    usize::from(full.width),
                    dim,
                );
            })
            .map(|_| ())
        })??;
        let frame_us = began.elapsed().as_secs_f64() * 1e6;
        window.frames += 1;
        window.render_us += render_us;
        window.frame_us += frame_us;
        window.max_frame_us = window.max_frame_us.max(frame_us);
        window.bytes += bytes.replace(0);
        let span = window_at.elapsed();
        if span >= WINDOW {
            let secs = span.as_secs_f64();
            let n = f64::from(window.frames);
            hud = format!(
                " {:.1} fps | render {:.0} us | frame {:.0} us (max {:.0}) | {:.1} KB/s | {}x{} {:?} | any key quits ",
                n / secs,
                window.render_us / n,
                window.frame_us / n,
                window.max_frame_us,
                window.bytes as f64 / secs / 1024.0,
                area.width,
                area.height,
                o.tier,
            );
            (window, window_at) = (Window::default(), Instant::now());
        }
        let wait = scene
            .next_frame_in(elapsed, area)
            .map_or(IDLE_POLL, |step| step.saturating_sub(began.elapsed()));
        if event::poll(wait)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            return Ok(());
        }
    }
}
