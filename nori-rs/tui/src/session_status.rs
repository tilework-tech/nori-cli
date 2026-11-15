//! Words the TUI uses for session status.
//!
//! The session manager decides each session's status. This module turns
//! statuses into the short words shown in the status line, the session list
//! and the background-request line, and remembers which sessions finished a
//! turn while another session was on screen.

use std::collections::HashMap;
use std::collections::HashSet;

use nori_protocol::SessionStatus;

use crate::app_event::SessionGeneration;

/// The word for a session blocked on the user. Kept in one place so it can be
/// renamed in one line.
pub(crate) const WAITING_ON_YOU_LABEL: &str = "waiting on you";

/// Name of the session-list filter that shows only sessions blocked on the
/// user.
pub(crate) const WAITING_FILTER_LABEL: &str = "waiting on you";

/// Suffix for a session that finished in the background and has not been
/// viewed since.
const NEW_SUFFIX: &str = " · new";

/// Status in the order the status line lists it: what needs the user first.
const SUMMARY_ORDER: [SessionStatus; 5] = [
    SessionStatus::WaitingOnYou,
    SessionStatus::Failed,
    SessionStatus::Working,
    SessionStatus::Starting,
    SessionStatus::Ready,
];

pub(crate) fn status_label(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::Starting => "starting",
        SessionStatus::Working => "working",
        SessionStatus::WaitingOnYou => WAITING_ON_YOU_LABEL,
        SessionStatus::Ready => "ready",
        SessionStatus::Failed => "failed",
        SessionStatus::Closed => "closed",
        SessionStatus::Done => "done",
    }
}

/// The color of a status word and glyph in session lists: amber for what
/// needs the user, cyan for work in progress, green for ready, red for
/// failed.
pub(crate) fn status_tone(status: SessionStatus) -> nori_tui_components::DetailTone {
    use nori_tui_components::DetailTone;
    match status {
        SessionStatus::WaitingOnYou => DetailTone::Warning,
        SessionStatus::Working => DetailTone::Info,
        SessionStatus::Ready => DetailTone::Success,
        SessionStatus::Failed => DetailTone::Error,
        SessionStatus::Closed | SessionStatus::Done | SessionStatus::Starting => DetailTone::Muted,
    }
}

/// The status word for one session row. A ready session that finished while
/// another session was on screen reads `ready · new` until it is viewed.
pub(crate) fn status_word(status: SessionStatus, unseen_finish: bool) -> String {
    let label = status_label(status);
    if unseen_finish && status == SessionStatus::Ready {
        format!("{label}{NEW_SUFFIX}")
    } else {
        label.to_string()
    }
}

/// Counts of sessions per status, such as `1 waiting on you · 2 working`.
/// Closed sessions are left out. Returns `None` when nothing is counted.
pub(crate) fn status_summary(statuses: impl IntoIterator<Item = SessionStatus>) -> Option<String> {
    let statuses: Vec<SessionStatus> = statuses.into_iter().collect();
    let parts: Vec<String> = SUMMARY_ORDER
        .iter()
        .filter_map(|wanted| {
            let count = statuses.iter().filter(|status| *status == wanted).count();
            (count > 0).then(|| format!("{count} {}", status_label(*wanted)))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// Remembers which sessions finished a turn while they were not on screen.
#[derive(Default)]
pub(crate) struct UnseenFinishes {
    last_status: HashMap<SessionGeneration, SessionStatus>,
    unseen: HashSet<SessionGeneration>,
}

impl UnseenFinishes {
    /// Record a session's current status. A session that goes from busy to
    /// ready while off screen becomes unseen; a session on screen is seen.
    pub(crate) fn observe(
        &mut self,
        generation: SessionGeneration,
        status: SessionStatus,
        on_screen: bool,
    ) {
        let previous = self.last_status.insert(generation, status);
        if on_screen {
            self.unseen.remove(&generation);
            return;
        }
        let was_busy = matches!(
            previous,
            Some(SessionStatus::Working | SessionStatus::WaitingOnYou)
        );
        if was_busy && status == SessionStatus::Ready {
            self.unseen.insert(generation);
        } else if status != SessionStatus::Ready {
            self.unseen.remove(&generation);
        }
    }

    pub(crate) fn is_unseen(&self, generation: SessionGeneration) -> bool {
        self.unseen.contains(&generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn summary_lists_what_needs_the_user_first_and_skips_closed() {
        assert_eq!(
            status_summary([
                SessionStatus::Ready,
                SessionStatus::Working,
                SessionStatus::Closed,
                SessionStatus::WaitingOnYou,
                SessionStatus::Working,
            ]),
            Some("1 waiting on you · 2 working · 1 ready".to_string())
        );
        assert_eq!(
            status_summary([SessionStatus::Working, SessionStatus::Ready]),
            Some("1 working · 1 ready".to_string())
        );
        assert_eq!(status_summary([SessionStatus::Closed]), None);
    }

    #[test]
    fn a_background_finish_reads_new_until_the_session_is_viewed() {
        let mut finishes = UnseenFinishes::default();
        finishes.observe(1, SessionStatus::Working, false);
        finishes.observe(1, SessionStatus::Ready, false);
        let before = status_word(SessionStatus::Ready, finishes.is_unseen(1));
        finishes.observe(1, SessionStatus::Ready, true);
        let after = status_word(SessionStatus::Ready, finishes.is_unseen(1));
        assert_eq!((before.as_str(), after.as_str()), ("ready · new", "ready"));
    }

    #[test]
    fn a_finish_on_screen_or_a_session_that_was_never_busy_is_not_new() {
        let mut finishes = UnseenFinishes::default();
        finishes.observe(1, SessionStatus::Working, true);
        finishes.observe(1, SessionStatus::Ready, true);
        finishes.observe(2, SessionStatus::Starting, false);
        finishes.observe(2, SessionStatus::Ready, false);
        assert_eq!(
            (finishes.is_unseen(1), finishes.is_unseen(2)),
            (false, false)
        );
    }

    #[test]
    fn starting_a_new_turn_clears_new() {
        let mut finishes = UnseenFinishes::default();
        finishes.observe(1, SessionStatus::Working, false);
        finishes.observe(1, SessionStatus::Ready, false);
        finishes.observe(1, SessionStatus::Working, false);
        assert!(!finishes.is_unseen(1));
    }
}
