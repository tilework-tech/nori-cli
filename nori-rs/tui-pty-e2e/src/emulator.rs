//! The terminal the PTY session draws into.
//!
//! `vt100` keeps scrollback but ignores `ESC[3J` (erase scrollback), which
//! the TUI sends when it reprints a session. This emulator applies it the way
//! a terminal does: the visible screen stays and the history is dropped, even
//! when the sequence is split across reads.

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
}

impl Emulator {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: Parser::new(rows, cols, SCROLLBACK_ROWS),
            rows,
            cols,
            pending: Vec::new(),
        }
    }

    pub fn process(&mut self, data: &[u8]) {
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

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
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
}
