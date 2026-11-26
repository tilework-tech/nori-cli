//! Program status reports for the terminal (OSC 7501, Program Status Protocol
//! rev 0.4).
//!
//! The TUI tells its own terminal what the foreground session is doing so a
//! tab indicator or notification can show it without the user looking at the
//! tab. Two halves keep host state and terminal output apart:
//!
//! - [`ProgramStatusTracker`] folds one session's host events into a
//!   [`ProgramStatus`]. Both frontends feed it the same envelopes the host
//!   publishes: plain `nori` per `ChatWidget`, `nori attach` per conversation.
//! - [`ProgramStatusReporter`] encodes a status as an OSC 7501 report and
//!   writes it only when the bytes would change, then clears the status when
//!   the frontend exits or detaches.
//!
//! Reports address the root record (no `id` key) because tmux drops reports
//! that carry one, and they never carry a `title` key so the tab title stays
//! the agent's name.

mod report;
mod tracker;

pub(crate) use report::ProgramStatusReporter;
pub(crate) use report::clear_after_panic;
pub(crate) use tracker::ProgramStatusTracker;

/// What a session is doing, as the terminal is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProgramState {
    /// No turn has run yet, or the last one was cancelled.
    Idle,
    /// A turn is running.
    Working,
    /// A permission request waits for an answer.
    Blocked,
    /// The last turn finished.
    Done,
    /// The last turn failed, or the agent stopped unexpectedly.
    Error,
}

/// One session's status: its state and, while blocked, the title of the
/// oldest open permission request when the agent supplied one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProgramStatus {
    pub state: ProgramState,
    pub approval_title: Option<String>,
}

impl ProgramStatus {
    pub(crate) const IDLE: Self = Self {
        state: ProgramState::Idle,
        approval_title: None,
    };
}
