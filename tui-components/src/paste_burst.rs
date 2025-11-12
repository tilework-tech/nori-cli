//! Paste burst detection for distinguishing pasted vs typed input.
//!
//! This module provides [`PasteBurst`] which uses timing heuristics to detect
//! when input is likely pasted rather than typed, allowing special handling
//! for pasted content (e.g., buffering multi-line pastes, suppressing Enter submission).

use std::time::Duration;
use std::time::Instant;

// Heuristic thresholds for detecting paste-like input bursts.
// Detect quickly to avoid showing typed prefix before paste is recognized
const PASTE_BURST_MIN_CHARS: u16 = 3;
const PASTE_BURST_CHAR_INTERVAL: Duration = Duration::from_millis(8);
const PASTE_ENTER_SUPPRESS_WINDOW: Duration = Duration::from_millis(120);

/// State machine for detecting paste-like input bursts based on timing.
///
/// Uses character arrival timing to distinguish between typed and pasted input,
/// enabling special handling for pastes (e.g., buffering multi-line content).
///
/// # Examples
///
/// ```rust
/// use codex_tui_components::paste_burst::{PasteBurst, CharDecision, FlushResult};
/// use std::time::{Duration, Instant};
///
/// let mut detector = PasteBurst::default();
/// let now = Instant::now();
///
/// // Fast successive characters trigger buffering
/// let decision1 = detector.on_plain_char('h', now);
/// let decision2 = detector.on_plain_char('e', now + Duration::from_millis(5));
/// let decision3 = detector.on_plain_char('l', now + Duration::from_millis(10));
/// let decision4 = detector.on_plain_char('l', now + Duration::from_millis(15));
///
/// // After burst detected, subsequent chars are buffered
/// match decision4 {
///     CharDecision::BeginBuffer { .. } => {
///         // Start buffering retroactively
///     }
///     _ => {}
/// }
///
/// // After delay, flush the buffered content
/// let later = now + Duration::from_millis(100);
/// match detector.flush_if_due(later) {
///     FlushResult::Paste(content) => {
///         // Handle pasted content
///     }
///     _ => {}
/// }
/// ```
#[derive(Default)]
pub struct PasteBurst {
    last_plain_char_time: Option<Instant>,
    consecutive_plain_char_burst: u16,
    burst_window_until: Option<Instant>,
    buffer: String,
    active: bool,
    // Hold first fast char briefly to avoid rendering flicker
    pending_first_char: Option<(char, Instant)>,
}

/// Decision on how to handle a plain character based on paste detection.
pub enum CharDecision {
    /// Start buffering and retroactively capture some already-inserted chars.
    BeginBuffer {
        /// Number of characters to grab retroactively from before cursor
        retro_chars: u16,
    },
    /// We are currently buffering; append the current char into the buffer.
    BufferAppend,
    /// Do not insert/render this char yet; temporarily save the first fast
    /// char while we wait to see if a paste-like burst follows.
    RetainFirstChar,
    /// Begin buffering using the previously saved first char (no retro grab needed).
    BeginBufferFromPending,
}

/// Information about retroactively grabbed characters.
pub struct RetroGrab {
    /// Byte index where the grabbed text starts
    pub start_byte: usize,
    /// The grabbed text content
    pub grabbed: String,
}

/// Result of flushing buffered paste detection state.
pub enum FlushResult {
    /// Buffered paste content ready to insert
    Paste(String),
    /// Single typed character (was held briefly, no paste detected)
    Typed(char),
    /// Nothing to flush
    None,
}

impl PasteBurst {
    /// Recommended delay to wait between simulated keypresses (or before
    /// scheduling a UI tick) so that a pending fast keystroke is flushed
    /// out of the burst detector as normal typed input.
    ///
    /// Primarily used by tests and by the TUI to reliably cross the
    /// paste-burst timing threshold.
    pub fn recommended_flush_delay() -> Duration {
        PASTE_BURST_CHAR_INTERVAL + Duration::from_millis(1)
    }

