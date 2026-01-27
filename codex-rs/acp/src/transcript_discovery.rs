//! Transcript location discovery for external ACP agents.
//!
//! This module provides functions to detect the current running transcript file
//! for Claude Code, Codex, and Gemini agents. This enables session statistics
//! display (e.g., token usage) in the TUI footer.
//!
//! ## Agent Transcript Locations
//!
//! Each agent stores session transcripts in different locations:
//!
//! - **Claude Code**: `~/.claude/projects/<transformed-path>/<session-uuid>.jsonl`
//!   - Path is transformed by replacing non-alphanumeric chars with dashes
//!   - Example: `/home/user/project` → `-home-user-project`
//!
//! - **Codex**: `~/.codex/sessions/YYYY/MM/DD/rollout-<timestamp>-<uuid>.jsonl`
//!   - Sessions matched by comparing CWD in first JSON line
//!
//! - **Gemini**: `~/.gemini/tmp/<sha256-hash>/chats/<session>.json`
//!   - Hash is SHA256 of the canonical working directory path

use crate::AgentKind;
use sha2::Digest;
use sha2::Sha256;
use std::fs;
use std::io::BufRead;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use thiserror::Error;

/// Information about a discovered transcript location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptLocation {
    /// The agent that created this transcript.
    pub agent_kind: AgentKind,
    /// Path to the transcript file.
    pub transcript_path: PathBuf,
    /// Session identifier (UUID or derived from filename).
    pub session_id: String,
}

/// Errors that can occur during transcript discovery.
#[derive(Debug, Error)]
pub enum DiscoveryError {
    /// No agent environment detected.
    #[error("no agent environment detected")]
    NoAgentDetected,

    /// Agent home directory not found.
    #[error("agent home directory not found: {0}")]
    HomeNotFound(String),

    /// No sessions found for the current working directory.
    #[error("no sessions found for working directory: {0}")]
    NoSessionsFound(PathBuf),

    /// I/O error during discovery.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// JSON parse error (for Codex CWD matching).
    #[error("JSON parse error: {0}")]
    JsonError(#[from] serde_json::Error),
}

/// Detect which agent environment we're running in based on environment variables.
///
/// Returns `Some(AgentKind)` if a known agent environment is detected, `None` otherwise.
pub fn detect_agent_kind() -> Option<AgentKind> {
    // Check for Claude Code environment
    if std::env::var("CLAUDECODE").is_ok() {
        return Some(AgentKind::ClaudeCode);
    }

    // Check for Codex environment (look for CODEX_HOME or other indicators)
    if std::env::var("CODEX_CLI").is_ok() {
        return Some(AgentKind::Codex);
    }

    // Check for Gemini environment
    if std::env::var("GEMINI_CLI").is_ok() {
        return Some(AgentKind::Gemini);
    }

    None
}

/// Discover the current transcript location for the given working directory.
///
/// This is the main entry point for transcript discovery. It detects which agent
/// is running and dispatches to the appropriate discovery function.
///
/// # Arguments
///
/// * `cwd` - The current working directory to find transcripts for
///
/// # Returns
///
/// Returns the discovered transcript location, or an error if no transcript
/// could be found.
pub fn discover_current_transcript(cwd: &Path) -> Result<TranscriptLocation, DiscoveryError> {
    let agent = detect_agent_kind().ok_or(DiscoveryError::NoAgentDetected)?;

    match agent {
        AgentKind::ClaudeCode => find_current_transcript_claude(cwd),
        AgentKind::Codex => find_current_transcript_codex(cwd),
        AgentKind::Gemini => find_current_transcript_gemini(cwd),
    }
}

/// Transform a path to Claude Code's project directory name format.
///
/// Claude Code transforms working directory paths by:
/// 1. Resolving symlinks (if possible)
/// 2. Replacing all non-alphanumeric characters (except `-`) with `-`
/// 3. Adding a leading `-` if not present
///
/// # Example
///
/// `/home/user/my-project` → `-home-user-my-project`
pub fn transform_path_to_claude_project_name(path: &Path) -> String {
    // Try to resolve symlinks, fall back to original path if not possible
    let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    // Convert to string
    let path_str = resolved.to_string_lossy();

    // Replace all non-alphanumeric characters (except -) with -
    let mut transformed: String = path_str
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();

    // Add leading dash if not present
    if !transformed.starts_with('-') {
        transformed.insert(0, '-');
    }

    transformed
}

/// Find the current transcript for Claude Code.
///
/// Looks in `~/.claude/projects/<transformed-path>/` and returns the most
/// recently modified `.jsonl` file.
pub fn find_current_transcript_claude(cwd: &Path) -> Result<TranscriptLocation, DiscoveryError> {
    let home = dirs::home_dir().ok_or_else(|| DiscoveryError::HomeNotFound("~".to_string()))?;

    let project_name = transform_path_to_claude_project_name(cwd);
    let project_dir = home.join(".claude").join("projects").join(&project_name);

    if !project_dir.exists() {
        return Err(DiscoveryError::NoSessionsFound(cwd.to_path_buf()));
    }

    // Find all .jsonl files and get the most recently modified one
    let mut most_recent: Option<(PathBuf, SystemTime)> = None;

    for entry in fs::read_dir(&project_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") && path.is_file() {
            let metadata = entry.metadata()?;
            let modified = metadata.modified()?;

            match &most_recent {
                None => most_recent = Some((path, modified)),
                Some((_, prev_time)) if modified > *prev_time => {
                    most_recent = Some((path, modified));
                }
                _ => {}
            }
        }
    }

