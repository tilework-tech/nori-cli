//! Jiro's face on the interactive welcome header.
//!
//! The face `〉_ ▐` stands in for the title while the default agent session
//! starts. The whole face is green. It opens with its eyes closed, opens
//! them, blinks a few times in quick succession (design B3: both eyes squash
//! together at 80 ms per frame), then holds still with its eyes open for as
//! long as the session takes. When the session is ready it winks once (W1: the
//! right eye becomes a dash for 240 ms) and the mouth types the wordmark
//! `〉nori CLI` and then the dim version, pushing the right eye along (R1, at
//! 30 ms per letter).
//!
//! The timeline is a pure function of the time elapsed and of when readiness
//! arrived, so every frame can be checked with a fixed clock. Readiness never
//! waits on the face: a blink in progress finishes, later blinks are dropped,
//! and the finale starts at once. A face frozen before readiness keeps its
//! eyes open and never winks.
//!
//! Every face frame is five cells wide: `〉` (U+3009) takes two.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::Duration;
use std::time::Instant;

use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

const WORD: &str = "nori CLI";
const CLOSED_AT_START: Duration = Duration::from_millis(400);
const BLINK_FRAME: Duration = Duration::from_millis(80);
const OPEN_BETWEEN_BLINKS: Duration = Duration::from_millis(400);
const OPEN_BEFORE_WINK: Duration = Duration::from_millis(120);
const WINK: Duration = Duration::from_millis(240);
const OPEN_BEFORE_TYPING: Duration = Duration::from_millis(200);
const LETTER: Duration = Duration::from_millis(30);
const TYPED_HOLD: Duration = Duration::from_millis(240);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    Open,
    Squashed,
    Closed,
    Wink,
    /// The first `n` letters of the wordmark and version, with the mouth as
    /// the cursor.
    Typing(usize),
    Wordmark,
}

#[derive(Debug)]
struct Timeline {
    start: Instant,
    /// The name, then the version: everything the mouth types.
    typed: String,
    blinks: i32,
    /// When the finale starts, measured from `start`.
    finale_at: Option<Duration>,
    frozen: Option<Frame>,
    current: Frame,
}

impl Timeline {
    /// The end of the opening, measured from `start`.
    fn opened_at(&self) -> Duration {
        CLOSED_AT_START + BLINK_FRAME
    }

    /// The start of blink `index` (zero-based), measured from `start`.
    fn blink_at(&self, index: i32) -> Duration {
        let index = u32::try_from(index).unwrap_or_default();
        self.opened_at() + OPEN_BETWEEN_BLINKS * (index + 1) + BLINK_FRAME * 3 * index
    }

    /// The frame before readiness, ignoring any finale.
    fn waiting_frame(&self, elapsed: Duration) -> Frame {
        if elapsed < CLOSED_AT_START {
            return Frame::Closed;
        }
        if elapsed < self.opened_at() {
            return Frame::Squashed;
        }
        for index in 0..self.blinks {
            let at = self.blink_at(index);
            if elapsed >= at && elapsed < at + BLINK_FRAME * 3 {
                return if elapsed < at + BLINK_FRAME || elapsed >= at + BLINK_FRAME * 2 {
                    Frame::Squashed
                } else {
                    Frame::Closed
                };
            }
        }
        Frame::Open
    }

    /// When readiness at `elapsed` may start the finale: once the eyes are
    /// open, so a blink in progress finishes and a face still closed opens.
    fn finale_start(&self, elapsed: Duration) -> Duration {
        if elapsed < CLOSED_AT_START {
            return elapsed + BLINK_FRAME;
        }
        if elapsed < self.opened_at() {
            return self.opened_at();
        }
        (0..self.blinks)
            .map(|index| self.blink_at(index))
            .find(|at| elapsed >= *at && elapsed < *at + BLINK_FRAME * 3)
            .map_or(elapsed, |at| at + BLINK_FRAME * 3)
    }

    /// Before the finale, a face that skipped its closed hold squashes once
    /// on the way to open.
    fn opening_frame(&self, elapsed: Duration, finale_at: Duration) -> Frame {
        if finale_at < self.opened_at() && elapsed + BLINK_FRAME >= finale_at {
            Frame::Squashed
        } else {
            self.waiting_frame(elapsed)
        }
    }

    /// The finale `since` after it started, and how long that frame lasts.
    fn finale_frame(&self, since: Duration) -> (Frame, Option<Duration>) {
        let mut at = Duration::ZERO;
        let mut steps = vec![
            (Frame::Open, OPEN_BEFORE_WINK),
            (Frame::Wink, WINK),
            (Frame::Open, OPEN_BEFORE_TYPING),
        ];
        let letters_total = self.typed.chars().count();
        for letters in 1..=letters_total {
            let hold = if letters == letters_total {
                LETTER + TYPED_HOLD
            } else {
                LETTER
            };
            steps.push((Frame::Typing(letters), hold));
        }
        for (frame, length) in steps {
            if since < at + length {
                return (frame, Some(at + length - since));
            }
            at += length;
        }
        (Frame::Wordmark, None)
    }

