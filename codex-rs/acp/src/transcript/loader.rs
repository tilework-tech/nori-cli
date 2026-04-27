//! TranscriptLoader - Loads and lists transcripts for viewing.

use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use serde::Deserialize;
use serde::Serialize;
use tokio::io::AsyncBufReadExt;

use super::BY_PROJECT_DIR;
use super::PROJECT_METADATA_FILE;
use super::SESSIONS_DIR;
use super::TRANSCRIPTS_DIR;
use super::project::compute_project_id;
use super::types::SessionMetaEntry;
use super::types::TranscriptEntry;
use super::types::TranscriptLine;

/// Information about a project with transcripts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    /// Hash-based project identifier
    pub id: String,
    /// Human-readable project name
    pub name: String,
    /// Git remote URL if available
    pub git_remote: Option<String>,
    /// Working directory
    pub cwd: PathBuf,
    /// Number of sessions in this project
    pub session_count: usize,
    /// Timestamp of most recent session
    pub last_session_at: Option<String>,
}

/// Information about a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Session identifier (UUID)
    pub session_id: String,
    /// Project identifier
    pub project_id: String,
    /// When the session started
    pub started_at: String,
    /// Working directory for the session
    pub cwd: PathBuf,
    /// ACP agent used for the session (e.g., "claude-code", "codex", "gemini")
    pub agent: Option<String>,
    /// Number of entries in the transcript (approximate)
    pub entry_count: usize,
}

/// A loaded transcript with all entries.
#[derive(Debug, Clone)]
pub struct Transcript {
    /// Session metadata (first entry)
    pub meta: SessionMetaEntry,
    /// All entries including the metadata
    pub entries: Vec<TranscriptLine>,
}

/// Loads and lists transcripts for viewing.
pub struct TranscriptLoader {
    nori_home: PathBuf,
}

const TRANSCRIPT_LOAD_PROGRESS_BYTES: usize = 100 * 1024 * 1024;

impl TranscriptLoader {
    /// Create a new TranscriptLoader.
    pub fn new(nori_home: PathBuf) -> Self {
        Self { nori_home }
    }

    /// Get the base path for transcripts.
    fn transcripts_base(&self) -> PathBuf {
        self.nori_home.join(TRANSCRIPTS_DIR).join(BY_PROJECT_DIR)
    }

    /// List all projects that have transcripts.
    pub async fn list_projects(&self) -> io::Result<Vec<ProjectInfo>> {
        let base_path = self.transcripts_base();

        if !base_path.exists() {
            return Ok(Vec::new());
        }

        let mut projects = Vec::new();
        let mut read_dir = tokio::fs::read_dir(&base_path).await?;

        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            // Try to read project metadata
            let meta_path = path.join(PROJECT_METADATA_FILE);
            if let Ok(content) = tokio::fs::read_to_string(&meta_path).await
                && let Ok(meta) = serde_json::from_str::<serde_json::Value>(&content)
            {
                // Count sessions
                let sessions_path = path.join(SESSIONS_DIR);
                let session_count = count_sessions(&sessions_path).await.unwrap_or(0);

                // Get last session timestamp
                let last_session_at = get_last_session_timestamp(&sessions_path).await;

                projects.push(ProjectInfo {
                    id: meta["id"].as_str().unwrap_or_default().to_string(),
                    name: meta["name"].as_str().unwrap_or_default().to_string(),
                    git_remote: meta["git_remote"].as_str().map(String::from),
                    cwd: PathBuf::from(meta["cwd"].as_str().unwrap_or_default()),
                    session_count,
                    last_session_at,
                });
            }
        }

        // Sort by last session timestamp (most recent first)
        projects.sort_by(|a, b| b.last_session_at.as_ref().cmp(&a.last_session_at.as_ref()));

