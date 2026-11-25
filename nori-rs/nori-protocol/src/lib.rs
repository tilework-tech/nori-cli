//! Public types for embedding a Nori ACP session.
//!
//! ACP owns agent/client semantics. Nori adds only the lifecycle and product
//! notifications that ACP does not define.

mod context;
mod delegation;
mod host;
mod pending_request;
mod reply_preview;
mod session_envelope;
mod session_event;

pub use agent_client_protocol_schema as acp;
pub use context::*;
pub use delegation::*;
pub use host::*;
pub use pending_request::PendingRequest;
pub use reply_preview::ReplyPreview;
pub use session_envelope::*;
pub use session_event::*;

/// ACP `_meta` key correlating a prompt with user-message chunks that echo it.
pub const PROMPT_ECHO_ID_META_KEY: &str = "nori.dev/promptEchoId";
