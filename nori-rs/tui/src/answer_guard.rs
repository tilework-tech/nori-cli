//! Keeps keystrokes meant for the composer from answering a request that
//! appears while the user is typing. Option keys (digits, Enter and an
//! option's letter shortcut) count only once a request has been visible for
//! [`REQUEST_ANSWER_GRACE`]; Esc, Ctrl+C and navigation always work.

use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

/// How long a newly visible request ignores option keys.
pub(crate) const REQUEST_ANSWER_GRACE: Duration = Duration::from_millis(400);

/// The time source a guard reads, injectable so tests can step time.
#[derive(Clone)]
pub(crate) struct Clock(Arc<dyn Fn() -> Instant + Send + Sync>);

impl Clock {
    pub(crate) fn system() -> Self {
        Self(Arc::new(Instant::now))
    }

    pub(crate) fn now(&self) -> Instant {
        (self.0)()
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::system()
    }
}

impl std::fmt::Debug for Clock {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Clock")
    }
}

/// A clock that moves only when a test advances it.
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct ManualClock(Arc<std::sync::Mutex<Instant>>);

#[cfg(test)]
impl ManualClock {
    pub(crate) fn new() -> Self {
        Self(Arc::new(std::sync::Mutex::new(Instant::now())))
    }

    pub(crate) fn advance(&self, by: Duration) {
        let mut now = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *now += by;
    }

    /// Steps past the grace period of every request shown so far.
    pub(crate) fn pass_grace(&self) {
        self.advance(REQUEST_ANSWER_GRACE + Duration::from_millis(1));
    }

    pub(crate) fn clock(&self) -> Clock {
        let now = self.0.clone();
        Clock(Arc::new(move || {
            *now.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }))
    }
}

/// Tracks when the current request became visible.
#[derive(Clone, Debug, Default)]
pub(crate) struct AnswerGuard {
    clock: Clock,
    shown_at: Option<Instant>,
}

impl AnswerGuard {
    pub(crate) fn new(clock: Clock) -> Self {
        Self {
            clock,
            shown_at: None,
        }
    }

    /// Starts the grace period: a request has just become visible.
    pub(crate) fn shown(&mut self) {
        self.shown_at = Some(self.clock.now());
    }

    /// Whether `key` may act on the request now. Keys that cannot pick an
    /// option always may.
    pub(crate) fn allows(&self, key: &KeyEvent) -> bool {
        !is_option_key(key)
            || self.shown_at.is_none_or(|shown_at| {
                self.clock.now().saturating_duration_since(shown_at) >= REQUEST_ANSWER_GRACE
            })
    }
}

/// Whether `key` picks an option: a digit, Enter, or a plain letter.
fn is_option_key(key: &KeyEvent) -> bool {
    let plain = !key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    match key.code {
        KeyCode::Enter => true,
        KeyCode::Char(c) => plain && c.is_alphanumeric(),
        _ => false,
    }
}
