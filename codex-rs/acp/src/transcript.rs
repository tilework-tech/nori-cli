//! Project-grouped transcript storage for session history.
//!
//! Transcripts are stored in `~/.nori/cli/projects/<project-key>/<session-id>.jsonl`
//! where:
//! - `<project-key>` is a SHA-256 hash of the project root path (see `project_key` module)
//! - `<session-id>` is the conversation ID for the session
//!
//! Each transcript file is a JSONL file with entries containing:
//! - `session_id`: The conversation/session ID
//! - `ts`: Unix timestamp (seconds since epoch)
//! - `text`: The message text
//! - `role`: Either "user" or "assistant"
//!
//! This enables future loading of session transcripts for session resumption.
//!
//! Additionally, a `manifest.json` file in each project directory stores metadata
//! about the project (path, git info, timestamps) for informational purposes.

use std::fs::File;
use std::fs::OpenOptions;
use std::io::Result;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;

use crate::config::HistoryPersistence;
use codex_protocol::ConversationId;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Directory name for project-grouped transcripts inside nori home.
const PROJECTS_DIR: &str = "projects";

/// Manifest filename in each project directory.
const MANIFEST_FILENAME: &str = "manifest.json";

const MAX_RETRIES: usize = 10;
const RETRY_SLEEP: Duration = Duration::from_millis(100);

/// Role of a message in the transcript.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptRole {
    User,
    Assistant,
}

impl std::fmt::Display for TranscriptRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TranscriptRole::User => write!(f, "user"),
            TranscriptRole::Assistant => write!(f, "assistant"),
        }
    }
}

/// A single entry in a project-grouped transcript file.
///
/// Unlike `HistoryEntry` (which stores only user messages without roles),
/// `TranscriptEntry` includes the role to enable full conversation reconstruction.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TranscriptEntry {
    pub session_id: String,
    pub ts: u64,
    pub text: String,
    pub role: TranscriptRole,
}

/// Project manifest stored in each project directory.
///
/// This provides informational metadata about the project for which
/// transcripts are stored.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ProjectManifest {
    /// The original project path (for informational purposes).
    pub project_path: PathBuf,
    /// Git remote URL if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_remote_url: Option<String>,
    /// Git branch name if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    /// Unix timestamp when this project directory was created.
    pub created_at: u64,
    /// Unix timestamp of the most recent session activity.
    pub last_session_at: u64,
}

/// Get the projects directory path for a given nori_home directory.
pub fn projects_dir(nori_home: &Path) -> PathBuf {
    nori_home.join(PROJECTS_DIR)
}

/// Get the transcript directory path for a specific project.
pub fn transcript_dir(nori_home: &Path, project_key: &str) -> PathBuf {
    projects_dir(nori_home).join(project_key)
}

/// Get the transcript file path for a specific session.
pub fn transcript_filepath(nori_home: &Path, project_key: &str, session_id: &str) -> PathBuf {
    transcript_dir(nori_home, project_key).join(format!("{session_id}.jsonl"))
}

/// Get the manifest file path for a specific project.
pub fn manifest_filepath(nori_home: &Path, project_key: &str) -> PathBuf {
    transcript_dir(nori_home, project_key).join(MANIFEST_FILENAME)
}

