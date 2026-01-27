//! Transcript recorder for persisting session transcripts.
//!
//! Uses async channel for non-blocking writes, following the same pattern
//! as the core RolloutRecorder.

use std::io::Error as IoError;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use time::OffsetDateTime;
use time::format_description::FormatItem;
use time::macros::format_description;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::Sender;
use tokio::sync::mpsc::{self};
use tokio::sync::oneshot;
use tracing::debug;
use tracing::warn;
use uuid::Uuid;

use super::project::compute_project_id;
use super::types::AssistantEntry;
use super::types::ContentBlock;
use super::types::GitInfo;
use super::types::SessionMetaEntry;
use super::types::TRANSCRIPT_SCHEMA_VERSION;
use super::types::ToolCallEntry;
use super::types::ToolResultEntry;
use super::types::TranscriptEntry;
use super::types::TranscriptLine;
use super::types::UserEntry;

/// Subdirectory for transcripts within NORI_HOME.
const TRANSCRIPTS_SUBDIR: &str = "transcripts";
/// Subdirectory for project-organized transcripts.
const BY_PROJECT_SUBDIR: &str = "by-project";
/// Subdirectory for sessions within a project.
const SESSIONS_SUBDIR: &str = "sessions";

/// Records transcript entries for a session.
#[derive(Clone)]
pub struct TranscriptRecorder {
    tx: Sender<TranscriptCmd>,
    session_id: String,
    project_id: String,
    transcript_path: PathBuf,
    /// Per-session counter for generating unique message IDs.
    message_counter: Arc<AtomicU64>,
}

enum TranscriptCmd {
    Write(Box<TranscriptEntry>),
    Flush { ack: oneshot::Sender<()> },
    Shutdown { ack: oneshot::Sender<()> },
}

impl TranscriptRecorder {
    /// Initialize for a new session.
    pub async fn new(nori_home: &Path, cwd: &Path, model: Option<String>) -> std::io::Result<Self> {
        // Compute project ID from cwd
        let project = compute_project_id(cwd)?;

        // Create session ID
        let session_id = Uuid::new_v4().to_string();

        // Create directory structure: $NORI_HOME/transcripts/by-project/{project_id}/sessions/
        let sessions_dir = nori_home
            .join(TRANSCRIPTS_SUBDIR)
            .join(BY_PROJECT_SUBDIR)
            .join(&project.id)
            .join(SESSIONS_SUBDIR);
        std::fs::create_dir_all(&sessions_dir)?;

        // Create transcript file path
        let transcript_path = sessions_dir.join(format!("{session_id}.jsonl"));

        // Open file for writing
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&transcript_path)
            .await?;

        // Create channel for async writes
        let (tx, rx) = mpsc::channel::<TranscriptCmd>(256);

        // Collect git info if available
        let git_info = collect_git_info(cwd);

        // Create session metadata
        let session_meta = SessionMetaEntry {
            session_id: session_id.clone(),
            project_id: project.id.clone(),
            started_at: current_timestamp(),
            cwd: cwd.to_path_buf(),
            model,
            cli_version: env!("CARGO_PKG_VERSION").to_string(),
            git: git_info,
        };

        debug!(
            "TranscriptRecorder initialized: project_id={}, session_id={}, path={}",
            project.id,
            session_id,
            transcript_path.display()
        );

        // Spawn writer task with error logging
        tokio::spawn(async move {
            if let Err(e) = transcript_writer(file, rx, session_meta).await {
                warn!("Transcript writer task failed: {e}");
            }
        });

