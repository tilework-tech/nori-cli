//! TranscriptRecorder - Records transcript entries for a session.
//!
//! Uses async channel for non-blocking writes (same pattern as core RolloutRecorder).

use std::io;
use std::io::Error as IoError;
use std::path::Path;
use std::path::PathBuf;

use serde_json;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;

use super::BY_PROJECT_DIR;
use super::PROJECT_METADATA_FILE;
use super::SESSIONS_DIR;
use super::TRANSCRIPTS_DIR;
use super::project::ProjectId;
use super::project::compute_project_id;
use super::types::AssistantEntry;
use super::types::Attachment;
use super::types::ClientEventEntry;
use super::types::ContentBlock;
use super::types::GitInfo;
use super::types::PatchApplyEntry;
use super::types::PatchOperationType;
use super::types::SessionMetaEntry;
use super::types::ToolCallEntry;
use super::types::ToolResultEntry;
use super::types::TranscriptEntry;
use super::types::TranscriptLine;
use super::types::UserEntry;
use super::types::now_iso8601;
use nori_protocol::ClientEvent as NoriClientEvent;

/// Commands sent to the background writer task.
enum TranscriptCmd {
    Write(Box<TranscriptEntry>),
    Flush { ack: oneshot::Sender<()> },
    Shutdown { ack: oneshot::Sender<()> },
}

/// Records transcript entries for a session.
/// Uses async channel for non-blocking writes (same pattern as core RolloutRecorder).
#[derive(Clone)]
pub struct TranscriptRecorder {
    tx: Sender<TranscriptCmd>,
    session_id: String,
    project_id: String,
    transcript_path: PathBuf,
}

impl TranscriptRecorder {
    /// Initialize for a new session.
    ///
    /// - Detects project from cwd (git root or cwd path)
    /// - Creates project directory if needed
    /// - Opens new session JSONL file
    /// - Writes SessionMeta as first entry
    pub async fn new(
        nori_home: &Path,
        cwd: &Path,
        agent: Option<String>,
        cli_version: &str,
        acp_session_id: Option<String>,
    ) -> io::Result<Self> {
        // Compute project ID from cwd
        let project_id_info = compute_project_id(cwd).await?;

        // Create session ID (UUID)
        let session_id = generate_session_id();

        // Create directory structure
        let project_dir = nori_home
            .join(TRANSCRIPTS_DIR)
            .join(BY_PROJECT_DIR)
            .join(&project_id_info.id);
        let sessions_dir = project_dir.join(SESSIONS_DIR);
        tokio::fs::create_dir_all(&sessions_dir).await?;

        // Write/update project metadata
        let project_meta_path = project_dir.join(PROJECT_METADATA_FILE);
        write_project_metadata(&project_meta_path, &project_id_info).await?;

        // Create session transcript file
        let transcript_path = sessions_dir.join(format!("{session_id}.jsonl"));
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&transcript_path)
            .await?;

