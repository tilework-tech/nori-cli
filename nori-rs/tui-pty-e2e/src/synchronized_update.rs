//! Synchronized output (DEC mode 2026) for the test terminal.
//!
//! The TUI wraps every frame in `ESC[?2026h` … `ESC[?2026l` so a terminal
//! shows it all at once. Its stdout is line-buffered, so one frame reaches the
//! PTY in several writes, and `vt100` ignores the markers. Without this gate a
//! poll can land inside a frame and the test reads a half-drawn screen.
//!
//! [`SynchronizedUpdates`] holds a frame's bytes until its end marker arrives,
//! including markers split across reads. Like a real terminal, it gives up on
//! a frame that stays open too long ([`FRAME_TIMEOUT`]) or grows too large
//! ([`MAX_FRAME_BYTES`]) and releases what it holds, so a missing end marker
//! cannot freeze the screen.

use std::time::Duration;
use std::time::Instant;

const BEGIN: &[u8] = b"\x1b[?2026h";
const END: &[u8] = b"\x1b[?2026l";

/// How long a frame may stay open before its bytes are applied anyway.
pub const FRAME_TIMEOUT: Duration = Duration::from_secs(1);

/// How many bytes a frame may hold before they are applied anyway.
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

#[derive(Default)]
pub struct SynchronizedUpdates {
    /// Bytes not yet released: an open frame, or a tail that may begin one.
    held: Vec<u8>,
    /// When the open frame began, if one is open.
    frame_opened_at: Option<Instant>,
}

impl SynchronizedUpdates {
    /// Accept `data` read at `now` and return the bytes that may be applied.
    pub fn feed(&mut self, data: &[u8], now: Instant) -> Vec<u8> {
        let mut ready = self.release_stale_frame(now);
        self.held.extend_from_slice(data);
        loop {
            if self.frame_opened_at.is_some() {
                let Some(at) = find_subslice(&self.held, END) else {
                    if self.held.len() > MAX_FRAME_BYTES {
                        ready.append(&mut self.held);
                        self.frame_opened_at = None;
                    }
                    return ready;
                };
                ready.extend(self.held.drain(..at + END.len()));
                self.frame_opened_at = None;
            } else if let Some(at) = find_subslice(&self.held, BEGIN) {
                ready.extend(self.held.drain(..at));
                self.frame_opened_at = Some(now);
            } else {
                let keep = (1..BEGIN.len())
                    .rev()
                    .find(|len| self.held.ends_with(&BEGIN[..*len]))
                    .unwrap_or(0);
                ready.extend(self.held.drain(..self.held.len() - keep));
                return ready;
            }
        }
    }

    /// Release a frame that has been open longer than [`FRAME_TIMEOUT`] at
    /// `now`; otherwise release nothing.
    pub fn release_stale_frame(&mut self, now: Instant) -> Vec<u8> {
        match self.frame_opened_at {
            Some(opened_at) if now.duration_since(opened_at) >= FRAME_TIMEOUT => {
                self.frame_opened_at = None;
                std::mem::take(&mut self.held)
            }
            Some(_) | None => Vec::new(),
        }
    }

    /// Whether a frame has begun and not yet ended.
    pub fn is_frame_open(&self) -> bool {
        self.frame_opened_at.is_some()
    }
}

pub fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn frame(body: &[u8]) -> Vec<u8> {
        [BEGIN, body, END].concat()
    }

    #[test]
    fn bytes_outside_a_frame_pass_through() {
        let mut gate = SynchronizedUpdates::default();
        assert_eq!(gate.feed(b"plain text", Instant::now()), b"plain text");
        assert!(!gate.is_frame_open());
    }

    #[test]
    fn a_frame_is_released_whole_only_once_it_ends() {
        let data = [b"before".as_slice(), &frame(b"frame body"), b"after"].concat();
        let frame_start = b"before".len();
        let frame_end = frame_start + frame(b"frame body").len();
        for split in 0..=data.len() {
            let mut gate = SynchronizedUpdates::default();
            let now = Instant::now();
            let first = gate.feed(&data[..split], now);
            let second = gate.feed(&data[split..], now);

            assert!(
                first.len() <= frame_start || first.len() >= frame_end,
                "split at {split} released part of the frame: {first:?}"
            );
            assert_eq!([first, second].concat(), data, "split at {split}");
            assert!(!gate.is_frame_open(), "split at {split}");
        }
    }

    #[test]
    fn a_frame_is_open_until_its_end_marker_arrives() {
        let mut gate = SynchronizedUpdates::default();
        let now = Instant::now();
        assert_eq!(gate.feed(b"x\x1b[?2026hbody\x1b[?20", now), b"x");
        assert!(gate.is_frame_open());
        assert_eq!(gate.feed(b"26ly", now), b"\x1b[?2026hbody\x1b[?2026ly");
        assert!(!gate.is_frame_open());
    }

    #[test]
    fn a_frame_without_an_end_is_released_after_the_timeout() {
        let mut gate = SynchronizedUpdates::default();
        let opened = Instant::now();
        assert_eq!(gate.feed(b"\x1b[?2026hstuck", opened), b"");

        let early = opened + FRAME_TIMEOUT / 2;
        assert_eq!(gate.release_stale_frame(early), b"");
        assert!(gate.is_frame_open());

        let late = opened + FRAME_TIMEOUT;
        assert_eq!(gate.release_stale_frame(late), b"\x1b[?2026hstuck");
        assert!(!gate.is_frame_open());
    }

    #[test]
    fn new_bytes_after_the_timeout_release_the_stale_frame_first() {
        let mut gate = SynchronizedUpdates::default();
        let opened = Instant::now();
        gate.feed(b"\x1b[?2026hstuck", opened);
        assert_eq!(
            gate.feed(b" more", opened + FRAME_TIMEOUT),
            b"\x1b[?2026hstuck more"
        );
    }

    #[test]
    fn an_oversized_frame_is_released() {
        let mut gate = SynchronizedUpdates::default();
        let body = vec![b'x'; MAX_FRAME_BYTES];
        let released = gate.feed(&[BEGIN, &body].concat(), Instant::now());
        assert_eq!(released.len(), BEGIN.len() + MAX_FRAME_BYTES);
        assert!(!gate.is_frame_open());
    }
}
