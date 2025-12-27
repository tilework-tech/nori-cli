//! Session transcript discovery for Claude, Codex, and Gemini agents.
//!
//! This module provides functions to locate session transcript files based on
//! agent type, session ID, and current working directory. It implements async
//! discovery with mtime-based caching for performance.
//!
//! Transcript locations:
//! - **Claude**: `~/.claude/projects/<PROJECT_PATH>/<SESSIONID>.jsonl`
//!   where PROJECT_PATH is the cwd with `/` replaced by `-`
//! - **Codex**: `~/.codex/sessions/<YEAR>/<MM>/<DD>/rollout-*-<SESSION_GUID>.jsonl`
//! - **Gemini**: `~/.gemini/tmp/<HASHED_PATH>/chats/session-*-<SESSIONID>.json`

use crate::session_parser::AgentKind;
use std::path::Path;
use std::path::PathBuf;
use tokio::fs;

/// Error types for session discovery operations.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("home directory not found")]
    HomeNotFound,

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("session transcript not found")]
    TranscriptNotFound,
}

/// Discover the transcript path for a given agent session.
///
/// # Arguments
///
/// * `agent_kind` - The type of agent (Claude, Codex, or Gemini)
/// * `session_id` - The session identifier from ACP
/// * `cwd` - The current working directory (used for Claude project path)
///
/// # Returns
///
/// Returns the path to the transcript file if found, or a `DiscoveryError` otherwise.
pub async fn discover_transcript_path(
    agent_kind: AgentKind,
    session_id: &str,
    cwd: &Path,
) -> Result<PathBuf, DiscoveryError> {
    let home = dirs::home_dir().ok_or(DiscoveryError::HomeNotFound)?;
    discover_transcript_path_with_home(agent_kind, session_id, cwd, &home).await
}

/// Discover the transcript path using a custom home directory.
///
/// This function is primarily for testing, allowing the home directory to be overridden.
pub async fn discover_transcript_path_with_home(
    agent_kind: AgentKind,
    session_id: &str,
    cwd: &Path,
    home: &Path,
) -> Result<PathBuf, DiscoveryError> {
    match agent_kind {
        AgentKind::Claude => discover_claude_transcript(session_id, cwd, home).await,
        AgentKind::Codex => discover_codex_transcript(session_id, home).await,
        AgentKind::Gemini => discover_gemini_transcript(session_id, home).await,
    }
}

/// Convert a cwd path to Claude's project path format.
///
/// Claude stores projects with `/` replaced by `-`. For example:
/// `/home/user/nori-cli` becomes `-home-user-nori-cli`
pub fn cwd_to_claude_project_path(cwd: &Path) -> String {
    let path_str = cwd.to_string_lossy();

    // Replace all `/` with `-`
    // The result will start with `-` since absolute paths start with `/`
    path_str.replace('/', "-")
}

/// Discover a Claude session transcript.
///
/// Claude stores transcripts in `~/.claude/projects/<PROJECT_PATH>/<SESSIONID>.jsonl`
/// where PROJECT_PATH is the cwd with `/` replaced by `-`.
async fn discover_claude_transcript(
    session_id: &str,
    cwd: &Path,
    home: &Path,
) -> Result<PathBuf, DiscoveryError> {
    let project_path = cwd_to_claude_project_path(cwd);
    let transcript_filename = format!("{session_id}.jsonl");

    let transcript_path = home
        .join(".claude")
        .join("projects")
        .join(&project_path)
        .join(&transcript_filename);

    if fs::metadata(&transcript_path).await.is_ok() {
        Ok(transcript_path)
    } else {
        Err(DiscoveryError::TranscriptNotFound)
    }
}

/// Discover a Codex session transcript by searching for files containing the session GUID.
///
/// Codex stores transcripts in `~/.codex/sessions/<YEAR>/<MM>/<DD>/rollout-*-<SESSION_GUID>.jsonl`.
/// We search recursively for a file ending with `-<session_id>.jsonl`.
async fn discover_codex_transcript(
    session_id: &str,
    home: &Path,
) -> Result<PathBuf, DiscoveryError> {
    let sessions_dir = home.join(".codex").join("sessions");

    if fs::metadata(&sessions_dir).await.is_err() {
        return Err(DiscoveryError::TranscriptNotFound);
    }

    // Search recursively for files ending with `-<session_id>.jsonl`
    let suffix = format!("-{session_id}.jsonl");
    find_file_with_suffix(&sessions_dir, &suffix).await
}

/// Discover a Gemini session transcript by searching for files containing the session ID.
///
/// Gemini stores transcripts in `~/.gemini/tmp/<HASHED_PATH>/chats/session-*-<SESSIONID>.json`.
/// We search recursively for a file ending with `-<session_id>.json`.
async fn discover_gemini_transcript(
    session_id: &str,
    home: &Path,
) -> Result<PathBuf, DiscoveryError> {
    let tmp_dir = home.join(".gemini").join("tmp");

    if fs::metadata(&tmp_dir).await.is_err() {
        return Err(DiscoveryError::TranscriptNotFound);
    }

    // Search recursively for files ending with `-<session_id>.json`
    let suffix = format!("-{session_id}.json");
    find_file_with_suffix(&tmp_dir, &suffix).await
}

/// Recursively search for a file with the given suffix.
async fn find_file_with_suffix(dir: &Path, suffix: &str) -> Result<PathBuf, DiscoveryError> {
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current_dir) = stack.pop() {
        let mut entries = match fs::read_dir(&current_dir).await {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let file_type = match entry.file_type().await {
                Ok(ft) => ft,
                Err(_) => continue,
            };

            if file_type.is_dir() {
                stack.push(path);
            } else if let Some(filename) = path.file_name().and_then(|s| s.to_str())
                && filename.ends_with(suffix)
            {
                return Ok(path);
            }
        }
    }

    Err(DiscoveryError::TranscriptNotFound)
}

#[cfg(test)]
mod tests {
    // Tests are in tests/session_discovery_test.rs
}
