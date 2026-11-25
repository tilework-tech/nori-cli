//! Replies from the synthetic xterm used by the PTY tests.

use std::io;
use std::io::Write;

// The device-attributes reply tells crossterm the keyboard uses legacy input.
// The palette uses a white foreground and black background.
const QUERIES: &[(&[u8], &[u8])] = &[
    (b"\x1b[6n", b"\x1b[1;1R"),
    (b"\x1b[c", b"\x1b[?1;2c"),
    (b"\x1b]10;?\x1b\\", b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\"),
    (b"\x1b]11;?\x1b\\", b"\x1b]11;rgb:0000/0000/0000\x1b\\"),
];

#[derive(Default)]
pub(super) struct TerminalQueries {
    pending: Vec<u8>,
    pub(super) withhold_palette: bool,
}

impl TerminalQueries {
    /// Observe the same byte stream as the screen parser. Keep a possible query
    /// prefix when the PTY splits a sequence between reads.
    pub(super) fn respond(&mut self, bytes: &[u8], writer: &mut dyn Write) -> io::Result<()> {
        for &byte in bytes {
            if self.pending.is_empty() && byte != 0x1b {
                continue;
            }
            self.pending.push(byte);
            if let Some((query, reply)) = QUERIES
                .iter()
                .find(|(query, _)| query.starts_with(&self.pending))
            {
                if self.pending.len() == query.len() {
                    if !(self.withhold_palette && query.starts_with(b"\x1b]")) {
                        writer.write_all(reply)?;
                        writer.flush()?;
                    }
                    self.pending.clear();
                }
            } else {
                // An ESC inside an unfinished OSC may start the next query.
                let next = self.pending[1..]
                    .iter()
                    .rposition(|&b| b == 0x1b)
                    .map_or(self.pending.len(), |i| i + 1);
                self.pending.drain(..next);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::TerminalQueries;
    use pretty_assertions::assert_eq;

    #[test]
    fn startup_queries_receive_the_same_replies_at_every_read_boundary() {
        let stream = b"hello\x1b[6n\x1b[?u\x1b[c\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b[6nworld";
        let expected = b"\x1b[1;1R\x1b[?1;2c\x1b]10;rgb:ffff/ffff/ffff\x1b\\\x1b]11;rgb:0000/0000/0000\x1b\\\x1b[1;1R";

        for boundary in 0..=stream.len() {
            let mut terminal = TerminalQueries::default();
            let mut replies = Vec::new();
            terminal.respond(&stream[..boundary], &mut replies).unwrap();
            terminal.respond(&stream[boundary..], &mut replies).unwrap();
            assert_eq!(replies.as_slice(), expected, "read boundary {boundary}");
        }
    }

    #[test]
    fn drawing_and_incomplete_queries_generate_no_input() {
        let mut terminal = TerminalQueries::default();
        let mut replies = Vec::new();
        for byte in b"draw\x1b[0m\x1b[3J\x1b[?u\x1b[6A\x1b]10;title\x1b\\\x1b]10;?\x1b" {
            terminal.respond(&[*byte], &mut replies).unwrap();
        }
        assert!(replies.is_empty());
        terminal.respond(b"[6n", &mut replies).unwrap();
        assert_eq!(replies.as_slice(), b"\x1b[1;1R");
    }
}