        // Set file permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            tokio::fs::set_permissions(&transcript_path, perms).await?;
        }

        // Create channel and spawn writer task
        let (tx, rx) = mpsc::channel::<TranscriptCmd>(256);

        // Collect git info for session metadata
        let git_info = collect_git_info(cwd).await;

        // Create session metadata
        let session_meta = SessionMetaEntry {
            session_id: session_id.clone(),
            project_id: project_id_info.id.clone(),
            started_at: now_iso8601(),
            cwd: cwd.to_path_buf(),
            agent,
            cli_version: cli_version.to_string(),
            git: git_info,
            acp_session_id,
        };

        // Spawn background writer
        tokio::spawn(transcript_writer(file, rx, session_meta));

        Ok(Self {
            tx,
            session_id,
            project_id: project_id_info.id,
            transcript_path,
        })
    }

    /// Record a user message.
    pub async fn record_user_message(
        &self,
        id: &str,
        content: &str,
        attachments: Vec<Attachment>,
    ) -> io::Result<()> {
        let entry = TranscriptEntry::User(UserEntry {
            id: id.to_string(),
            content: content.to_string(),
            attachments,
        });
        self.send_entry(entry).await
    }

    /// Record a tool call (when tool execution begins).
    pub async fn record_tool_call(
        &self,
        call_id: &str,
        name: &str,
        input: &serde_json::Value,
    ) -> io::Result<()> {
        let entry = TranscriptEntry::ToolCall(ToolCallEntry {
            call_id: call_id.to_string(),
            name: name.to_string(),
            input: input.clone(),
        });
        self.send_entry(entry).await
    }

    /// Record a normalized ACP-native client event.
    pub async fn record_client_event(&self, event: &NoriClientEvent) -> io::Result<()> {
        self.send_entry(TranscriptEntry::ClientEvent(ClientEventEntry {
            event: event.clone(),
        }))
        .await
    }

    /// Record a tool result (when tool execution completes).
    pub async fn record_tool_result(
        &self,
        call_id: &str,
        output: &str,
        truncated: bool,
        exit_code: Option<i32>,
    ) -> io::Result<()> {
        let entry = TranscriptEntry::ToolResult(ToolResultEntry {
            call_id: call_id.to_string(),
            output: output.to_string(),
            truncated,
            exit_code,
        });
        self.send_entry(entry).await
    }

    /// Record a complete assistant turn (after streaming finishes).
    pub async fn record_assistant_message(
        &self,
        id: &str,
        content: Vec<ContentBlock>,
        agent: Option<String>,
    ) -> io::Result<()> {
        let entry = TranscriptEntry::Assistant(AssistantEntry {
            id: id.to_string(),
            content,
            agent,
        });
        self.send_entry(entry).await
    }

    /// Record a patch operation (file edit/write/delete).
    pub async fn record_patch_apply(
        &self,
        call_id: &str,
        operation: PatchOperationType,
        path: &Path,
        success: bool,
        error: Option<String>,
    ) -> io::Result<()> {
        let entry = TranscriptEntry::PatchApply(PatchApplyEntry {
            call_id: call_id.to_string(),
            operation,
            path: path.to_path_buf(),
            success,
            error,
        });
        self.send_entry(entry).await
    }

    /// Flush all pending writes.
    pub async fn flush(&self) -> io::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(TranscriptCmd::Flush { ack: tx })
            .await
            .map_err(|e| IoError::other(format!("failed to queue transcript flush: {e}")))?;
        rx.await
            .map_err(|e| IoError::other(format!("failed waiting for transcript flush: {e}")))
    }

    /// Graceful shutdown.
    pub async fn shutdown(&self) -> io::Result<()> {
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

    /// Send an entry to the background writer.
    async fn send_entry(&self, entry: TranscriptEntry) -> io::Result<()> {
        self.tx
            .send(TranscriptCmd::Write(Box::new(entry)))
            .await
            .map_err(|e| IoError::other(format!("failed to queue transcript entry: {e}")))
    }
}

/// Background writer task that processes commands and writes to file.
async fn transcript_writer(
    mut file: File,
    mut rx: mpsc::Receiver<TranscriptCmd>,
    session_meta: SessionMetaEntry,
) -> io::Result<()> {
    // Write session metadata as the first line
    let meta_entry = TranscriptEntry::SessionMeta(session_meta);
    let line = TranscriptLine::new(meta_entry);
    write_line(&mut file, &line).await?;

    // Process commands
    while let Some(cmd) = rx.recv().await {
        match cmd {
            TranscriptCmd::Write(entry) => {
                let line = TranscriptLine::new(*entry);
                write_line(&mut file, &line).await?;
            }
            TranscriptCmd::Flush { ack } => {
                if let Err(e) = file.flush().await {
                    let _ = ack.send(());
                    return Err(e);
                }
                let _ = ack.send(());
            }
            TranscriptCmd::Shutdown { ack } => {
                let _ = file.flush().await;
                let _ = ack.send(());
                return Ok(());
            }
        }
    }

    Ok(())
}

/// Write a single JSONL line to the file.
async fn write_line(file: &mut File, line: &TranscriptLine) -> io::Result<()> {
    let mut json = serde_json::to_string(line)?;
    json.push('\n');
    file.write_all(json.as_bytes()).await?;
    file.flush().await?;
    Ok(())
}