/// Append a transcript entry to the session's transcript file.
///
/// Creates the project directory and transcript file if they don't exist.
/// Uses advisory file locking to ensure concurrent writes don't interleave.
///
/// # Arguments
/// * `entry` - The transcript entry to append
/// * `nori_home` - The nori home directory (e.g., `~/.nori/cli`)
/// * `project_key` - The project key (hash of project path)
/// * `persistence` - The history persistence policy
///
/// # Returns
/// `Ok(())` if the entry was successfully appended (or skipped due to persistence policy),
/// or an error if the file operation failed.
pub async fn append_transcript(
    entry: &TranscriptEntry,
    nori_home: &Path,
    project_key: &str,
    persistence: HistoryPersistence,
) -> Result<()> {
    match persistence {
        HistoryPersistence::SaveAll => {
            // Save everything: proceed.
        }
        HistoryPersistence::None => {
            // No history persistence requested.
            return Ok(());
        }
    }

    // Resolve transcript file path and ensure the parent directory exists.
    let dir = transcript_dir(nori_home, project_key);
    tokio::fs::create_dir_all(&dir).await?;

    let path = transcript_filepath(nori_home, project_key, &entry.session_id);

    // Construct the JSON line.
    let mut line = serde_json::to_string(entry)
        .map_err(|e| std::io::Error::other(format!("failed to serialise transcript entry: {e}")))?;
    line.push('\n');

    // Open in append-only mode.
    let mut options = OpenOptions::new();
    options.append(true).read(true).create(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }

    let mut file = options.open(&path)?;

    // Ensure permissions.
    ensure_owner_only_permissions(&file).await?;

    // Perform a blocking write under an advisory write lock.
    tokio::task::spawn_blocking(move || -> Result<()> {
        for _ in 0..MAX_RETRIES {
            match file.try_lock() {
                Ok(()) => {
                    file.write_all(line.as_bytes())?;
                    file.flush()?;
                    return Ok(());
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    std::thread::sleep(RETRY_SLEEP);
                }
                Err(e) => return Err(e.into()),
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "could not acquire exclusive lock on transcript file after multiple attempts",
        ))
    })
    .await??;

    Ok(())
}

/// Update the project manifest with current metadata.
///
/// Creates or updates the manifest.json file in the project directory.
/// If the manifest already exists, updates `last_session_at` timestamp.
///
/// # Arguments
/// * `nori_home` - The nori home directory
/// * `project_key` - The project key
/// * `project_path` - The original project path
/// * `git_remote_url` - Optional git remote URL
/// * `git_branch` - Optional git branch name
pub async fn update_project_manifest(
    nori_home: &Path,
    project_key: &str,
    project_path: &Path,
    git_remote_url: Option<String>,
    git_branch: Option<String>,
) -> Result<()> {
    let dir = transcript_dir(nori_home, project_key);
    tokio::fs::create_dir_all(&dir).await?;

    let manifest_path = manifest_filepath(nori_home, project_key);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| std::io::Error::other(format!("system clock before Unix epoch: {e}")))?
        .as_secs();

    // Try to read existing manifest to preserve created_at.
    let manifest = if manifest_path.exists() {
        let content = tokio::fs::read_to_string(&manifest_path).await?;
        let mut existing: ProjectManifest = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::other(format!("failed to parse manifest: {e}")))?;
        // Update timestamps and git info.
        existing.last_session_at = now;
        if git_remote_url.is_some() {
            existing.git_remote_url = git_remote_url;
        }
        if git_branch.is_some() {
            existing.git_branch = git_branch;
        }
        existing
    } else {
        ProjectManifest {
            project_path: project_path.to_path_buf(),
            git_remote_url,
            git_branch,
            created_at: now,
            last_session_at: now,
        }
    };

    let content = serde_json::to_string_pretty(&manifest)
        .map_err(|e| std::io::Error::other(format!("failed to serialise manifest: {e}")))?;

    // Write atomically by writing to temp file then renaming.
    let temp_path = manifest_path.with_extension("json.tmp");
    tokio::fs::write(&temp_path, content).await?;
    tokio::fs::rename(&temp_path, &manifest_path).await?;

    // Ensure permissions on the manifest file.
    #[cfg(unix)]
    {
        let perms = std::fs::Permissions::from_mode(0o600);
        tokio::fs::set_permissions(&manifest_path, perms).await?;
    }

    Ok(())
}

/// Read the project manifest if it exists.
pub async fn read_project_manifest(
    nori_home: &Path,
    project_key: &str,
) -> Result<Option<ProjectManifest>> {
    let manifest_path = manifest_filepath(nori_home, project_key);
    if !manifest_path.exists() {
        return Ok(None);
    }

    let content = tokio::fs::read_to_string(&manifest_path).await?;
    let manifest: ProjectManifest = serde_json::from_str(&content)
        .map_err(|e| std::io::Error::other(format!("failed to parse manifest: {e}")))?;
    Ok(Some(manifest))
}