        Ok(projects)
    }

    /// List all sessions for a specific project.
    pub async fn list_sessions(&self, project_id: &str) -> io::Result<Vec<SessionInfo>> {
        let started = Instant::now();
        let sessions_path = self.transcripts_base().join(project_id).join(SESSIONS_DIR);

        if !sessions_path.exists() {
            tracing::info!(
                target: "nori_resume",
                phase = "transcript_loader.list_sessions.missing_dir",
                project_id,
                sessions_path = %sessions_path.display(),
                elapsed_ms = started.elapsed().as_millis(),
                "no transcript sessions directory found",
            );
            return Ok(Vec::new());
        }

        tracing::info!(
            target: "nori_resume",
            phase = "transcript_loader.list_sessions.start",
            project_id,
            sessions_path = %sessions_path.display(),
            "listing transcript sessions",
        );

        let mut sessions = Vec::new();
        let mut read_dir = tokio::fs::read_dir(&sessions_path).await?;
        let mut file_count = 0usize;

        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "jsonl") {
                file_count += 1;
                match load_session_info(&path, project_id).await {
                    Ok(info) => sessions.push(info),
                    Err(error) => tracing::warn!(
                        target: "nori_resume",
                        phase = "transcript_loader.list_sessions.session_info_error",
                        project_id,
                        path = %path.display(),
                        error = %error,
                        "failed to load transcript session info",
                    ),
                }
            }
        }

        // Sort by started_at (most recent first)
        sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));

        tracing::info!(
            target: "nori_resume",
            phase = "transcript_loader.list_sessions.done",
            project_id,
            file_count,
            loaded_session_count = sessions.len(),
            elapsed_ms = started.elapsed().as_millis(),
            "finished listing transcript sessions",
        );

        Ok(sessions)
    }

    /// Find sessions for the current working directory.
    /// Useful for showing "recent sessions in this project".
    pub async fn find_sessions_for_cwd(&self, cwd: &Path) -> io::Result<Vec<SessionInfo>> {
        let started = Instant::now();
        tracing::info!(
            target: "nori_resume",
            phase = "transcript_loader.find_sessions_for_cwd.start",
            cwd = %cwd.display(),
            "finding transcript sessions for cwd",
        );

        // Compute project ID for the cwd
        let project_started = Instant::now();
        let project_id = compute_project_id(cwd).await?;
        tracing::info!(
            target: "nori_resume",
            phase = "transcript_loader.find_sessions_for_cwd.project_id",
            cwd = %cwd.display(),
            project_id = %project_id.id,
            project_name = %project_id.name,
            git_remote = project_id.git_remote.as_deref().unwrap_or("<none>"),
            elapsed_ms = project_started.elapsed().as_millis(),
            total_elapsed_ms = started.elapsed().as_millis(),
            "computed transcript project id",
        );

        let list_started = Instant::now();
        let sessions = self.list_sessions(&project_id.id).await?;
        tracing::info!(
            target: "nori_resume",
            phase = "transcript_loader.find_sessions_for_cwd.done",
            cwd = %cwd.display(),
            project_id = %project_id.id,
            session_count = sessions.len(),
            list_elapsed_ms = list_started.elapsed().as_millis(),
            total_elapsed_ms = started.elapsed().as_millis(),
            "finished finding transcript sessions for cwd",
        );
        Ok(sessions)
    }

    /// Load a complete transcript for display.
    pub async fn load_transcript(
        &self,
        project_id: &str,
        session_id: &str,
    ) -> io::Result<Transcript> {
        let path = self
            .transcripts_base()
            .join(project_id)
            .join(SESSIONS_DIR)
            .join(format!("{session_id}.jsonl"));

        load_transcript_from_path(&path).await
    }

    /// Load just the session metadata (for quick listing).
    pub async fn load_session_meta(
        &self,
        project_id: &str,
        session_id: &str,
    ) -> io::Result<SessionMetaEntry> {
        let path = self
            .transcripts_base()
            .join(project_id)
            .join(SESSIONS_DIR)
            .join(format!("{session_id}.jsonl"));

        load_session_meta_from_path(&path).await
    }

    /// Get the path to a session's transcript file.
    pub fn session_path(&self, project_id: &str, session_id: &str) -> PathBuf {
        self.transcripts_base()
            .join(project_id)
            .join(SESSIONS_DIR)
            .join(format!("{session_id}.jsonl"))
    }
}

