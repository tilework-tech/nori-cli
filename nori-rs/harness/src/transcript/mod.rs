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

/// Subdirectory for transcripts within NORI_HOME
pub(crate) const TRANSCRIPTS_DIR: &str = "transcripts";
/// Subdirectory for project-organized transcripts
pub(crate) const BY_PROJECT_DIR: &str = "by-project";
/// Subdirectory for session files within a project
pub(crate) const SESSIONS_DIR: &str = "sessions";
/// Project metadata filename
pub(crate) const PROJECT_METADATA_FILE: &str = "project.json";

pub use loader::ProjectInfo;
pub use loader::SessionInfo;
pub use loader::SessionMetadata;
pub use loader::Transcript;
pub use loader::TranscriptLoader;
pub use loader::TranscriptRecord;
pub(crate) use loader::read_seed_entries;
pub use project::ProjectId;
pub use project::compute_project_id;
pub use recorder::TranscriptRecorder;
pub use types::Attachment;
pub(crate) use types::ContentBlock;
pub use types::GitInfo;
pub use types::SessionMetaEntry;
pub(crate) use types::TranscriptEntry;

#[cfg(test)]
pub(crate) use types::AssistantEntry;
#[cfg(test)]
pub(crate) use types::ClientEventEntry;
#[cfg(test)]
pub(crate) use types::PatchApplyEntry;
#[cfg(test)]
pub(crate) use types::PatchOperationType;
#[cfg(test)]
pub(crate) use types::SessionEventEntry;
#[cfg(test)]
pub(crate) use types::ToolCallEntry;
#[cfg(test)]
pub(crate) use types::ToolResultEntry;
#[cfg(test)]
pub(crate) use types::TranscriptLine;
#[cfg(test)]
pub(crate) use types::UserEntry;

#[cfg(test)]
mod tests;