    let (transcript_path, _) =
        most_recent.ok_or_else(|| DiscoveryError::NoSessionsFound(cwd.to_path_buf()))?;

    // Extract session ID from filename (UUID before .jsonl)
    let session_id = transcript_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(TranscriptLocation {
        agent_kind: AgentKind::ClaudeCode,
        transcript_path,
        session_id,
    })
}

/// Find the current transcript for Codex.
///
/// Traverses `~/.codex/sessions/YYYY/MM/DD/` and finds sessions where the
/// `cwd` field in the first JSON line matches the provided working directory.
/// Returns the most recently modified matching session.
pub fn find_current_transcript_codex(cwd: &Path) -> Result<TranscriptLocation, DiscoveryError> {
    let home = dirs::home_dir().ok_or_else(|| DiscoveryError::HomeNotFound("~".to_string()))?;
    let sessions_root = home.join(".codex").join("sessions");

    if !sessions_root.exists() {
        return Err(DiscoveryError::NoSessionsFound(cwd.to_path_buf()));
    }

    // Normalize the CWD for comparison
    let normalized_cwd = normalize_path(cwd);

    let mut most_recent: Option<(PathBuf, String, SystemTime)> = None;

    // Traverse year/month/day structure
    for year_entry in read_dir_sorted_desc(&sessions_root)? {
        if !year_entry.path().is_dir() {
            continue;
        }

        for month_entry in read_dir_sorted_desc(&year_entry.path())? {
            if !month_entry.path().is_dir() {
                continue;
            }

            for day_entry in read_dir_sorted_desc(&month_entry.path())? {
                if !day_entry.path().is_dir() {
                    continue;
                }

                for session_entry in read_dir_sorted_desc(&day_entry.path())? {
                    let path = session_entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                        continue;
                    }

                    // Read first line to get session metadata
                    if let Ok(meta) = read_codex_session_meta(&path) {
                        let session_cwd = normalize_path(Path::new(&meta.cwd));

                        if session_cwd == normalized_cwd {
                            let modified = session_entry.metadata()?.modified()?;

                            match &most_recent {
                                None => {
                                    most_recent = Some((path, meta.id, modified));
                                }
                                Some((_, _, prev_time)) if modified > *prev_time => {
                                    most_recent = Some((path, meta.id, modified));
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    let (transcript_path, session_id, _) =
        most_recent.ok_or_else(|| DiscoveryError::NoSessionsFound(cwd.to_path_buf()))?;

    Ok(TranscriptLocation {
        agent_kind: AgentKind::Codex,
        transcript_path,
        session_id,
    })
}

/// Session metadata extracted from the first line of a Codex session file.
#[derive(Debug)]
struct CodexSessionMeta {
    id: String,
    cwd: String,
}

/// Read the session metadata from the first line of a Codex JSONL file.
fn read_codex_session_meta(path: &Path) -> Result<CodexSessionMeta, DiscoveryError> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);

    if let Some(first_line) = reader.lines().next() {
        let line = first_line?;
        let v: serde_json::Value = serde_json::from_str(&line)?;

        // Extract payload.id and payload.cwd
        let id = v
            .get("payload")
            .and_then(|p| p.get("id"))
            .and_then(|i| i.as_str())
            .unwrap_or("")
            .to_string();

        let cwd = v
            .get("payload")
            .and_then(|p| p.get("cwd"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        Ok(CodexSessionMeta { id, cwd })
    } else {
        Err(DiscoveryError::IoError(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "empty session file",
        )))
    }
}

/// Find the current transcript for Gemini.
///
/// Computes SHA256 hash of the canonical path, then looks in
/// `~/.gemini/tmp/<hash>/chats/` for the most recently modified `.json` file.
pub fn find_current_transcript_gemini(cwd: &Path) -> Result<TranscriptLocation, DiscoveryError> {
    let home = dirs::home_dir().ok_or_else(|| DiscoveryError::HomeNotFound("~".to_string()))?;

    // Compute SHA256 hash of the canonical path
    let canonical = fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let path_str = canonical.to_string_lossy();
    let hash = format!("{:x}", Sha256::digest(path_str.as_bytes()));

    let chats_dir = home.join(".gemini").join("tmp").join(&hash).join("chats");

    if !chats_dir.exists() {
        return Err(DiscoveryError::NoSessionsFound(cwd.to_path_buf()));
    }

    // Find the most recently modified .json file
    let mut most_recent: Option<(PathBuf, SystemTime)> = None;

    for entry in fs::read_dir(&chats_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) == Some("json") && path.is_file() {
            let metadata = entry.metadata()?;
            let modified = metadata.modified()?;

            match &most_recent {
                None => most_recent = Some((path, modified)),
                Some((_, prev_time)) if modified > *prev_time => {
                    most_recent = Some((path, modified));
                }
                _ => {}
            }
        }
    }

    let (transcript_path, _) =
        most_recent.ok_or_else(|| DiscoveryError::NoSessionsFound(cwd.to_path_buf()))?;

    // Extract session ID from filename
    let session_id = transcript_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(TranscriptLocation {
        agent_kind: AgentKind::Gemini,
        transcript_path,
        session_id,
    })
}

/// Normalize a path for comparison.
///
/// Cleans the path, converts to absolute, and resolves symlinks if possible.
fn normalize_path(path: &Path) -> PathBuf {
    let cleaned = path.to_path_buf();

    // Try to get absolute path
    let absolute = if cleaned.is_absolute() {
        cleaned
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&cleaned))
            .unwrap_or(cleaned)
    };

    // Try to resolve symlinks
    fs::canonicalize(&absolute).unwrap_or(absolute)
}

/// Read directory entries sorted in descending order by name.
fn read_dir_sorted_desc(path: &Path) -> std::io::Result<Vec<fs::DirEntry>> {
    let mut entries: Vec<_> = fs::read_dir(path)?
        .filter_map(std::result::Result::ok)
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.file_name()));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_transform_path_to_claude_project_name_basic() {
        let path = Path::new("/home/user/my-project");
        let result = transform_path_to_claude_project_name(path);
        assert_eq!(result, "-home-user-my-project");
    }

    #[test]
    fn test_transform_path_to_claude_project_name_special_chars() {
        // Path with special characters should have them replaced with dashes
        let path = Path::new("/home/user/My Projects(1)/app");
        let result = transform_path_to_claude_project_name(path);
        assert_eq!(result, "-home-user-My-Projects-1--app");
    }

    #[test]
    fn test_transform_path_to_claude_project_name_preserves_existing_dashes() {
        let path = Path::new("/home/user/my-cool-project");
        let result = transform_path_to_claude_project_name(path);
        assert_eq!(result, "-home-user-my-cool-project");
    }

    #[test]
    fn test_normalize_path_handles_relative() {
        let path = Path::new("./some/path");
        let normalized = normalize_path(path);
        assert!(normalized.is_absolute());
    }

    #[test]
    fn test_read_codex_session_meta_extracts_fields() {
        let temp_dir = TempDir::new().unwrap();
        let session_file = temp_dir.path().join("test-session.jsonl");

        {
            let mut f = fs::File::create(&session_file).unwrap();
            writeln!(
                f,
                r#"{{"type": "session_meta", "payload": {{"id": "test-id-123", "cwd": "/path/to/project"}}}}"#
            )
            .unwrap();
        }

        let meta = read_codex_session_meta(&session_file).unwrap();
        assert_eq!(meta.id, "test-id-123");
        assert_eq!(meta.cwd, "/path/to/project");
    }
}
