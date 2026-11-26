//! The terminal the PTY session draws into.
//!
//! `vt100` keeps scrollback but ignores `ESC[3J` (erase scrollback), which
//! the TUI sends when it reprints a session. This emulator applies it the way
//! a terminal does: the visible screen stays and the history is dropped, even
//! when the sequence is split across reads.
//!
//! It also applies synchronized output frames atomically; see
//! [`crate::synchronized_update`].

use crate::synchronized_update::SynchronizedUpdates;
use crate::synchronized_update::find_subslice;
use std::time::Instant;
use vt100::Parser;

/// Lines of terminal history kept by the emulator.
pub const SCROLLBACK_ROWS: usize = 20_000;

const ERASE_SCROLLBACK: &[u8] = b"\x1b[3J";

pub struct Emulator {
    parser: Parser,
    rows: u16,
    cols: u16,
    /// Bytes held back because they may begin a split erase-scrollback.
    pending: Vec<u8>,
    synchronized: SynchronizedUpdates,
}

impl Emulator {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: Parser::new(rows, cols, SCROLLBACK_ROWS),
            rows,
            cols,
            pending: Vec::new(),
            synchronized: SynchronizedUpdates::default(),
        }
    }

    /// Apply PTY output. Bytes of a synchronized frame are held until the
    /// frame ends.
    pub fn process(&mut self, data: &[u8]) {
        let ready = self.synchronized.feed(data, Instant::now());
        self.apply(&ready);
    }

    /// Apply a synchronized frame that has stayed open too long.
    pub fn release_stale_frame(&mut self) {
        let ready = self.synchronized.release_stale_frame(Instant::now());
        self.apply(&ready);
    }

    /// Whether a synchronized frame has begun and not yet ended, so the
    /// screen is about to change.
    pub fn is_frame_open(&self) -> bool {
        self.synchronized.is_frame_open()
    }

    fn apply(&mut self, data: &[u8]) {
        let mut buffered = std::mem::take(&mut self.pending);
        buffered.extend_from_slice(data);
        let mut rest = buffered.as_slice();
        while let Some(at) = find_subslice(rest, ERASE_SCROLLBACK) {
            self.parser.process(&rest[..at]);
            self.erase_scrollback();
            rest = &rest[at + ERASE_SCROLLBACK.len()..];
        }
        let keep = (1..ERASE_SCROLLBACK.len())
            .rev()
            .find(|len| rest.ends_with(&ERASE_SCROLLBACK[..*len]))
            .unwrap_or(0);
        self.parser.process(&rest[..rest.len() - keep]);
        self.pending = rest[rest.len() - keep..].to_vec();
    }

    fn erase_scrollback(&mut self) {
        let screen = self.parser.screen().contents_formatted();
        let mut parser = Parser::new(self.rows, self.cols, SCROLLBACK_ROWS);
        parser.process(&screen);
        self.parser = parser;
    }

    /// The visible screen.
    pub fn screen_contents(&self) -> String {
        self.parser.screen().contents()
    }

    /// Whether the terminal is showing its alternate screen.
    pub fn is_alternate_screen(&self) -> bool {
        self.parser.screen().alternate_screen()
    }

    /// Every scrollback line, oldest first, followed by the visible screen.
    pub fn history_contents(&mut self) -> String {
        self.parser.set_scrollback(usize::MAX);
        let depth = self.parser.screen().scrollback();
        let mut lines = Vec::with_capacity(depth + usize::from(self.rows));
        for offset in (1..=depth).rev() {
            self.parser.set_scrollback(offset);
            if let Some(row) = self.parser.screen().rows(0, self.cols).next() {
                lines.push(row);
            }
        }
        self.parser.set_scrollback(0);
        lines.extend(self.parser.screen().rows(0, self.cols));
        lines.join("\n")
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.set_size(rows, cols);
        self.rows = rows;
        self.cols = cols;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn history_lines(emulator: &mut Emulator) -> Vec<String> {
        emulator
            .history_contents()
            .lines()
            .map(|line| line.trim_end().to_string())
            .filter(|line| !line.is_empty())
            .collect()
    }

    fn print_lines(emulator: &mut Emulator, lines: &[&str]) {
        for line in lines {
            emulator.process(format!("{line}\r\n").as_bytes());
        }
    }

    #[test]
    fn history_keeps_lines_scrolled_off_the_screen() {
        let mut emulator = Emulator::new(3, 20);
        print_lines(&mut emulator, &["one", "two", "three", "four", "five"]);
        assert_eq!(
            history_lines(&mut emulator),
            ["one", "two", "three", "four", "five"]
        );
    }

    #[test]
    fn erase_scrollback_drops_history_and_keeps_the_screen() {
        let mut emulator = Emulator::new(3, 20);
        print_lines(&mut emulator, &["old 1", "old 2", "old 3", "old 4"]);
        emulator.process(b"\x1b[3J");
        print_lines(&mut emulator, &["new"]);
        assert_eq!(history_lines(&mut emulator), ["old 3", "old 4", "new"]);
    }

    #[test]
    fn an_erase_split_across_reads_still_applies() {
        let mut emulator = Emulator::new(3, 20);
        print_lines(&mut emulator, &["old 1", "old 2", "old 3", "old 4"]);
        emulator.process(b"\x1b[2J\x1b[H\x1b[");
        emulator.process(b"3Jnew\r\n");
        assert_eq!(history_lines(&mut emulator), ["new"]);
    }

    #[test]
    fn a_synchronized_frame_shows_whole_at_every_read_boundary() {
        let before = b"old 1\r\nold 2";
        let frame = b"\x1b[?2026h\x1b[H\x1b[2Knew 1\r\n\x1b[2Knew 2\x1b[?2026l";
        let screen = |emulator: &Emulator| {
            emulator
                .screen_contents()
                .lines()
                .map(str::trim_end)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("|")
        };
        for first in 0..=frame.len() {
            for second in first..=frame.len() {
                let mut emulator = Emulator::new(3, 20);
                emulator.process(before);
                for chunk in [&frame[..first], &frame[first..second]] {
                    emulator.process(chunk);
                    let shown = screen(&emulator);
                    assert!(
                        shown == "old 1|old 2" || shown == "new 1|new 2",
                        "reads split at {first} and {second} showed {shown:?}"
                    );
                }
                emulator.process(&frame[second..]);
                assert_eq!(screen(&emulator), "new 1|new 2");
                assert!(!emulator.is_frame_open());
            }
        }
    }
}
