//! Host side of the Agent Client Protocol: agent registry, subprocess
//! connection management (JSON-RPC over stdio) and permission plumbing.
//!
//! Layer 0 of the crate layering (`docs/specs/crate-layering.md`): this crate
//! must stay independent of the session harness and any terminal UI.

pub mod connection;
mod error_category;
pub mod patch;
pub mod registry;

pub use error_category::AcpErrorCategory;
pub use error_category::AcpErrorDetails;
pub use error_category::categorize_acp_error;
pub use error_category::categorize_acp_error_chain;
