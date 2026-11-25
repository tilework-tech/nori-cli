//! Keyboard input and optional palette replies share one terminal reader across TUI screens.

use std::future::poll_fn;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;
use std::time::Duration;
use std::time::Instant;

use crossterm::event::TerminalEvent;
use crossterm::event::TerminalEventStream;
use crossterm::style::Color;
use tokio_stream::Stream;

const HANDOFF_WINDOW: Duration = Duration::from_millis(200);

#[derive(Default)]
struct Reader {
    stream: Option<TerminalEventStream>,
    query: Option<PaletteQuery>,
}

struct PaletteQuery {
    deadline: Instant,
    foreground_pending: bool,
    background_pending: bool,
}

#[derive(Clone, Default)]
pub(super) struct TerminalInput(Arc<Mutex<Reader>>);

impl TerminalInput {
    fn reader(&self) -> MutexGuard<'_, Reader> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) async fn next(&self) -> Option<io::Result<TerminalEvent>> {
        poll_fn(|cx| {
            let mut reader = self.reader();
            Pin::new(
                reader
                    .stream
                    .get_or_insert_with(TerminalEventStream::default),
            )
            .poll_next(cx)
        })
        .await
    }

    pub(super) fn request_palette(&self) {
        if !cfg!(unix) {
            return;
        }
        let mut reader = self.reader();
        // OSC replies have no request ID. Never repeat a still-outstanding slot:
        // an old reply would otherwise complete the new query before handoff.
        let bytes: &[u8] = match &reader.query {
            Some(query) if query.foreground_pending && query.background_pending => return,
            Some(query) if query.foreground_pending => b"\x1b]11;?\x1b\\",
            Some(_) => b"\x1b]10;?\x1b\\",
            None => b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\",
        };
        if write_query(bytes).is_ok() {
            reader.query = Some(PaletteQuery {
                deadline: Instant::now() + HANDOFF_WINDOW,
                foreground_pending: true,
                background_pending: true,
            });
        }
    }

    pub(super) fn color_report(&self, slot: u8, color: Option<Color>) -> bool {
        let mut reader = self.reader();
        if let Some(query) = &mut reader.query {
            match slot {
                10 => query.foreground_pending = false,
                11 => query.background_pending = false,
                _ => (),
            }
            if !query.foreground_pending && !query.background_pending {
                reader.query = None;
            }
        }
        crate::terminal_palette::record(slot, color)
    }

    pub(super) fn release(&self) {
        let mut reader = self.reader();
        drop(reader.stream.take());
        if let Some(query) = reader.query.take() {
            let remaining = query.deadline.saturating_duration_since(Instant::now());
            // DSR 5 replies in output order, after any supported OSC replies. Only wait
            // while the original query is young and we still own the terminal.
            if !remaining.is_zero() && write_query(b"\x1b[5n").is_ok() {
                let _ = crossterm::event::drain_terminal_responses(remaining);
            }
        }
    }
}

fn write_query(bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut tty = io::stdout().lock();
    tty.write_all(bytes)?;
    tty.flush()
}
