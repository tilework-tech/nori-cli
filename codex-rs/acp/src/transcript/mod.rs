//! Nori transcript persistence system.
//!
//! This module provides transcript recording and loading for Nori CLI sessions.
//! Transcripts capture the client-side view of conversations - what the user typed
//! and what the assistant responded with - stored in JSONL format.
//!
//! ## Storage Structure
//!
//! Transcripts are organized by project:
//! ```text
//! $NORI_HOME/transcripts/by-project/{project-id}/
//!   ├── project.json           # Project metadata
//!   └── sessions/
//!       └── {session-id}.jsonl # Individual session transcript
//! ```

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
pub use types::Attachment;
pub use types::ContentBlock;
pub use types::GitInfo;
pub use types::SessionMetaEntry;
pub use types::ToolCallEntry;
pub use types::ToolResultEntry;
pub use types::TranscriptEntry;
pub use types::TranscriptLine;
pub use types::UserEntry;

#[cfg(test)]
mod tests;
