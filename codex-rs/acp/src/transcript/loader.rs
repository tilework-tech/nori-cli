//! Transcript loader for reading and listing transcripts.

use std::io::Error as IoError;
use std::path::Path;
use std::path::PathBuf;

use tracing::debug;
use tracing::warn;

use super::project::compute_project_id;
use super::types::SessionMetaEntry;
use super::types::TranscriptEntry;
use super::types::TranscriptLine;

/// Subdirectory for transcripts within NORI_HOME.
const TRANSCRIPTS_SUBDIR: &str = "transcripts";
/// Subdirectory for project-organized transcripts.
const BY_PROJECT_SUBDIR: &str = "by-project";
/// Subdirectory for sessions within a project.
const SESSIONS_SUBDIR: &str = "sessions";

/// Information about a project with transcripts.
#[derive(Debug, Clone)]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    pub git_remote: Option<String>,
    pub cwd: PathBuf,
    pub session_count: usize,
    pub last_session_at: Option<String>,
}

/// Information about a session.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub project_id: String,
    pub started_at: String,
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub message_count: usize,
}

/// A loaded transcript.
#[derive(Debug)]
pub struct Transcript {
    pub meta: SessionMetaEntry,
    pub entries: Vec<TranscriptLine>,
}

/// Loads and lists transcripts for viewing.
pub struct TranscriptLoader {
    nori_home: PathBuf,
}

impl TranscriptLoader {
    pub fn new(nori_home: PathBuf) -> Self {
        Self { nori_home }
    }

    /// Get the base path for transcripts.
    fn transcripts_path(&self) -> PathBuf {
        self.nori_home
            .join(TRANSCRIPTS_SUBDIR)
            .join(BY_PROJECT_SUBDIR)
    }

    /// Get the sessions directory for a project.
    fn project_sessions_path(&self, project_id: &str) -> PathBuf {
        self.transcripts_path()
            .join(project_id)
            .join(SESSIONS_SUBDIR)
    }

    /// Get the path to a specific session file.
    fn session_file_path(&self, project_id: &str, session_id: &str) -> PathBuf {
        self.project_sessions_path(project_id)
            .join(format!("{session_id}.jsonl"))
    }

    /// List all projects that have transcripts.
    pub async fn list_projects(&self) -> std::io::Result<Vec<ProjectInfo>> {
        let projects_path = self.transcripts_path();

        if !projects_path.exists() {
            return Ok(Vec::new());
        }

        let mut projects = Vec::new();

        let mut entries = tokio::fs::read_dir(&projects_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let project_id = match path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            // Count sessions in this project
            let sessions_path = path.join(SESSIONS_SUBDIR);
            let session_count = if sessions_path.exists() {
                count_jsonl_files(&sessions_path).await
            } else {
                0
            };

            if session_count == 0 {
                continue;
            }

            // Try to get info from the most recent session
            let (name, cwd, git_remote, last_session_at) =
                self.get_project_info_from_sessions(&project_id).await;

            projects.push(ProjectInfo {
                id: project_id,
                name,
                git_remote,
                cwd,
                session_count,
                last_session_at,
            });
        }

        Ok(projects)
    }