    /// Entry point: decide how to treat a plain char with current timing.
    ///
    /// Call this for each plain (non-modified) character input event.
    /// Returns a [`CharDecision`] indicating how to handle the character.
    pub fn on_plain_char(&mut self, ch: char, now: Instant) -> CharDecision {
        match self.last_plain_char_time {
            Some(prev) if now.duration_since(prev) <= PASTE_BURST_CHAR_INTERVAL => {
                self.consecutive_plain_char_burst =
                    self.consecutive_plain_char_burst.saturating_add(1)
            }
            _ => self.consecutive_plain_char_burst = 1,
        }
        self.last_plain_char_time = Some(now);

        if self.active {
            self.burst_window_until = Some(now + PASTE_ENTER_SUPPRESS_WINDOW);
            return CharDecision::BufferAppend;
        }

        // If we already held a first char and receive a second fast char,
        // start buffering without retro-grabbing (we never rendered the first).
        if let Some((held, held_at)) = self.pending_first_char {
            if now.duration_since(held_at) <= PASTE_BURST_CHAR_INTERVAL {
                self.active = true;
                // take() to clear pending; we already captured the held char above
                let _ = self.pending_first_char.take();
                self.buffer.push(held);
                self.burst_window_until = Some(now + PASTE_ENTER_SUPPRESS_WINDOW);
                return CharDecision::BeginBufferFromPending;
            }
        }

        if self.consecutive_plain_char_burst >= PASTE_BURST_MIN_CHARS {
            return CharDecision::BeginBuffer {
                retro_chars: self.consecutive_plain_char_burst.saturating_sub(1),
            };
        }

        // Save the first fast char very briefly to see if a burst follows.
        self.pending_first_char = Some((ch, now));
        CharDecision::RetainFirstChar
    }

    /// Flushes the buffered burst if the inter-key timeout has elapsed.
    ///
    /// Returns:
    /// - `FlushResult::Paste(String)` when actively buffering paste-like input
    /// - `FlushResult::Typed(char)` when a single fast first-char had no subsequent burst
    /// - `FlushResult::None` if timeout hasn't elapsed or nothing to flush
    pub fn flush_if_due(&mut self, now: Instant) -> FlushResult {
        let timed_out = self
            .last_plain_char_time
            .is_some_and(|t| now.duration_since(t) > PASTE_BURST_CHAR_INTERVAL);
        if timed_out && self.is_active_internal() {
            self.active = false;
            let out = std::mem::take(&mut self.buffer);
            FlushResult::Paste(out)
        } else if timed_out {
            // If we were saving a single fast char and no burst followed,
            // flush it as normal typed input.
            if let Some((ch, _at)) = self.pending_first_char.take() {
                FlushResult::Typed(ch)
            } else {
                FlushResult::None
            }
        } else {
            FlushResult::None
        }
    }

    /// While bursting: accumulate a newline into the buffer instead of
    /// submitting the textarea.
    ///
    /// Returns true if a newline was appended (we are in a burst context),
    /// false otherwise.
    pub fn append_newline_if_active(&mut self, now: Instant) -> bool {
        if self.is_active() {
            self.buffer.push('\n');
            self.burst_window_until = Some(now + PASTE_ENTER_SUPPRESS_WINDOW);
            true
        } else {
            false
        }
    }

    /// Decides if Enter should insert a newline (burst context) vs submit.
    ///
    /// Returns true if currently in a paste burst or recent burst window.
    pub fn newline_should_insert_instead_of_submit(&self, now: Instant) -> bool {
        let in_burst_window = self.burst_window_until.is_some_and(|until| now <= until);
        self.is_active() || in_burst_window
    }

    /// Keeps the burst window alive by extending its timeout.
    pub fn extend_window(&mut self, now: Instant) {
        self.burst_window_until = Some(now + PASTE_ENTER_SUPPRESS_WINDOW);
    }

    /// Begins buffering with retroactively grabbed text.
    ///
    /// Used when paste burst is detected and we need to capture
    /// previously rendered characters into the buffer.
    pub fn begin_with_retro_grabbed(&mut self, grabbed: String, now: Instant) {
        if !grabbed.is_empty() {
            self.buffer.push_str(&grabbed);
        }
        self.active = true;
        self.burst_window_until = Some(now + PASTE_ENTER_SUPPRESS_WINDOW);
    }

