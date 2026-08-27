//! Public types for embedding a Nori ACP session.
//!
//! ACP owns agent/client semantics. Nori adds only the lifecycle and product
//! notifications that ACP does not define.

mod session_event;

pub use agent_client_protocol_schema as acp;
pub use session_event::*;

/// ACP `_meta` key correlating a prompt with user-message chunks that echo it.
pub const PROMPT_ECHO_ID_META_KEY: &str = "nori.dev/promptEchoId";
