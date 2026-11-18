//! TranscriptRecorder - Records transcript entries for a session.
//!
//! Uses async channel for non-blocking writes (same pattern as core RolloutRecorder).

use std::io;
use std::io::Error as IoError;
use std::path::Path;
use std::path::PathBuf;

use serde_json;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncSeekExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;

use super::BY_PROJECT_DIR;
use super::PROJECT_METADATA_FILE;
use super::SESSIONS_DIR;
use super::TRANSCRIPTS_DIR;
use super::Transcript;
use super::project::ProjectId;
use super::project::compute_project_id;
use super::sync_retry::SyncRetry;
use super::types::Attachment;
use super::types::GitInfo;
use super::types::SessionEventEntry;
use super::types::SessionMetaEntry;
use super::types::TranscriptEntry;
use super::types::TranscriptLine;
use super::types::UserEntry;
use super::types::now_iso8601;

/// Commands sent to the background writer task.
enum TranscriptCmd {
    Write(Box<TranscriptEntry>),
    Flush {
        durable: bool,
        ack: oneshot::Sender<io::Result<()>>,
    },
    Checkpoint {
        ack: oneshot::Sender<io::Result<String>>,
    },
    Shutdown {
        ack: oneshot::Sender<io::Result<()>>,
    },
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
    /// Continue the same Nori session without exposing a truncated transcript.
    /// The replacement inode also isolates any writer retained by the old runtime.
    pub(crate) async fn resume_same_session(
        nori_home: &Path,
        transcript: &Transcript,
        acp_session_id: String,
        agent_history: Option<Vec<nori_protocol::SessionEvent>>,
    ) -> io::Result<Self> {
        let mut meta = transcript.meta.clone();
        meta.acp_session_id = Some(acp_session_id);
        let sessions_dir = nori_home
            .join(TRANSCRIPTS_DIR)
            .join(BY_PROJECT_DIR)
            .join(&meta.project_id)
            .join(SESSIONS_DIR);
        tokio::fs::create_dir_all(&sessions_dir).await?;
        let transcript_path = sessions_dir.join(format!("{}.jsonl", meta.session_id));
        let temporary = tempfile::Builder::new()
            .prefix(".resume-")
            .tempfile_in(&sessions_dir)?;
        let (file, temporary_path) = temporary.into_parts();
        let mut file = File::from_std(file);
        let mut metadata_line = transcript.entries[0].clone();
        metadata_line.entry = TranscriptEntry::SessionMeta(meta.clone());
        let mut batch = serialize_line(&metadata_line)?;
        if let Some(events) = agent_history {
            for evidence in transcript.archived_context_evidence() {
                batch.push_str(&serialize_line(&TranscriptLine::new(
                    TranscriptEntry::SessionEvent(evidence),
                ))?);
            }
            let replay_start = nori_protocol::SessionEvent::Nori(
                nori_protocol::NoriEvent::ReplayStarted(nori_protocol::ReplayStarted {
                    source: nori_protocol::ReplaySource::Agent,
                }),
            );
            let replay_end =
                nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ReplayFinished);
            for event in std::iter::once(replay_start)
                .chain(events)
                .chain(std::iter::once(replay_end))
            {
                batch.push_str(&serialize_line(&TranscriptLine::new(
                    TranscriptEntry::SessionEvent(SessionEventEntry::new(event)),
                ))?);
            }
        } else {
            for line in transcript.entries.iter().skip(1) {
                batch.push_str(&serialize_line(line)?);
            }
        }
        file.write_all(batch.as_bytes()).await?;
        file.flush().await?;
        file.sync_data().await?;
        tokio::fs::rename(&temporary_path, &transcript_path).await?;
        let (tx, rx) = mpsc::channel::<TranscriptCmd>(256);
        tokio::spawn(run_writer_loop(file, rx, None));
        Ok(Self {
            tx,
            session_id: meta.session_id,
            project_id: meta.project_id,
            transcript_path,
        })
    }

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
        Self::new_with_session_id(nori_home, cwd, agent, cli_version, acp_session_id, None).await
    }

    /// Initialize for a new session whose ID was chosen by the caller, such as
    /// the session manager's conversation ID. `None` mints a fresh UUID.
    pub async fn new_with_session_id(
        nori_home: &Path,
        cwd: &Path,
        agent: Option<String>,
        cli_version: &str,
        acp_session_id: Option<String>,
        session_id: Option<String>,
    ) -> io::Result<Self> {
        Self::create(
            nori_home,
            cwd,
            agent,
            cli_version,
            acp_session_id,
            session_id,
            false,
        )
        .await
    }

    pub(crate) async fn create(
        nori_home: &Path,
        cwd: &Path,
        agent: Option<String>,
        cli_version: &str,
        acp_session_id: Option<String>,
        session_id: Option<String>,
        retry_sync: bool,
    ) -> io::Result<Self> {
        // Compute project ID from cwd
        let project_id_info = compute_project_id(cwd).await?;

        let session_id = session_id.unwrap_or_else(generate_session_id);

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
            .read(true)
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
            forked_from: None,
        };

        // Spawn background writer
        tokio::spawn(transcript_writer(
            file,
            rx,
            session_meta,
            retry_sync.then(SyncRetry::default),
        ));

        Ok(Self {
            tx,
            session_id,
            project_id: project_id_info.id,
            transcript_path,
        })
    }

    /// Initialize a forked session seeded from a parent transcript.
    ///
    /// Mints a fresh session UUID, opens a new transcript file, and writes the
    /// `SessionMeta` (recording `forked_from` lineage and the new ACP session
    /// id) followed by `seed_entries` in a single batched write before
    /// returning, so the fork does not incur a per-line fsync storm on large
    /// parents.
    pub async fn new_forked(
        nori_home: &Path,
        cwd: &Path,
        agent: Option<String>,
        cli_version: &str,
        new_acp_session_id: String,
        forked_from: String,
        seed_entries: Vec<TranscriptEntry>,
    ) -> io::Result<Self> {
        let project_id_info = compute_project_id(cwd).await?;
        let session_id = generate_session_id();

        let project_dir = nori_home
            .join(TRANSCRIPTS_DIR)
            .join(BY_PROJECT_DIR)
            .join(&project_id_info.id);
        let sessions_dir = project_dir.join(SESSIONS_DIR);
        tokio::fs::create_dir_all(&sessions_dir).await?;

        let project_meta_path = project_dir.join(PROJECT_METADATA_FILE);
        write_project_metadata(&project_meta_path, &project_id_info).await?;

        let transcript_path = sessions_dir.join(format!("{session_id}.jsonl"));
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(true)
            .open(&transcript_path)
            .await?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            tokio::fs::set_permissions(&transcript_path, perms).await?;
        }

        let git_info = collect_git_info(cwd).await;
        let session_meta = SessionMetaEntry {
            session_id: session_id.clone(),
            project_id: project_id_info.id.clone(),
            started_at: now_iso8601(),
            cwd: cwd.to_path_buf(),
            agent,
            cli_version: cli_version.to_string(),
            git: git_info,
            acp_session_id: Some(new_acp_session_id),
            forked_from: Some(forked_from),
        };

        // Batch the metadata line plus every seeded entry into a single write.
        let mut batch = serialize_line(&TranscriptLine::new(TranscriptEntry::SessionMeta(
            session_meta,
        )))?;
        for entry in seed_entries {
            batch.push_str(&serialize_line(&TranscriptLine::new(entry))?);
        }
        file.write_all(batch.as_bytes()).await?;
        file.flush().await?;

        let (tx, rx) = mpsc::channel::<TranscriptCmd>(256);
        tokio::spawn(run_writer_loop(file, rx, None));

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

    /// Record the exact public event delivered across the Harness boundary.
    pub(crate) async fn record_session_event(&self, event: &SessionEventEntry) -> io::Result<()> {
        self.send_entry(TranscriptEntry::SessionEvent(event.clone()))
            .await
    }

    /// Flush all pending writes.
    pub async fn flush(&self) -> io::Result<()> {
        self.flush_writer(false).await
    }

    pub(crate) async fn durable_flush(&self) -> io::Result<()> {
        self.flush_writer(true).await
    }

    async fn flush_writer(&self, durable: bool) -> io::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(TranscriptCmd::Flush { durable, ack: tx })
            .await
            .map_err(|e| IoError::other(format!("failed to queue transcript flush: {e}")))?;
        rx.await
            .map_err(|e| IoError::other(format!("failed waiting for transcript flush: {e}")))?
    }

    pub(crate) async fn checkpoint(&self) -> io::Result<String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(TranscriptCmd::Checkpoint { ack: tx })
            .await
            .map_err(|error| {
                IoError::other(format!("could not queue transcript checkpoint: {error}"))
            })?;
        rx.await.map_err(|error| {
            IoError::other(format!("transcript checkpoint interrupted: {error}"))
        })?
    }

    /// Graceful shutdown.
    pub async fn shutdown(&self) -> io::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(TranscriptCmd::Shutdown { ack: tx })
            .await
            .map_err(|e| IoError::other(format!("failed to queue transcript shutdown: {e}")))?;
        rx.await
            .map_err(|e| IoError::other(format!("failed waiting for transcript shutdown: {e}")))?
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
    rx: mpsc::Receiver<TranscriptCmd>,
    session_meta: SessionMetaEntry,
    mut retry: Option<SyncRetry>,
) -> io::Result<()> {
    // Write session metadata as the first line
    let meta_entry = TranscriptEntry::SessionMeta(session_meta);
    let line = TranscriptLine::new(meta_entry);
    write_line(&mut file, &line, retry.as_mut()).await?;

    run_writer_loop(file, rx, retry).await
}