/// Create a transcript entry for a user message.
pub fn user_entry(session_id: &ConversationId, text: String) -> TranscriptEntry {
    TranscriptEntry {
        session_id: session_id.to_string(),
        ts: current_timestamp(),
        text,
        role: TranscriptRole::User,
    }
}

/// Create a transcript entry for an assistant message.
pub fn assistant_entry(session_id: &ConversationId, text: String) -> TranscriptEntry {
    TranscriptEntry {
        session_id: session_id.to_string(),
        ts: current_timestamp(),
        text,
        role: TranscriptRole::Assistant,
    }
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Summary information about a session for display in the session picker.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSummary {
    /// The session ID (filename without .jsonl extension).
    pub session_id: String,
    /// Unix timestamp of the first message in the session.
    pub created_at: u64,
    /// Unix timestamp of the last message in the session.
    pub last_activity: u64,
    /// The first user message in the session (truncated for preview).
    pub first_message_preview: String,
    /// Total number of messages in the session.
    pub message_count: usize,
}

/// Maximum length for the first message preview.
const PREVIEW_MAX_LEN: usize = 50;

/// List all sessions for a project, sorted by most recent activity first.
///
/// Returns session summaries including timestamp, first message preview, and message count.
/// Excludes the current session if provided.
///
/// # Arguments
/// * `nori_home` - The nori home directory
/// * `project_key` - The project key
/// * `exclude_session_id` - Optional session ID to exclude (typically the current session)
pub async fn list_project_sessions(
    nori_home: &Path,
    project_key: &str,
    exclude_session_id: Option<&str>,
) -> Result<Vec<SessionSummary>> {
    let dir = transcript_dir(nori_home, project_key);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    let mut entries = tokio::fs::read_dir(&dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();

        // Only process .jsonl files
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }

        // Extract session ID from filename
        let session_id = match path.file_stem().and_then(|s| s.to_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };

        // Skip the excluded session (current session)
        if exclude_session_id == Some(session_id.as_str()) {
            continue;
        }

        // Parse the transcript file to get summary info
        if let Ok(summary) = parse_session_summary(&path, &session_id).await {
            sessions.push(summary);
        }
    }

    // Sort by last_activity descending (most recent first)
    sessions.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));

    Ok(sessions)
}

/// Parse a transcript file to extract summary information.
async fn parse_session_summary(path: &Path, session_id: &str) -> Result<SessionSummary> {
    let content = tokio::fs::read_to_string(path).await?;

    let mut first_message_preview = String::new();
    let mut created_at = u64::MAX;
    let mut last_activity = 0u64;
    let mut message_count = 0usize;

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<TranscriptEntry>(line) {
            Ok(entry) => {
                message_count += 1;

                // Track timestamps
                if entry.ts < created_at {
                    created_at = entry.ts;
                }
                if entry.ts > last_activity {
                    last_activity = entry.ts;
                }

                // Capture first user message for preview
                if first_message_preview.is_empty() && entry.role == TranscriptRole::User {
                    first_message_preview = truncate_preview(&entry.text);
                }
            }
            Err(_) => {
                // Skip malformed entries
                continue;
            }
        }
    }

    // If no messages found, return an error
    if message_count == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "empty or invalid transcript file",
        ));
    }

    // If no user message found, use a placeholder
    if first_message_preview.is_empty() {
        first_message_preview = "(no user messages)".to_string();
    }

    Ok(SessionSummary {
        session_id: session_id.to_string(),
        created_at: if created_at == u64::MAX {
            0
        } else {
            created_at
        },
        last_activity,
        first_message_preview,
        message_count,
    })
}

