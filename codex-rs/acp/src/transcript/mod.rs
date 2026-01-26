//! Nori transcript persistence for ACP sessions.
//!
//! This module provides client-side transcript storage that captures what the user
//! saw during a conversation - user messages, assistant responses, and tool executions.
//! Transcripts are organized by git project and can be reloaded for viewing.

mod loader;
mod project;
mod recorder;
mod types;

pub use loader::ProjectInfo;
pub use loader::SessionInfo;
pub use loader::Transcript;
pub use loader::TranscriptLoader;
pub use project::ProjectId;
pub use project::compute_project_id;
pub use recorder::TranscriptRecorder;
pub use types::AssistantEntry;
pub use types::ContentBlock;
pub use types::SessionMetaEntry;
pub use types::ToolCallEntry;
pub use types::ToolResultEntry;
pub use types::TranscriptEntry;
pub use types::TranscriptLine;
pub use types::UserEntry;

#[cfg(test)]
mod tests;