    /// Get project info from its sessions.
    async fn get_project_info_from_sessions(
        &self,
        project_id: &str,
    ) -> (String, PathBuf, Option<String>, Option<String>) {
        let sessions = match self.list_sessions(project_id).await {
            Ok(s) => s,
            Err(_) => return (project_id.to_string(), PathBuf::new(), None, None),
        };

        if let Some(session) = sessions.first() {
            let name = session
                .cwd
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| project_id.to_string());

            // Try to get git remote from session meta
            let git_remote = if let Ok(meta) = self
                .load_session_meta(project_id, &session.session_id)
                .await
            {
                meta.git.and_then(|g| g.remote_url)
            } else {
                None
            };

            (
                name,
                session.cwd.clone(),
                git_remote,
                Some(session.started_at.clone()),
            )
        } else {
            (project_id.to_string(), PathBuf::new(), None, None)
        }
    }

    /// List all sessions for a specific project.
    pub async fn list_sessions(&self, project_id: &str) -> std::io::Result<Vec<SessionInfo>> {
        let sessions_path = self.project_sessions_path(project_id);

        debug!(
            "list_sessions: project_id={}, sessions_path={}, exists={}",
            project_id,
            sessions_path.display(),
            sessions_path.exists()
        );

        if !sessions_path.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();

        let mut entries = tokio::fs::read_dir(&sessions_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            // Only process .jsonl files
            if path.extension().map(|e| e != "jsonl").unwrap_or(true) {
                continue;
            }

            let session_id = match path.file_stem().and_then(|n| n.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            // Load session meta to get info
            match self.load_session_meta(project_id, &session_id).await {
                Ok(meta) => {
                    // Count messages in the file
                    let message_count = count_lines_in_file(&path).await.unwrap_or(0);

                    sessions.push(SessionInfo {
                        session_id,
                        project_id: project_id.to_string(),
                        started_at: meta.started_at,
                        cwd: meta.cwd,
                        model: meta.model,
                        message_count,
                    });
                }
                Err(e) => {
                    warn!("Failed to load session meta for {session_id}: {e}");
                    continue;
                }
            }
        }

        // Sort by started_at descending (most recent first)
        sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));

        debug!("list_sessions: returning {} sessions", sessions.len());

        Ok(sessions)
    }

    /// Find sessions for the current working directory.
    pub async fn find_sessions_for_cwd(&self, cwd: &Path) -> std::io::Result<Vec<SessionInfo>> {
        // Compute project ID for this cwd
        let project = compute_project_id(cwd)?;
        debug!(
            "find_sessions_for_cwd: cwd={}, project_id={}, nori_home={}",
            cwd.display(),
            project.id,
            self.nori_home.display()
        );
        self.list_sessions(&project.id).await
    }

    /// Load a complete transcript for display.
    pub async fn load_transcript(
        &self,
        project_id: &str,
        session_id: &str,
    ) -> std::io::Result<Transcript> {
        let path = self.session_file_path(project_id, session_id);
        let content = tokio::fs::read_to_string(&path).await?;

        let mut entries = Vec::new();
        let mut meta: Option<SessionMetaEntry> = None;

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str::<TranscriptLine>(line) {
                Ok(transcript_line) => {
                    // Extract meta from the first SessionMeta entry
                    if meta.is_none()
                        && let TranscriptEntry::SessionMeta(ref m) = transcript_line.entry
                    {
                        meta = Some(m.clone());
                    }
                    entries.push(transcript_line);
                }
                Err(e) => {
                    warn!("Failed to parse transcript line: {e}");
                    continue;
                }
            }
        }

        let meta = meta.ok_or_else(|| {
            IoError::new(
                std::io::ErrorKind::InvalidData,
                "No session meta found in transcript",
            )
        })?;

        Ok(Transcript { meta, entries })
    }

    /// Load just the session metadata.
    pub async fn load_session_meta(
        &self,
        project_id: &str,
        session_id: &str,
    ) -> std::io::Result<SessionMetaEntry> {
        let path = self.session_file_path(project_id, session_id);
        let content = tokio::fs::read_to_string(&path).await?;

        // Only parse the first line
        let first_line = content.lines().next().ok_or_else(|| {
            IoError::new(std::io::ErrorKind::InvalidData, "Empty transcript file")
        })?;

        let transcript_line: TranscriptLine = serde_json::from_str(first_line)?;

        match transcript_line.entry {
            TranscriptEntry::SessionMeta(meta) => Ok(meta),
            _ => Err(IoError::new(
                std::io::ErrorKind::InvalidData,
                "First line is not session meta",
            )),
        }
    }
}

/// Count .jsonl files in a directory.
async fn count_jsonl_files(path: &Path) -> usize {
    let mut count = 0;
    if let Ok(mut entries) = tokio::fs::read_dir(path).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry
                .path()
                .extension()
                .map(|e| e == "jsonl")
                .unwrap_or(false)
            {
                count += 1;
            }
        }
    }
    count
}

/// Count lines in a file.
async fn count_lines_in_file(path: &Path) -> std::io::Result<usize> {
    let content = tokio::fs::read_to_string(path).await?;
    Ok(content.lines().filter(|l| !l.trim().is_empty()).count())
}