/// Generate a UUID for the session ID.
fn generate_session_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Write project metadata to project.json.
async fn write_project_metadata(path: &Path, project_id: &ProjectId) -> io::Result<()> {
    use serde_json::json;

    let now = now_iso8601();

    let metadata = json!({
        "id": project_id.id,
        "name": project_id.name,
        "git_remote": project_id.git_remote,
        "git_root": project_id.git_root,
        "cwd": project_id.cwd,
        "created_at": now,
        "updated_at": now,
    });

    let content = serde_json::to_string_pretty(&metadata)?;
    tokio::fs::write(path, content).await
}

/// Collect git info for session metadata.
async fn collect_git_info(cwd: &Path) -> Option<GitInfo> {
    use tokio::process::Command;
    use tokio::time::Duration;
    use tokio::time::timeout;

    const GIT_TIMEOUT: Duration = Duration::from_secs(5);

    // Check if we're in a git repo
    let is_git = timeout(
        GIT_TIMEOUT,
        Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(cwd)
            .output(),
    )
    .await
    .ok()?
    .ok()?;

    if !is_git.status.success() {
        return None;
    }

    // Get branch and commit hash in parallel
    let (branch_result, commit_result) = tokio::join!(
        timeout(
            GIT_TIMEOUT,
            Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(cwd)
                .output()
        ),
        timeout(
            GIT_TIMEOUT,
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(cwd)
                .output()
        )
    );

    let branch = branch_result
        .ok()
        .and_then(std::result::Result::ok)
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| s != "HEAD");

    let commit_hash = commit_result
        .ok()
        .and_then(std::result::Result::ok)
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());

    Some(GitInfo {
        branch,
        commit_hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_transcript_recorder_creates_directory_structure() {
        let temp_dir = TempDir::new().unwrap();
        let nori_home = temp_dir.path();
        let cwd = temp_dir.path();

        let recorder = TranscriptRecorder::new(
            nori_home,
            cwd,
            Some("claude-code".to_string()),
            "0.1.0",
            None,
        )
        .await
        .unwrap();

        // Verify directory structure was created
        let project_dir = nori_home
            .join(TRANSCRIPTS_DIR)
            .join(BY_PROJECT_DIR)
            .join(recorder.project_id());
        assert!(project_dir.exists());
        assert!(project_dir.join(SESSIONS_DIR).exists());
        assert!(project_dir.join(PROJECT_METADATA_FILE).exists());

        // Verify transcript file was created
        assert!(recorder.transcript_path().exists());

        recorder.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_transcript_recorder_writes_session_meta() {
        let temp_dir = TempDir::new().unwrap();
        let nori_home = temp_dir.path();
        let cwd = temp_dir.path();

        let recorder = TranscriptRecorder::new(
            nori_home,
            cwd,
            Some("claude-code".to_string()),
            "0.1.0",
            None,
        )
        .await
        .unwrap();

        // Give the writer a moment to write the session meta
        recorder.flush().await.unwrap();
        recorder.shutdown().await.unwrap();

        // Read the transcript file
        let content = tokio::fs::read_to_string(recorder.transcript_path())
            .await
            .unwrap();
        let lines: Vec<&str> = content.lines().collect();

        assert!(!lines.is_empty());

        // First line should be session_meta
        let first_line: TranscriptLine = serde_json::from_str(lines[0]).unwrap();
        match first_line.entry {
            TranscriptEntry::SessionMeta(meta) => {
                assert_eq!(meta.session_id, recorder.session_id());
                assert_eq!(meta.project_id, recorder.project_id());
                assert_eq!(meta.cli_version, "0.1.0");
                assert_eq!(meta.agent, Some("claude-code".to_string()));
            }
            _ => panic!("Expected SessionMeta entry"),
        }
    }

    #[tokio::test]
    async fn test_transcript_recorder_records_user_message() {
        let temp_dir = TempDir::new().unwrap();
        let nori_home = temp_dir.path();
        let cwd = temp_dir.path();

        let recorder = TranscriptRecorder::new(nori_home, cwd, None, "0.1.0", None)
            .await
            .unwrap();

        recorder
            .record_user_message("msg-001", "Hello, world!", vec![])
            .await
            .unwrap();
        recorder.flush().await.unwrap();
        recorder.shutdown().await.unwrap();

        // Read the transcript file
        let content = tokio::fs::read_to_string(recorder.transcript_path())
            .await
            .unwrap();
        let lines: Vec<&str> = content.lines().collect();

        assert_eq!(lines.len(), 2); // SessionMeta + User message

        let user_line: TranscriptLine = serde_json::from_str(lines[1]).unwrap();
        match user_line.entry {
            TranscriptEntry::User(user) => {
                assert_eq!(user.id, "msg-001");
                assert_eq!(user.content, "Hello, world!");
            }
            _ => panic!("Expected User entry"),
        }
    }

    #[tokio::test]
    async fn test_transcript_recorder_records_tool_call_and_result() {
        let temp_dir = TempDir::new().unwrap();
        let nori_home = temp_dir.path();
        let cwd = temp_dir.path();

        let recorder = TranscriptRecorder::new(nori_home, cwd, None, "0.1.0", None)
            .await
            .unwrap();

        recorder
            .record_tool_call("call-001", "shell", &serde_json::json!({"command": "ls"}))
            .await
            .unwrap();
        recorder
            .record_tool_result("call-001", "file1.txt\nfile2.txt", false, Some(0))
            .await
            .unwrap();
        recorder.flush().await.unwrap();
        recorder.shutdown().await.unwrap();

        // Read the transcript file
        let content = tokio::fs::read_to_string(recorder.transcript_path())
            .await
            .unwrap();
        let lines: Vec<&str> = content.lines().collect();

        assert_eq!(lines.len(), 3); // SessionMeta + ToolCall + ToolResult

        let tool_call: TranscriptLine = serde_json::from_str(lines[1]).unwrap();
        match tool_call.entry {
            TranscriptEntry::ToolCall(call) => {
                assert_eq!(call.call_id, "call-001");
                assert_eq!(call.name, "shell");
            }
            _ => panic!("Expected ToolCall entry"),
        }

        let tool_result: TranscriptLine = serde_json::from_str(lines[2]).unwrap();
        match tool_result.entry {
            TranscriptEntry::ToolResult(result) => {
                assert_eq!(result.call_id, "call-001");
                assert_eq!(result.output, "file1.txt\nfile2.txt");
                assert_eq!(result.exit_code, Some(0));
            }
            _ => panic!("Expected ToolResult entry"),
        }
    }

    #[tokio::test]
    async fn test_transcript_recorder_records_client_event() {
        let temp_dir = TempDir::new().unwrap();
        let nori_home = temp_dir.path();
        let cwd = temp_dir.path();

        let recorder = TranscriptRecorder::new(nori_home, cwd, None, "0.1.0", None)
            .await
            .unwrap();

        let event = nori_protocol::ClientEvent::ToolSnapshot(nori_protocol::ToolSnapshot {
            call_id: "call-001".to_string(),
            title: "Edit /src/main.rs".to_string(),
            kind: nori_protocol::ToolKind::Edit,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: None,
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
        });

        recorder.record_client_event(&event).await.unwrap();
        recorder.flush().await.unwrap();
        recorder.shutdown().await.unwrap();

        let content = tokio::fs::read_to_string(recorder.transcript_path())
            .await
            .unwrap();
        let lines: Vec<&str> = content.lines().collect();

        assert_eq!(lines.len(), 2);

        let client_event_line: TranscriptLine = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(
            client_event_line.entry,
            TranscriptEntry::ClientEvent(ClientEventEntry { event })
        );
    }

    #[tokio::test]
    async fn test_transcript_recorder_records_assistant_message() {
        let temp_dir = TempDir::new().unwrap();
        let nori_home = temp_dir.path();
        let cwd = temp_dir.path();

        let recorder = TranscriptRecorder::new(nori_home, cwd, None, "0.1.0", None)
            .await
            .unwrap();

        recorder
            .record_assistant_message(
                "msg-002",
                vec![ContentBlock::Text {
                    text: "Here is my response.".to_string(),
                }],
                Some("claude-code".to_string()),
            )
            .await
            .unwrap();
        recorder.flush().await.unwrap();
        recorder.shutdown().await.unwrap();

        // Read the transcript file
        let content = tokio::fs::read_to_string(recorder.transcript_path())
            .await
            .unwrap();
        let lines: Vec<&str> = content.lines().collect();

        assert_eq!(lines.len(), 2);

        let assistant_line: TranscriptLine = serde_json::from_str(lines[1]).unwrap();
        match assistant_line.entry {
            TranscriptEntry::Assistant(assistant) => {
                assert_eq!(assistant.id, "msg-002");
                assert_eq!(assistant.content.len(), 1);
                assert_eq!(assistant.agent, Some("claude-code".to_string()));
            }
            _ => panic!("Expected Assistant entry"),
        }
    }

    #[tokio::test]
    async fn test_transcript_recorder_full_conversation() {
        let temp_dir = TempDir::new().unwrap();
        let nori_home = temp_dir.path();
        let cwd = temp_dir.path();

        let recorder = TranscriptRecorder::new(
            nori_home,
            cwd,
            Some("claude-code".to_string()),
            "0.1.0",
            None,
        )
        .await
        .unwrap();

        // Simulate a full conversation
        recorder
            .record_user_message("msg-001", "What files are in src?", vec![])
            .await
            .unwrap();
        recorder
            .record_tool_call(
                "call-001",
                "shell",
                &serde_json::json!({"command": "ls src/"}),
            )
            .await
            .unwrap();
        recorder
            .record_tool_result("call-001", "main.rs\nlib.rs", false, Some(0))
            .await
            .unwrap();
        recorder
            .record_assistant_message(
                "msg-002",
                vec![ContentBlock::Text {
                    text: "The src directory contains main.rs and lib.rs.".to_string(),
                }],
                Some("claude-code".to_string()),
            )
            .await
            .unwrap();

        recorder.flush().await.unwrap();
        recorder.shutdown().await.unwrap();

        // Read and verify
        let content = tokio::fs::read_to_string(recorder.transcript_path())
            .await
            .unwrap();
        let lines: Vec<&str> = content.lines().collect();

        // SessionMeta + User + ToolCall + ToolResult + Assistant = 5 lines
        assert_eq!(lines.len(), 5);

        // Verify all lines are valid JSON
        for line in &lines {
            let _: TranscriptLine = serde_json::from_str(line).unwrap();
        }
    }

    #[tokio::test]
    async fn test_transcript_recorder_records_patch_apply() {
        use super::super::types::PatchOperationType;

        let temp_dir = TempDir::new().unwrap();
        let nori_home = temp_dir.path();
        let cwd = temp_dir.path();

        let recorder = TranscriptRecorder::new(nori_home, cwd, None, "0.1.0", None)
            .await
            .unwrap();

        // Record a successful edit
        recorder
            .record_patch_apply(
                "call-edit-001",
                PatchOperationType::Edit,
                &PathBuf::from("/src/main.rs"),
                true,
                None,
            )
            .await
            .unwrap();

        // Record a failed write
        recorder
            .record_patch_apply(
                "call-write-001",
                PatchOperationType::Write,
                &PathBuf::from("/src/new_file.rs"),
                false,
                Some("Permission denied".to_string()),
            )
            .await
            .unwrap();

        recorder.flush().await.unwrap();
        recorder.shutdown().await.unwrap();

        // Read the transcript file
        let content = tokio::fs::read_to_string(recorder.transcript_path())
            .await
            .unwrap();
        let lines: Vec<&str> = content.lines().collect();

        assert_eq!(lines.len(), 3); // SessionMeta + 2 PatchApply entries

        let edit_line: TranscriptLine = serde_json::from_str(lines[1]).unwrap();
        match edit_line.entry {
            TranscriptEntry::PatchApply(patch) => {
                assert_eq!(patch.call_id, "call-edit-001");
                assert_eq!(patch.operation, PatchOperationType::Edit);
                assert_eq!(patch.path, PathBuf::from("/src/main.rs"));
                assert!(patch.success);
                assert!(patch.error.is_none());
            }
            _ => panic!("Expected PatchApply entry"),
        }

        let write_line: TranscriptLine = serde_json::from_str(lines[2]).unwrap();
        match write_line.entry {
            TranscriptEntry::PatchApply(patch) => {
                assert_eq!(patch.call_id, "call-write-001");
                assert_eq!(patch.operation, PatchOperationType::Write);
                assert_eq!(patch.path, PathBuf::from("/src/new_file.rs"));
                assert!(!patch.success);
                assert_eq!(patch.error, Some("Permission denied".to_string()));
            }
            _ => panic!("Expected PatchApply entry"),
        }
    }
}