/// Truncate text for preview display.
fn truncate_preview(text: &str) -> String {
    // Take first line only and truncate
    let first_line = text.lines().next().unwrap_or(text);
    if first_line.len() <= PREVIEW_MAX_LEN {
        first_line.to_string()
    } else {
        // Use char_indices to find a safe UTF-8 boundary
        let truncated: String = first_line
            .char_indices()
            .take_while(|(i, _)| *i < PREVIEW_MAX_LEN - 3)
            .map(|(_, c)| c)
            .collect();
        format!("{truncated}...")
    }
}

/// Load all transcript entries for a session.
///
/// Returns entries in chronological order.
///
/// # Arguments
/// * `nori_home` - The nori home directory
/// * `project_key` - The project key
/// * `session_id` - The session ID to load
pub async fn load_transcript(
    nori_home: &Path,
    project_key: &str,
    session_id: &str,
) -> Result<Vec<TranscriptEntry>> {
    let path = transcript_filepath(nori_home, project_key, session_id);

    if !path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("transcript file not found: {}", path.display()),
        ));
    }

    let content = tokio::fs::read_to_string(&path).await?;
    let mut entries = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<TranscriptEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(_) => {
                // Skip malformed entries but continue parsing
                continue;
            }
        }
    }

    // Sort by timestamp to ensure chronological order
    entries.sort_by_key(|e| e.ts);

    Ok(entries)
}

/// On Unix systems, ensure the file permissions are `0o600` (rw-------).
#[cfg(unix)]
async fn ensure_owner_only_permissions(file: &File) -> Result<()> {
    let metadata = file.metadata()?;
    let current_mode = metadata.permissions().mode() & 0o777;
    if current_mode != 0o600 {
        let mut perms = metadata.permissions();
        perms.set_mode(0o600);
        let perms_clone = perms.clone();
        let file_clone = file.try_clone()?;
        tokio::task::spawn_blocking(move || file_clone.set_permissions(perms_clone)).await??;
    }
    Ok(())
}

