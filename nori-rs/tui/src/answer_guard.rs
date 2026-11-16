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

/// Whether `key` picks an option: a digit, Enter, or a plain letter other
/// than the `j`/`k` that move the selection.
fn is_option_key(key: &KeyEvent) -> bool {
    let plain = !key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    match key.code {
        KeyCode::Enter => true,
        KeyCode::Char('j' | 'k') => false,
        KeyCode::Char(c) => plain && c.is_alphanumeric(),
        _ => false,
    }
}

/// The option Esc picks on a permission request: "reject once" when offered,
/// else "reject always". `None` means Esc cancels the request.
pub(crate) fn esc_reject_option(
    options: &[nori_protocol::acp::v1::PermissionOption],
) -> Option<usize> {
    use nori_protocol::acp::v1::PermissionOptionKind;
    options
        .iter()
        .position(|option| option.kind == PermissionOptionKind::RejectOnce)
        .or_else(|| {
            options
                .iter()
                .position(|option| option.kind == PermissionOptionKind::RejectAlways)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn only_keys_that_answer_wait_out_the_grace_period() {
        let clock = ManualClock::new();
        let mut guard = AnswerGuard::new(clock.clock());
        guard.shown();
        let key = |code, modifiers| KeyEvent::new(code, modifiers);
        let held: Vec<bool> = [
            key(KeyCode::Char('1'), KeyModifiers::NONE),
            key(KeyCode::Enter, KeyModifiers::NONE),
            key(KeyCode::Char('y'), KeyModifiers::NONE),
            key(KeyCode::Char('N'), KeyModifiers::SHIFT),
        ]
        .iter()
        .map(|key| guard.allows(key))
        .collect();
        assert_eq!(held, vec![false; 4], "answer keys wait");
        let free: Vec<bool> = [
            key(KeyCode::Esc, KeyModifiers::NONE),
            key(KeyCode::Up, KeyModifiers::NONE),
            key(KeyCode::Char('j'), KeyModifiers::NONE),
            key(KeyCode::Char('k'), KeyModifiers::NONE),
            key(KeyCode::Char('c'), KeyModifiers::CONTROL),
            key(KeyCode::Char('1'), KeyModifiers::ALT),
        ]
        .iter()
        .map(|key| guard.allows(key))
        .collect();
        assert_eq!(free, vec![true; 6], "other keys act at once");
        clock.pass_grace();
        assert!(guard.allows(&key(KeyCode::Char('1'), KeyModifiers::NONE)));
    }
}
