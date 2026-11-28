//! Asciicast v2 recordings of the welcome.

use std::cell::RefCell;
use std::fs::File;
use std::io;
use std::io::Write;
use std::rc::Rc;
use std::time::Duration;

use anyhow::Result;
use ratatui::Terminal;
use ratatui::TerminalOptions;
use ratatui::Viewport;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;

use crate::Opts;

/// A writer that keeps everything written to it.
#[derive(Clone, Default)]
pub struct Sink(Rc<RefCell<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Sink {
    pub fn take(&self) -> Vec<u8> {
        std::mem::take(&mut *self.0.borrow_mut())
    }
}

/// An offscreen terminal of `w` x `h` cells whose output lands in the sink.
pub fn terminal(w: u16, h: u16) -> Result<(Terminal<CrosstermBackend<Sink>>, Sink)> {
    let sink = Sink::default();
    let term = Terminal::with_options(
        CrosstermBackend::new(sink.clone()),
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, w, h)),
        },
    )?;
    Ok((term, sink))
}

fn json_str(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as i32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn write(path: &str, o: &Opts) -> Result<()> {
    let (w, h) = o.size;
    let (mut term, sink) = terminal(w, h)?;
    let mut scene = o.scene();
    let mut f = File::create(path)?;
    writeln!(
        f,
        "{{\"version\": 2, \"width\": {w}, \"height\": {h}, \"env\": {{\"TERM\": \"xterm-256color\"}}}}"
    )?;
    writeln!(f, "[0.0, \"o\", {}]", json_str(b"\x1b[?25l"))?;
    let area = Rect::new(0, 0, w, h);
    let (from, until) = (
        Duration::from_millis(o.from),
        Duration::from_millis(o.until),
    );
    let mut elapsed = from;
    while elapsed <= until {
        term.draw(|fr| scene.render(elapsed, fr.area(), fr.buffer_mut()))?;
        let bytes = sink.take();
        if !bytes.is_empty() {
            writeln!(
                f,
                "[{:.3}, \"o\", {}]",
                (elapsed - from).as_secs_f64(),
                json_str(&bytes)
            )?;
        }
        let step = o
            .frame
            .map(Duration::from_millis)
            .or_else(|| scene.next_frame_in(elapsed, area));
        let Some(step) = step else { break };
        elapsed += step;
    }
    writeln!(
        f,
        "[{:.3}, \"o\", \"\"]",
        (o.until - o.from) as f64 / 1000.0 + 0.5
    )?;
    Ok(())
}
