//! Host side of the Agent Client Protocol: agent registry, subprocess
//! connection management (JSON-RPC over stdio), permission plumbing, and
//! translation between ACP wire types and the internal event vocabulary.
//!
//! Layer 0 of the crate layering (`docs/specs/crate-layering.md`): this crate
//! must stay independent of the session harness and any terminal UI.

mod claude_models;
pub mod connection;
mod error_category;
pub mod patch;
pub mod registry;
pub mod translator;

pub use error_category::AcpErrorCategory;
pub use error_category::categorize_acp_error;