        Ok(Self {
            tx,
            session_id,
            project_id: project.id,
            transcript_path,
            message_counter: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Generate a unique message ID for this session.
    fn generate_message_id(&self) -> String {
        let count = self.message_counter.fetch_add(1, Ordering::Relaxed);
        format!("msg-{count:06}")
    }

    /// Record a user message.
    pub async fn record_user_message(&self, content: &str) -> std::io::Result<()> {
        debug!(
            "Recording user message: session={}, content_len={}",
            self.session_id,
            content.len()
        );
        let entry = TranscriptEntry::User(UserEntry {
            id: self.generate_message_id(),
            content: content.to_string(),
        });
        self.send_entry(entry).await
    }

    /// Record a tool call.
    pub async fn record_tool_call(
        &self,
        call_id: &str,
        name: &str,
        input: &serde_json::Value,
    ) -> std::io::Result<()> {
        let entry = TranscriptEntry::ToolCall(ToolCallEntry {
            call_id: call_id.to_string(),
            name: name.to_string(),
            input: input.clone(),
        });
        self.send_entry(entry).await
    }

    /// Record a tool result.
    pub async fn record_tool_result(
        &self,
        call_id: &str,
        output: &str,
        truncated: bool,
        exit_code: Option<i32>,
    ) -> std::io::Result<()> {
        let entry = TranscriptEntry::ToolResult(ToolResultEntry {
            call_id: call_id.to_string(),
            output: output.to_string(),
            truncated,
            exit_code,
        });
        self.send_entry(entry).await
    }

    /// Record a complete assistant turn.
    pub async fn record_assistant_message(
        &self,
        content: Vec<ContentBlock>,
        model: Option<String>,
    ) -> std::io::Result<()> {
        let entry = TranscriptEntry::Assistant(AssistantEntry {
            id: self.generate_message_id(),
            content,
            model,
        });
        self.send_entry(entry).await
    }

    /// Flush all pending writes.
    pub async fn flush(&self) -> std::io::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(TranscriptCmd::Flush { ack: tx })
            .await
            .map_err(|e| IoError::other(format!("failed to queue transcript flush: {e}")))?;
        rx.await
            .map_err(|e| IoError::other(format!("failed waiting for transcript flush: {e}")))
    }

    /// Graceful shutdown.
    pub async fn shutdown(&self) -> std::io::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(TranscriptCmd::Shutdown { ack: tx })
            .await
            .map_err(|e| IoError::other(format!("failed to queue transcript shutdown: {e}")))?;
        rx.await
            .map_err(|e| IoError::other(format!("failed waiting for transcript shutdown: {e}")))
    }

    /// Get the path to this session's transcript file.
    pub fn transcript_path(&self) -> &Path {
        &self.transcript_path
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get the project ID.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    async fn send_entry(&self, entry: TranscriptEntry) -> std::io::Result<()> {
        self.tx
            .send(TranscriptCmd::Write(Box::new(entry)))
            .await
            .map_err(|e| IoError::other(format!("failed to queue transcript entry: {e}")))
    }
}

/// Background task that owns the file and writes entries.
async fn transcript_writer(
    file: tokio::fs::File,
    mut rx: mpsc::Receiver<TranscriptCmd>,
    session_meta: SessionMetaEntry,
) -> std::io::Result<()> {
    let mut writer = JsonlWriter { file };

    // Write session meta as first entry
    writer
        .write_entry(TranscriptEntry::SessionMeta(session_meta))
        .await?;

    // Process commands
    while let Some(cmd) = rx.recv().await {
        match cmd {
            TranscriptCmd::Write(entry) => {
                writer.write_entry(*entry).await?;
            }
            TranscriptCmd::Flush { ack } => {
                if let Err(e) = writer.file.flush().await {
                    let _ = ack.send(());
                    return Err(e);
                }
                let _ = ack.send(());
            }
            TranscriptCmd::Shutdown { ack } => {
                let _ = writer.file.flush().await;
                let _ = ack.send(());
                break;
            }
        }
    }

    Ok(())
}

struct JsonlWriter {
    file: tokio::fs::File,
}

impl JsonlWriter {
    async fn write_entry(&mut self, entry: TranscriptEntry) -> std::io::Result<()> {
        let line = TranscriptLine {
            ts: current_timestamp(),
            v: TRANSCRIPT_SCHEMA_VERSION,
            entry,
        };

        let mut json = serde_json::to_string(&line)?;
        json.push('\n');
        self.file.write_all(json.as_bytes()).await?;
        self.file.flush().await?;
        Ok(())
    }
}

/// Get current UTC timestamp in ISO 8601 format.
fn current_timestamp() -> String {
    let format: &[FormatItem] =
        format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");
    OffsetDateTime::now_utc()
        .format(format)
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Collect git information from the working directory.
fn collect_git_info(cwd: &Path) -> Option<GitInfo> {
    use std::process::Command;

    // Get current branch
    let branch = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    // Get current commit hash
    let commit_hash = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    // Get remote URL
    let remote_url = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    // Only return GitInfo if we have at least one piece of info
    if branch.is_some() || commit_hash.is_some() || remote_url.is_some() {
        Some(GitInfo {
            branch,
            commit_hash,
            remote_url,
        })
    } else {
        None
    }
}