/// Process writer commands against an already-opened, already-seeded file.
async fn run_writer_loop(
    mut file: File,
    mut rx: mpsc::Receiver<TranscriptCmd>,
    mut retry: Option<SyncRetry>,
) -> io::Result<()> {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            TranscriptCmd::Write(entry) => {
                let line = TranscriptLine::new(*entry);
                write_line(&mut file, &line, retry.as_mut()).await?;
            }
            TranscriptCmd::Flush { durable, ack } => {
                let result = flush_file(&mut file, durable, retry.as_mut()).await;
                let failed = result.is_err()
                    && !(durable && retry.as_ref().is_some_and(SyncRetry::can_retry));
                let _ = ack.send(result);
                if failed {
                    break;
                }
            }
            TranscriptCmd::Checkpoint { ack } => {
                if let Err(error) = flush_file(&mut file, true, retry.as_mut()).await {
                    let _ = ack.send(Err(error));
                    if retry.as_ref().is_some_and(|state| !state.can_retry()) {
                        break;
                    }
                    continue;
                }
                let result = async {
                    file.rewind().await?;
                    let mut data = String::new();
                    file.read_to_string(&mut data).await?;
                    Ok(data)
                }
                .await;
                // A failed read can leave the next write positioned inside the transcript.
                if result.is_err() && file.seek(std::io::SeekFrom::End(0)).await.is_err() {
                    let _ = ack.send(result);
                    break;
                }
                let _ = ack.send(result);
            }
            TranscriptCmd::Shutdown { ack } => {
                let _ = ack.send(file.flush().await);
                return Ok(());
            }
        }
    }

    Ok(())
}

