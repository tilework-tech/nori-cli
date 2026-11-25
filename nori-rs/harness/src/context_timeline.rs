//! Context usage observations at Nori prompt boundaries.

use nori_protocol::acp::v1 as acp;

mod reducer;
pub(crate) use reducer::ContextTimelineState;
pub(crate) use reducer::is_timeline_evidence;

pub use nori_protocol::ContextCheckpoint;
pub use nori_protocol::ContextObservation;
pub use nori_protocol::ContextSegment;
pub use nori_protocol::ContextTimeline;
pub use nori_protocol::ContextTurn;
