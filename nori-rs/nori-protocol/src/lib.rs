//! Public types for embedding a Nori ACP session.
//!
//! ACP owns agent/client semantics. Nori adds only the lifecycle and product
//! notifications that ACP does not define.

mod session_event;

pub use agent_client_protocol_schema as acp;
pub use session_event::*;
