use serde::Deserialize;
use serde::Serialize;

use crate::SessionEvent;

/// One session event as it leaves the session manager, tagged with the
/// session it belongs to.
///
/// `seq` starts at 1 for each conversation and increases by one per event
/// with no gaps, so a client following several sessions can sort events by
/// session and detect a dropped event. A fork starts a new conversation and
/// therefore a new sequence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEnvelope {
    /// Nori's conversation ID for the session (see `docs/glossary.md`).
    pub conversation_id: String,
    /// Name of the agent that the session runs on, as in the agent registry.
    pub agent: String,
    /// Per-conversation sequence number, starting at 1.
    pub seq: i64,
    pub event: SessionEvent,
}

/// Coarse status of a managed session, small enough to show in a list row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// The agent process is being prepared or the session is being created.
    Starting,
    /// A turn, load, or cancellation is in progress.
    Working,
    /// The agent is waiting on the user, for example for a permission choice.
    NeedsYou,
    /// The session is idle and accepts a prompt.
    Ready,
    /// The session ended without being asked to, for example because its
    /// agent process died. It can be resumed from its transcript.
    Failed,
    /// The session was shut down, closed, or superseded by a fork.
    Closed,
}