    /// Appends a char into the burst buffer.
    pub fn append_char_to_buffer(&mut self, ch: char, now: Instant) {
        self.buffer.push(ch);
        self.burst_window_until = Some(now + PASTE_ENTER_SUPPRESS_WINDOW);
    }

    /// Tries to append a char into the burst buffer only if a burst is already active.
    ///
    /// Returns true when the char was captured into the existing burst, false otherwise.
    pub fn try_append_char_if_active(&mut self, ch: char, now: Instant) -> bool {
        if self.active || !self.buffer.is_empty() {
            self.append_char_to_buffer(ch, now);
            true
        } else {
            false
        }
    }

    /// Decides whether to begin buffering by retroactively capturing recent
    /// chars from the slice before the cursor.
    ///
    /// Heuristic: if the retro-grabbed slice contains any whitespace or is
    /// sufficiently long (>= 16 characters), treat it as paste-like to avoid
    /// rendering the typed prefix momentarily before the paste is recognized.
    /// This favors responsiveness and prevents flicker for typical pastes
    /// (URLs, file paths, multiline text) while not triggering on short words.
    ///
    /// Returns `Some(RetroGrab)` with the start byte and grabbed text when we
    /// decide to buffer retroactively; otherwise `None`.
    pub fn decide_begin_buffer(
        &mut self,
        now: Instant,
        before: &str,
        retro_chars: usize,
    ) -> Option<RetroGrab> {
        let start_byte = retro_start_index(before, retro_chars);
        let grabbed = before[start_byte..].to_string();
        let looks_pastey =
            grabbed.chars().any(char::is_whitespace) || grabbed.chars().count() >= 16;
        if looks_pastey {
            // Note: caller is responsible for removing this slice from UI text.
            self.begin_with_retro_grabbed(grabbed.clone(), now);
            Some(RetroGrab {
                start_byte,
                grabbed,
            })
        } else {
            None
        }
    }

    /// Before applying modified/non-char input: flush buffered burst immediately.
    ///
    /// Returns the buffered content if any, or None.
    pub fn flush_before_modified_input(&mut self) -> Option<String> {
        if !self.is_active() {
            return None;
        }
        self.active = false;
        let mut out = std::mem::take(&mut self.buffer);
        if let Some((ch, _at)) = self.pending_first_char.take() {
            out.push(ch);
        }
        Some(out)
    }

    /// Clears only the timing window and any pending first-char.
    ///
    /// Does not emit or clear the buffered text itself; callers should have
    /// already flushed (if needed) via one of the flush methods above.
    pub fn clear_window_after_non_char(&mut self) {
        self.consecutive_plain_char_burst = 0;
        self.last_plain_char_time = None;
        self.burst_window_until = None;
        self.active = false;
        self.pending_first_char = None;
    }

    /// Returns true if we are in any paste-burst related transient state
    /// (actively buffering, have a non-empty buffer, or have saved the first
    /// fast char while waiting for a potential burst).
    pub fn is_active(&self) -> bool {
        self.is_active_internal() || self.pending_first_char.is_some()
    }

    fn is_active_internal(&self) -> bool {
        self.active || !self.buffer.is_empty()
    }

    /// Clears all paste detection state after an explicit paste event.
    ///
    /// Use this when handling bracketed paste or other explicit paste events
    /// to reset the detector state.
    pub fn clear_after_explicit_paste(&mut self) {
        self.last_plain_char_time = None;
        self.consecutive_plain_char_burst = 0;
        self.burst_window_until = None;
        self.active = false;
        self.buffer.clear();
        self.pending_first_char = None;
    }
}

/// Calculates the byte index to start retroactively grabbing characters.
///
/// Given a string and number of characters to grab from the end,
/// returns the byte index where those characters begin.
pub fn retro_start_index(before: &str, retro_chars: usize) -> usize {
    if retro_chars == 0 {
        return before.len();
    }
    before
        .char_indices()
        .rev()
        .nth(retro_chars.saturating_sub(1))
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}