/// Count the number of session files in a directory.
async fn count_sessions(sessions_path: &Path) -> io::Result<usize> {
    if !sessions_path.exists() {
        return Ok(0);
    }

    let mut count = 0;
    let mut read_dir = tokio::fs::read_dir(sessions_path).await?;

    while let Some(entry) = read_dir.next_entry().await? {
        if entry.path().extension().is_some_and(|ext| ext == "jsonl") {
            count += 1;
        }
    }

    Ok(count)
}

/// Get the timestamp of the most recent session.
async fn get_last_session_timestamp(sessions_path: &Path) -> Option<String> {
    if !sessions_path.exists() {
        return None;
    }

    let mut read_dir = tokio::fs::read_dir(sessions_path).await.ok()?;
    let mut latest: Option<String> = None;

    while let Some(entry) = read_dir.next_entry().await.ok()? {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "jsonl")
            && let Ok(meta) = load_session_meta_from_path(&path).await
            && latest.as_ref().is_none_or(|l| meta.started_at > *l)
        {
            latest = Some(meta.started_at);
        }
    }

    latest
}

/// Load session info from a transcript file.
async fn load_session_info(path: &Path, project_id: &str) -> io::Result<SessionInfo> {
    let started = Instant::now();
    let transcript_bytes = tokio::fs::metadata(path)
        .await
        .map(|metadata| metadata.len())
        .ok();
    tracing::info!(
        target: "nori_resume",
        phase = "transcript_loader.load_session_info.start",
        project_id,
        path = %path.display(),
        transcript_bytes,
        "loading transcript session metadata and line count",
    );

    let meta_started = Instant::now();
    let meta = load_session_meta_from_path(path).await?;
    tracing::info!(
        target: "nori_resume",
        phase = "transcript_loader.load_session_info.meta_loaded",
        project_id,
        path = %path.display(),
        session_id = %meta.session_id,
        agent = meta.agent.as_deref().unwrap_or("<unknown>"),
        elapsed_ms = meta_started.elapsed().as_millis(),
        total_elapsed_ms = started.elapsed().as_millis(),
        "loaded transcript session metadata",
    );

    // Count entries (approximate - just count lines)
    let read_started = Instant::now();
    let content = tokio::fs::read_to_string(path).await?;
    let read_elapsed_ms = read_started.elapsed().as_millis();
    let count_started = Instant::now();
    let entry_count = content.lines().count();
    tracing::info!(
        target: "nori_resume",
        phase = "transcript_loader.load_session_info.done",
        project_id,
        path = %path.display(),
        session_id = %meta.session_id,
        agent = meta.agent.as_deref().unwrap_or("<unknown>"),
        entry_count,
        transcript_bytes,
        read_elapsed_ms,
        count_elapsed_ms = count_started.elapsed().as_millis(),
        total_elapsed_ms = started.elapsed().as_millis(),
        "loaded transcript session info",
    );

    Ok(SessionInfo {
        session_id: meta.session_id,
        project_id: project_id.to_string(),
        started_at: meta.started_at,
        cwd: meta.cwd,
        agent: meta.agent,
        entry_count,
    })
}

/// Load session metadata from the first line of a transcript file.
async fn load_session_meta_from_path(path: &Path) -> io::Result<SessionMetaEntry> {
    let file = tokio::fs::File::open(path).await?;
    let reader = tokio::io::BufReader::new(file);
    let mut lines = reader.lines();

    if let Some(first_line) = lines.next_line().await? {
        let parsed: TranscriptLine = serde_json::from_str(&first_line)
            .map_err(|e| io::Error::other(format!("failed to parse transcript line: {e}")))?;

        match parsed.entry {
            TranscriptEntry::SessionMeta(meta) => Ok(meta),
            _ => Err(io::Error::other(
                "first line of transcript is not session metadata",
            )),
        }
    } else {
        Err(io::Error::other("transcript file is empty"))
    }
}

