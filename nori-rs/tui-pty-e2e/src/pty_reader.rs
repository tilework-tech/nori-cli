//! The terminal side of the PTY, kept attached between polls.
//!
//! A real terminal reads everything the program writes and answers its
//! queries at once. The tests only look at the screen when they poll, and
//! many wait on something else in between: an agent's stderr, a file, a
//! sleep. If nothing read the PTY meanwhile, the program's writes would block
//! once the PTY buffer filled, stalling its whole event loop. On macOS that
//! buffer holds only a few kilobytes, so a TUI that animates while a test
//! waits elsewhere stops dead there and nowhere else.
//!
//! A background thread therefore reads the PTY for the whole session, answers
//! terminal queries as they arrive, and queues the output.
//! [`crate::TuiSession::poll`] applies the queued output to the screen.

use std::io::Read;
use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::mpsc;

use crate::terminal_queries::TerminalQueries;

/// The PTY writer, shared by the test's input and the query replies.
pub(crate) type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// Output the reader thread has read and not yet handed to the screen.
pub(crate) struct PtyOutput {
    rx: mpsc::Receiver<std::io::Result<Vec<u8>>>,
    /// The read error that ended the output, reported by every later take.
    ended: Option<std::io::ErrorKind>,
}

impl PtyOutput {
    /// Read `reader` on a background thread until the PTY closes, answering
    /// queries through `writer`.
    pub(crate) fn spawn(
        mut reader: Box<dyn Read + Send>,
        writer: SharedWriter,
        mut queries: TerminalQueries,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                let chunk = match reader.read(&mut buf) {
                    // EOF, or EIO once the child side has closed.
                    Ok(0) => return,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => Err(error),
                    Ok(n) => {
                        let bytes = &buf[..n];
                        let mut writer = writer.lock().unwrap_or_else(PoisonError::into_inner);
                        queries
                            .respond(bytes, &mut *writer)
                            .map(|()| bytes.to_vec())
                    }
                };
                let failed = chunk.is_err();
                if tx.send(chunk).is_err() || failed {
                    return;
                }
            }
        });
        Self { rx, ended: None }
    }

    /// Everything read since the last call, without waiting. Once the output
    /// is drained, the read error that ended it (EIO after the child exits on
    /// Linux) is returned on every call, as a direct read would.
    pub(crate) fn take(&mut self) -> std::io::Result<Vec<u8>> {
        let mut out = Vec::new();
        while let Ok(chunk) = self.rx.try_recv() {
            match chunk {
                Ok(bytes) => out.extend_from_slice(&bytes),
                Err(error) => self.ended = Some(error.kind()),
            }
        }
        match self.ended {
            Some(kind) if out.is_empty() => Err(kind.into()),
            Some(_) | None => Ok(out),
        }
    }
}