async fn flush_file(
    file: &mut File,
    durable: bool,
    retry: Option<&mut SyncRetry>,
) -> io::Result<()> {
    if durable && let Some(retry) = retry {
        return retry.sync(file).await;
    }
    file.flush().await?;
    if durable {
        file.sync_data().await?;
    }
    Ok(())
}

/// Serialize a transcript line into its newline-terminated JSONL form.
fn serialize_line(line: &TranscriptLine) -> io::Result<String> {
    let mut json = serde_json::to_string(line)?;
    json.push('\n');
    Ok(json)
}

/// Write a single JSONL line to the file.
async fn write_line(
    file: &mut File,
    line: &TranscriptLine,
    retry: Option<&mut SyncRetry>,
) -> io::Result<()> {
    let json = serialize_line(line)?;
    file.write_all(json.as_bytes()).await?;
    file.flush().await?;
    if let Some(retry) = retry {
        retry.appended(json.as_bytes());
    }
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
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn checkpoint_refuses_an_unsyncable_transcript() -> anyhow::Result<()> {
    let file = File::options().write(true).open("/dev/null").await?;
    let (tx, rx) = mpsc::channel(2);
    let recorder = TranscriptRecorder {
        tx,
        session_id: "fixture".into(),
        project_id: "fixture".into(),
        transcript_path: "/dev/null".into(),
    };
    let writer = tokio::spawn(run_writer_loop(file, rx, None));
    assert!(
        recorder.checkpoint().await.is_err(),
        "fsync failed but checkpoint succeeded"
    );
    assert!(
        recorder.durable_flush().await.is_err(),
        "fsync failed but binding recorder barrier succeeded"
    );
    drop(recorder);
    writer.await??;
    Ok(())
}