    /// How long until the waiting face next changes, ignoring any finale.
    fn waiting_change(&self, elapsed: Duration) -> Option<Duration> {
        [CLOSED_AT_START, self.opened_at()]
            .into_iter()
            .chain((0..self.blinks).flat_map(|index| {
                let at = self.blink_at(index);
                [
                    at,
                    at + BLINK_FRAME,
                    at + BLINK_FRAME * 2,
                    at + BLINK_FRAME * 3,
                ]
            }))
            .find(|at| *at > elapsed)
            .map(|at| at - elapsed)
    }

    /// The frame at `elapsed`, and how long until it changes.
    fn frame_at(&self, elapsed: Duration) -> (Frame, Option<Duration>) {
        if let Some(frame) = self.frozen {
            return (frame, None);
        }
        if let Some(finale_at) = self.finale_at {
            if elapsed >= finale_at {
                return self.finale_frame(elapsed - finale_at);
            }
            let until_finale = finale_at - elapsed;
            let change = self
                .waiting_change(elapsed)
                .map_or(until_finale, |change| change.min(until_finale));
            return (self.opening_frame(elapsed, finale_at), Some(change));
        }
        (self.waiting_frame(elapsed), self.waiting_change(elapsed))
    }
}

/// A shared handle to the face: the welcome header renders it and the app
/// drives it.
#[derive(Debug, Clone)]
pub(crate) struct StartupFace(Arc<Mutex<Timeline>>);

impl StartupFace {
    /// A face that starts closed at `start`, blinks `blinks` times, and types
    /// `version` after the name.
    pub(crate) fn new(start: Instant, blinks: i32, version: &str) -> Self {
        Self(Arc::new(Mutex::new(Timeline {
            start,
            typed: format!("{WORD} v{version}"),
            blinks,
            finale_at: None,
            frozen: None,
            current: Frame::Closed,
        })))
    }

    /// The static wordmark, for when animations are off.
    pub(crate) fn settled(version: &str) -> Self {
        let face = Self::new(Instant::now(), 0, version);
        face.with(|timeline| {
            timeline.frozen = Some(Frame::Wordmark);
            timeline.current = Frame::Wordmark;
        });
        face
    }

    fn with<T>(&self, f: impl FnOnce(&mut Timeline) -> T) -> T {
        f(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The default session is ready. Only the first call counts.
    pub(crate) fn ready(&self, now: Instant) {
        self.with(|timeline| {
            if timeline.finale_at.is_some() || timeline.frozen.is_some() {
                return;
            }
            let elapsed = now.saturating_duration_since(timeline.start);
            timeline.finale_at = Some(timeline.finale_start(elapsed));
        });
    }

    /// Stop moving: settle on the wordmark when the session is ready, and on
    /// the open face when it is not.
    pub(crate) fn freeze(&self) {
        self.with(|timeline| {
            if timeline.frozen.is_none() {
                let frame = if timeline.finale_at.is_some() {
                    Frame::Wordmark
                } else {
                    Frame::Open
                };
                timeline.frozen = Some(frame);
                timeline.current = frame;
            }
        });
    }

    /// Show the frame for `now`. Returns how long until the next frame, or
    /// `None` when the face is holding still.
    pub(crate) fn tick(&self, now: Instant) -> Option<Duration> {
        self.with(|timeline| {
            let (frame, change) = timeline.frame_at(now.saturating_duration_since(timeline.start));
            timeline.current = frame;
            change
        })
    }

    /// The face has settled for good: the wordmark is showing, or it froze.
    pub(crate) fn is_finished(&self) -> bool {
        self.with(|timeline| timeline.frozen.is_some() || timeline.current == Frame::Wordmark)
    }

    /// The title line for the frame last shown.
    pub(crate) fn title_line(&self) -> Line<'static> {
        let (frame, typed) = self.with(|timeline| (timeline.current, timeline.typed.clone()));
        let face = |left: &'static str, right: &'static str| -> Vec<Span<'static>> {
            vec![left.green(), "_".green(), " ".into(), right.green()]
        };
        // The name is bold and the version after it dim.
        let wordmark = |letters: usize| -> Vec<Span<'static>> {
            let shown: String = typed.chars().take(letters).collect();
            let split = shown.len().min(WORD.len());
            let (name, version) = shown.split_at(split);
            vec![name.to_string().bold(), version.to_string().dim()]
        };
        let mut spans: Vec<Span<'static>> = vec!["  ".into()];
        match frame {
            Frame::Open => spans.extend(face("〉", "▐")),
            Frame::Squashed => spans.extend(face("› ", "▗")),
            Frame::Closed => spans.extend(face("- ", "▁")),
            Frame::Wink => spans.extend(face("〉", "-")),
            Frame::Typing(letters) => {
                spans.push("〉".green());
                spans.extend(wordmark(letters));
                spans.extend(["_".green(), " ".into(), "▐".green()]);
            }
            Frame::Wordmark => {
                spans.push("〉".green());
                spans.extend(wordmark(typed.chars().count()));
            }
        }
        spans.retain(|span| !span.content.is_empty());
        spans.into()
    }
}

#[cfg(test)]
mod tests;