#[cfg(windows)]
async fn ensure_owner_only_permissions(_file: &File) -> Result<()> {
    Ok(())
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    #[test]
    fn test_transcript_role_serialization() {
        assert_eq!(
            serde_json::to_string(&TranscriptRole::User).unwrap(),
            "\"user\""
        );
        assert_eq!(
            serde_json::to_string(&TranscriptRole::Assistant).unwrap(),
            "\"assistant\""
        );
    }

    #[test]
    fn test_transcript_role_deserialization() {
        assert_eq!(
            serde_json::from_str::<TranscriptRole>("\"user\"").unwrap(),
            TranscriptRole::User
        );
        assert_eq!(
            serde_json::from_str::<TranscriptRole>("\"assistant\"").unwrap(),
            TranscriptRole::Assistant
        );
    }

    #[test]
    fn test_transcript_entry_serialization() {
        let entry = TranscriptEntry {
            session_id: "test-session".to_string(),
            ts: 1234567890,
            text: "Hello, world!".to_string(),
            role: TranscriptRole::User,
        };

        let json = serde_json::to_string(&entry).unwrap();
        let parsed: TranscriptEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, parsed);
    }

    #[test]
    fn test_transcript_dir_path() {
        let nori_home = Path::new("/home/user/.nori/cli");
        let project_key = "abc123def456";

        let dir = transcript_dir(nori_home, project_key);
        assert_eq!(
            dir,
            PathBuf::from("/home/user/.nori/cli/projects/abc123def456")
        );
    }

    #[test]
    fn test_transcript_filepath() {
        let nori_home = Path::new("/home/user/.nori/cli");
        let project_key = "abc123def456";
        let session_id = "session-001";

        let path = transcript_filepath(nori_home, project_key, session_id);
        assert_eq!(
            path,
            PathBuf::from("/home/user/.nori/cli/projects/abc123def456/session-001.jsonl")
        );
    }

    #[test]
    fn test_manifest_filepath() {
        let nori_home = Path::new("/home/user/.nori/cli");
        let project_key = "abc123def456";

        let path = manifest_filepath(nori_home, project_key);
        assert_eq!(
            path,
            PathBuf::from("/home/user/.nori/cli/projects/abc123def456/manifest.json")
        );
    }

    #[tokio::test]
    async fn test_append_transcript_creates_file() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let conversation_id = ConversationId::new();
        let project_key = "test-project-key";

        let entry = user_entry(&conversation_id, "Hello, world!".to_string());

        append_transcript(
            &entry,
            temp_dir.path(),
            project_key,
            HistoryPersistence::SaveAll,
        )
        .await
        .expect("append transcript");

        // Verify file exists
        let transcript_path =
            transcript_filepath(temp_dir.path(), project_key, &conversation_id.to_string());
        assert!(transcript_path.exists(), "transcript file should exist");

        // Verify content
        let content = std::fs::read_to_string(&transcript_path).expect("read transcript file");
        let parsed: TranscriptEntry = serde_json::from_str(content.trim()).expect("parse entry");
        assert_eq!(parsed.text, "Hello, world!");
        assert_eq!(parsed.role, TranscriptRole::User);
        assert_eq!(parsed.session_id, conversation_id.to_string());
    }

    #[tokio::test]
    async fn test_append_transcript_with_persistence_none_does_nothing() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let conversation_id = ConversationId::new();
        let project_key = "test-project-key";

        let entry = user_entry(&conversation_id, "Should not be saved".to_string());

        append_transcript(
            &entry,
            temp_dir.path(),
            project_key,
            HistoryPersistence::None,
        )
        .await
        .expect("append transcript");

        // Verify file does NOT exist
        let transcript_path =
            transcript_filepath(temp_dir.path(), project_key, &conversation_id.to_string());
        assert!(
            !transcript_path.exists(),
            "transcript file should NOT exist"
        );
    }

    #[tokio::test]
    async fn test_append_multiple_entries() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let conversation_id = ConversationId::new();
        let project_key = "test-project-key";

        // Append user message
        let user = user_entry(&conversation_id, "User message".to_string());
        append_transcript(
            &user,
            temp_dir.path(),
            project_key,
            HistoryPersistence::SaveAll,
        )
        .await
        .expect("append user");

        // Append assistant message
        let assistant = assistant_entry(&conversation_id, "Assistant response".to_string());
        append_transcript(
            &assistant,
            temp_dir.path(),
            project_key,
            HistoryPersistence::SaveAll,
        )
        .await
        .expect("append assistant");

        // Verify content
        let transcript_path =
            transcript_filepath(temp_dir.path(), project_key, &conversation_id.to_string());
        let content = std::fs::read_to_string(&transcript_path).expect("read transcript file");
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 2);

        let entry1: TranscriptEntry = serde_json::from_str(lines[0]).expect("parse entry 1");
        let entry2: TranscriptEntry = serde_json::from_str(lines[1]).expect("parse entry 2");

        assert_eq!(entry1.role, TranscriptRole::User);
        assert_eq!(entry1.text, "User message");
        assert_eq!(entry2.role, TranscriptRole::Assistant);
        assert_eq!(entry2.text, "Assistant response");
    }

    #[tokio::test]
    async fn test_sessions_grouped_by_project() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let conversation_id = ConversationId::new();

        // Different project keys
        let project_key_1 = "project-one";
        let project_key_2 = "project-two";

        let entry1 = user_entry(&conversation_id, "Project 1 message".to_string());
        append_transcript(
            &entry1,
            temp_dir.path(),
            project_key_1,
            HistoryPersistence::SaveAll,
        )
        .await
        .expect("append to project 1");

        let entry2 = user_entry(&conversation_id, "Project 2 message".to_string());
        append_transcript(
            &entry2,
            temp_dir.path(),
            project_key_2,
            HistoryPersistence::SaveAll,
        )
        .await
        .expect("append to project 2");

        // Verify both project directories exist
        let dir1 = transcript_dir(temp_dir.path(), project_key_1);
        let dir2 = transcript_dir(temp_dir.path(), project_key_2);
        assert!(dir1.exists(), "project 1 dir should exist");
        assert!(dir2.exists(), "project 2 dir should exist");
        assert_ne!(dir1, dir2, "project dirs should be different");
    }

    #[tokio::test]
    async fn test_update_project_manifest_creates_file() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let project_key = "test-project-key";
        let project_path = PathBuf::from("/home/user/my-project");

        update_project_manifest(
            temp_dir.path(),
            project_key,
            &project_path,
            Some("https://github.com/user/repo.git".to_string()),
            Some("main".to_string()),
        )
        .await
        .expect("update manifest");

        // Verify manifest exists
        let manifest_path = manifest_filepath(temp_dir.path(), project_key);
        assert!(manifest_path.exists(), "manifest file should exist");

        // Verify content
        let manifest = read_project_manifest(temp_dir.path(), project_key)
            .await
            .expect("read manifest")
            .expect("manifest should exist");

        assert_eq!(manifest.project_path, project_path);
        assert_eq!(
            manifest.git_remote_url,
            Some("https://github.com/user/repo.git".to_string())
        );
        assert_eq!(manifest.git_branch, Some("main".to_string()));
        assert!(manifest.created_at > 0);
        assert_eq!(manifest.created_at, manifest.last_session_at);
    }

    #[tokio::test]
    async fn test_update_project_manifest_updates_existing() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let project_key = "test-project-key";
        let project_path = PathBuf::from("/home/user/my-project");

        // Create initial manifest
        update_project_manifest(temp_dir.path(), project_key, &project_path, None, None)
            .await
            .expect("create manifest");

        let initial = read_project_manifest(temp_dir.path(), project_key)
            .await
            .expect("read manifest")
            .expect("manifest should exist");

        // Small delay to ensure different timestamp
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Update manifest
        update_project_manifest(
            temp_dir.path(),
            project_key,
            &project_path,
            Some("https://github.com/user/repo.git".to_string()),
            Some("develop".to_string()),
        )
        .await
        .expect("update manifest");

        let updated = read_project_manifest(temp_dir.path(), project_key)
            .await
            .expect("read manifest")
            .expect("manifest should exist");

        // created_at should be preserved
        assert_eq!(updated.created_at, initial.created_at);
        // last_session_at should be updated
        assert!(updated.last_session_at >= initial.last_session_at);
        // git info should be updated
        assert_eq!(
            updated.git_remote_url,
            Some("https://github.com/user/repo.git".to_string())
        );
        assert_eq!(updated.git_branch, Some("develop".to_string()));
    }

    #[tokio::test]
    async fn test_read_nonexistent_manifest() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let project_key = "nonexistent-project";

        let result = read_project_manifest(temp_dir.path(), project_key)
            .await
            .expect("read should succeed");

        assert!(
            result.is_none(),
            "should return None for nonexistent manifest"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_transcript_file_permissions_are_0600() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().expect("create temp dir");
        let conversation_id = ConversationId::new();
        let project_key = "test-project-key";

        let entry = user_entry(&conversation_id, "test".to_string());
        append_transcript(
            &entry,
            temp_dir.path(),
            project_key,
            HistoryPersistence::SaveAll,
        )
        .await
        .expect("append transcript");

        let transcript_path =
            transcript_filepath(temp_dir.path(), project_key, &conversation_id.to_string());
        let metadata = std::fs::metadata(&transcript_path).expect("get metadata");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "file permissions should be 0600");
    }

    #[test]
    fn test_user_entry_helper() {
        let conversation_id = ConversationId::new();
        let entry = user_entry(&conversation_id, "Hello".to_string());

        assert_eq!(entry.session_id, conversation_id.to_string());
        assert_eq!(entry.text, "Hello");
        assert_eq!(entry.role, TranscriptRole::User);
        assert!(entry.ts > 0);
    }

    #[test]
    fn test_assistant_entry_helper() {
        let conversation_id = ConversationId::new();
        let entry = assistant_entry(&conversation_id, "Hi there".to_string());

        assert_eq!(entry.session_id, conversation_id.to_string());
        assert_eq!(entry.text, "Hi there");
        assert_eq!(entry.role, TranscriptRole::Assistant);
        assert!(entry.ts > 0);
    }
}