/// Load a complete transcript from a file.
///
/// Lines that fail to deserialize (e.g. unknown entry types from older or newer
/// versions) are silently skipped so that transcripts remain loadable across
/// schema changes.
async fn load_transcript_from_path(path: &Path) -> io::Result<Transcript> {
    let started = Instant::now();
    let transcript_bytes = tokio::fs::metadata(path)
        .await
        .map(|metadata| metadata.len())
        .ok();
    tracing::info!(
        target: "nori_resume",
        phase = "transcript_loader.load_transcript.start",
        path = %path.display(),
        transcript_bytes,
        "loading full transcript",
    );

    let file = tokio::fs::File::open(path).await?;
    let reader = tokio::io::BufReader::new(file);
    let mut lines = reader.lines();

    let mut entries = Vec::new();
    let mut meta: Option<SessionMetaEntry> = None;
    let mut line_count = 0usize;
    let mut skipped_count = 0usize;
    let mut bytes_seen = 0usize;
    let mut next_progress_bytes = TRANSCRIPT_LOAD_PROGRESS_BYTES;

    while let Some(line) = lines.next_line().await? {
        line_count += 1;
        bytes_seen = bytes_seen.saturating_add(line.len() + 1);
        if bytes_seen >= next_progress_bytes {
            tracing::info!(
                target: "nori_resume",
                phase = "transcript_loader.load_transcript.progress",
                path = %path.display(),
                line_count,
                parsed_entry_count = entries.len(),
                skipped_count,
                bytes_seen,
                transcript_bytes,
                elapsed_ms = started.elapsed().as_millis(),
                "still loading full transcript",
            );
            next_progress_bytes =
                next_progress_bytes.saturating_add(TRANSCRIPT_LOAD_PROGRESS_BYTES);
        }

        if line.trim().is_empty() {
            continue;
        }

        let parsed: TranscriptLine = match serde_json::from_str(&line) {
            Ok(entry) => entry,
            Err(e) => {
                // The first line must be valid session metadata; fail hard.
                if meta.is_none() {
                    tracing::warn!(
                        target: "nori_resume",
                        phase = "transcript_loader.load_transcript.first_line_error",
                        path = %path.display(),
                        line_count,
                        elapsed_ms = started.elapsed().as_millis(),
                        error = %e,
                        "failed to parse first transcript line",
                    );
                    return Err(io::Error::other(format!(
                        "failed to parse transcript line: {e}"
                    )));
                }
                // Skip unrecognized entries (e.g. removed or future event types).
                skipped_count += 1;
                tracing::debug!(
                    path = %path.display(),
                    error = %e,
                    "skipping unrecognized transcript entry",
                );
                continue;
            }
        };

        // Extract metadata from first entry
        if meta.is_none()
            && let TranscriptEntry::SessionMeta(ref m) = parsed.entry
        {
            meta = Some(m.clone());
        }

        entries.push(parsed);
    }

    let meta =
        meta.ok_or_else(|| io::Error::other("transcript does not contain session metadata"))?;

    tracing::info!(
        target: "nori_resume",
        phase = "transcript_loader.load_transcript.done",
        path = %path.display(),
        session_id = %meta.session_id,
        agent = meta.agent.as_deref().unwrap_or("<unknown>"),
        line_count,
        parsed_entry_count = entries.len(),
        skipped_count,
        bytes_seen,
        transcript_bytes,
        elapsed_ms = started.elapsed().as_millis(),
        "loaded full transcript",
    );

    Ok(Transcript { meta, entries })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::ContentBlock;
    use crate::transcript::recorder::TranscriptRecorder;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_list_projects_empty() {
        let temp_dir = TempDir::new().unwrap();
        let loader = TranscriptLoader::new(temp_dir.path().to_path_buf());

        let projects = loader.list_projects().await.unwrap();
        assert!(projects.is_empty());
    }

    #[tokio::test]
    async fn test_list_projects_with_sessions() {
        let temp_dir = TempDir::new().unwrap();
        let nori_home = temp_dir.path();

        // Create a session to populate the project
        let recorder = TranscriptRecorder::new(nori_home, nori_home, None, "0.1.0", None)
            .await
            .unwrap();
        recorder
            .record_user_message("msg-001", "Hello", vec![])
            .await
            .unwrap();
        recorder.flush().await.unwrap();
        recorder.shutdown().await.unwrap();

        // Now list projects
        let loader = TranscriptLoader::new(nori_home.to_path_buf());
        let projects = loader.list_projects().await.unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].session_count, 1);
    }

    #[tokio::test]
    async fn test_list_sessions_for_project() {
        let temp_dir = TempDir::new().unwrap();
        let nori_home = temp_dir.path();

        // Create two sessions
        let recorder1 = TranscriptRecorder::new(nori_home, nori_home, None, "0.1.0", None)
            .await
            .unwrap();
        let project_id = recorder1.project_id().to_string();
        recorder1.flush().await.unwrap();
        recorder1.shutdown().await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let recorder2 = TranscriptRecorder::new(nori_home, nori_home, None, "0.1.0", None)
            .await
            .unwrap();
        recorder2.flush().await.unwrap();
        recorder2.shutdown().await.unwrap();

        // List sessions
        let loader = TranscriptLoader::new(nori_home.to_path_buf());
        let sessions = loader.list_sessions(&project_id).await.unwrap();

        assert_eq!(sessions.len(), 2);
        // Should be sorted by started_at descending
        assert!(sessions[0].started_at >= sessions[1].started_at);
    }

    #[tokio::test]
    async fn test_find_sessions_for_cwd() {
        let temp_dir = TempDir::new().unwrap();
        let nori_home = temp_dir.path();

        // Create a session
        let recorder = TranscriptRecorder::new(nori_home, nori_home, None, "0.1.0", None)
            .await
            .unwrap();
        recorder.flush().await.unwrap();
        recorder.shutdown().await.unwrap();

        // Find sessions for same cwd
        let loader = TranscriptLoader::new(nori_home.to_path_buf());
        let sessions = loader.find_sessions_for_cwd(nori_home).await.unwrap();

        assert_eq!(sessions.len(), 1);
    }

    #[tokio::test]
    async fn test_load_transcript() {
        let temp_dir = TempDir::new().unwrap();
        let nori_home = temp_dir.path();

        // Create a session with entries
        let recorder = TranscriptRecorder::new(
            nori_home,
            nori_home,
            Some("claude".to_string()),
            "0.1.0",
            None,
        )
        .await
        .unwrap();
        let project_id = recorder.project_id().to_string();
        let session_id = recorder.session_id().to_string();

        recorder
            .record_user_message("msg-001", "Hello", vec![])
            .await
            .unwrap();
        recorder
            .record_assistant_message(
                "msg-002",
                vec![ContentBlock::Text {
                    text: "Hi there!".to_string(),
                }],
                Some("claude".to_string()),
            )
            .await
            .unwrap();
        recorder.flush().await.unwrap();
        recorder.shutdown().await.unwrap();

        // Load the transcript
        let loader = TranscriptLoader::new(nori_home.to_path_buf());
        let transcript = loader
            .load_transcript(&project_id, &session_id)
            .await
            .unwrap();

        assert_eq!(transcript.meta.session_id, session_id);
        assert_eq!(transcript.entries.len(), 3); // SessionMeta + User + Assistant
    }

    #[tokio::test]
    async fn test_load_transcript_with_client_event_entry() {
        let temp_dir = TempDir::new().unwrap();
        let nori_home = temp_dir.path();

        let recorder = TranscriptRecorder::new(nori_home, nori_home, None, "0.1.0", None)
            .await
            .unwrap();
        let project_id = recorder.project_id().to_string();
        let session_id = recorder.session_id().to_string();

        recorder
            .record_client_event(&nori_protocol::ClientEvent::ToolSnapshot(
                nori_protocol::ToolSnapshot {
                    call_id: "call-001".to_string(),
                    title: "Edit /tmp/test.md".to_string(),
                    kind: nori_protocol::ToolKind::Edit,
                    phase: nori_protocol::ToolPhase::Completed,
                    locations: vec![],
                    invocation: None,
                    artifacts: vec![],
                    raw_input: None,
                    raw_output: None,
                    owner_request_id: None,
                },
            ))
            .await
            .unwrap();
        recorder.flush().await.unwrap();
        recorder.shutdown().await.unwrap();

        let loader = TranscriptLoader::new(nori_home.to_path_buf());
        let transcript = loader
            .load_transcript(&project_id, &session_id)
            .await
            .unwrap();

        assert_eq!(transcript.entries.len(), 2);
        assert!(matches!(
            transcript.entries[1].entry,
            TranscriptEntry::ClientEvent(_)
        ));
    }

    #[tokio::test]
    async fn test_load_session_meta() {
        let temp_dir = TempDir::new().unwrap();
        let nori_home = temp_dir.path();

        // Create a session
        let recorder = TranscriptRecorder::new(
            nori_home,
            nori_home,
            Some("claude".to_string()),
            "0.1.0",
            None,
        )
        .await
        .unwrap();
        let project_id = recorder.project_id().to_string();
        let session_id = recorder.session_id().to_string();
        recorder.flush().await.unwrap();
        recorder.shutdown().await.unwrap();

        // Load just the metadata
        let loader = TranscriptLoader::new(nori_home.to_path_buf());
        let meta = loader
            .load_session_meta(&project_id, &session_id)
            .await
            .unwrap();

        assert_eq!(meta.session_id, session_id);
        assert_eq!(meta.agent, Some("claude".to_string()));
        assert_eq!(meta.cli_version, "0.1.0");
    }

    #[tokio::test]
    async fn test_list_sessions_empty_project() {
        let temp_dir = TempDir::new().unwrap();
        let loader = TranscriptLoader::new(temp_dir.path().to_path_buf());

        let sessions = loader.list_sessions("nonexistent").await.unwrap();
        assert!(sessions.is_empty());
    }

    /// Helper to write a raw JSONL transcript file with the given lines.
    async fn write_raw_transcript(
        nori_home: &Path,
        project_id: &str,
        session_id: &str,
        lines: &[&str],
    ) {
        let sessions_dir = nori_home
            .join(TRANSCRIPTS_DIR)
            .join(BY_PROJECT_DIR)
            .join(project_id)
            .join(SESSIONS_DIR);
        tokio::fs::create_dir_all(&sessions_dir).await.unwrap();

        // Write project metadata
        let project_dir = nori_home
            .join(TRANSCRIPTS_DIR)
            .join(BY_PROJECT_DIR)
            .join(project_id);
        let meta_path = project_dir.join(PROJECT_METADATA_FILE);
        let meta_json = serde_json::json!({
            "id": project_id,
            "name": "test-project",
            "cwd": "/tmp/test",
        });
        tokio::fs::write(
            &meta_path,
            serde_json::to_string_pretty(&meta_json).unwrap(),
        )
        .await
        .unwrap();

        let path = sessions_dir.join(format!("{session_id}.jsonl"));
        let content = lines.join("\n") + "\n";
        tokio::fs::write(&path, content).await.unwrap();
    }

    #[tokio::test]
    async fn test_load_transcript_skips_unknown_entry_types() {
        let temp_dir = TempDir::new().unwrap();
        let nori_home = temp_dir.path();
        let project_id = "test-project-123";
        let session_id = "test-session-456";

        // Valid session meta line
        let meta_line = serde_json::json!({
            "ts": "2025-01-27T12:00:00.000Z",
            "v": 2,
            "type": "session_meta",
            "session_id": session_id,
            "project_id": project_id,
            "started_at": "2025-01-27T12:00:00.000Z",
            "cwd": "/tmp/test",
            "cli_version": "0.1.0"
        });

        // Unknown entry type (simulates old turn_lifecycle from removed ClientEvent variant)
        let unknown_line = serde_json::json!({
            "ts": "2025-01-27T12:00:01.000Z",
            "v": 2,
            "type": "client_event",
            "event": {
                "event_type": "turn_lifecycle",
                "data": {"phase": "started"}
            }
        });

        // Valid user entry
        let user_line = serde_json::json!({
            "ts": "2025-01-27T12:00:02.000Z",
            "v": 2,
            "type": "user",
            "id": "msg-001",
            "content": "Hello",
            "attachments": []
        });

        write_raw_transcript(
            nori_home,
            project_id,
            session_id,
            &[
                &meta_line.to_string(),
                &unknown_line.to_string(),
                &user_line.to_string(),
            ],
        )
        .await;

        let loader = TranscriptLoader::new(nori_home.to_path_buf());
        let transcript = loader
            .load_transcript(project_id, session_id)
            .await
            .unwrap();

        // Should have loaded meta + user, skipping the unknown entry
        assert_eq!(transcript.meta.session_id, session_id);
        assert_eq!(transcript.entries.len(), 2);
        assert!(matches!(
            transcript.entries[0].entry,
            TranscriptEntry::SessionMeta(_)
        ));
        assert!(matches!(
            transcript.entries[1].entry,
            TranscriptEntry::User(_)
        ));
    }

    #[tokio::test]
    async fn test_load_transcript_with_only_unknown_entries() {
        let temp_dir = TempDir::new().unwrap();
        let nori_home = temp_dir.path();
        let project_id = "test-project-789";
        let session_id = "test-session-abc";

        // Valid session meta line
        let meta_line = serde_json::json!({
            "ts": "2025-01-27T12:00:00.000Z",
            "v": 2,
            "type": "session_meta",
            "session_id": session_id,
            "project_id": project_id,
            "started_at": "2025-01-27T12:00:00.000Z",
            "cwd": "/tmp/test",
            "cli_version": "0.1.0"
        });

        // Only unknown entries after the meta
        let unknown1 = serde_json::json!({
            "ts": "2025-01-27T12:00:01.000Z",
            "v": 2,
            "type": "client_event",
            "event": {
                "event_type": "turn_lifecycle",
                "data": {"phase": "started"}
            }
        });
        let unknown2 = serde_json::json!({
            "ts": "2025-01-27T12:00:02.000Z",
            "v": 2,
            "type": "some_future_type",
            "payload": "whatever"
        });

        write_raw_transcript(
            nori_home,
            project_id,
            session_id,
            &[
                &meta_line.to_string(),
                &unknown1.to_string(),
                &unknown2.to_string(),
            ],
        )
        .await;

        let loader = TranscriptLoader::new(nori_home.to_path_buf());
        let transcript = loader
            .load_transcript(project_id, session_id)
            .await
            .unwrap();

        // Should load with just the meta entry, all unknown entries skipped
        assert_eq!(transcript.meta.session_id, session_id);
        assert_eq!(transcript.entries.len(), 1);
        assert!(matches!(
            transcript.entries[0].entry,
            TranscriptEntry::SessionMeta(_)
        ));
    }

    #[tokio::test]
    async fn test_load_transcript_fails_on_corrupt_first_line() {
        let temp_dir = TempDir::new().unwrap();
        let nori_home = temp_dir.path();
        let project_id = "test-project-fail";
        let session_id = "test-session-fail";

        // First line is not valid session metadata
        let bad_first_line = serde_json::json!({
            "ts": "2025-01-27T12:00:00.000Z",
            "v": 2,
            "type": "some_future_type",
            "data": {}
        });

        write_raw_transcript(
            nori_home,
            project_id,
            session_id,
            &[&bad_first_line.to_string()],
        )
        .await;

        let loader = TranscriptLoader::new(nori_home.to_path_buf());
        let result = loader.load_transcript(project_id, session_id).await;
        assert!(result.is_err());
    }
}
