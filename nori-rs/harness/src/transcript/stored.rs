//! Discovery of every session transcript on disk, across all projects.

use std::io;
use std::path::PathBuf;
use std::time::SystemTime;

use super::SESSIONS_DIR;
use super::loader::SessionMetadata;
use super::loader::TranscriptLoader;
use super::loader::load_first_user_preview_from_path;
use super::loader::load_session_metadata;

/// A session transcript file on disk, found without opening it.
#[derive(Debug, Clone)]
pub struct StoredSession {
    /// The session ID, from the file name.
    pub session_id: String,
    pub project_id: String,
    pub path: PathBuf,
    /// When the file was last written.
    pub updated_at: SystemTime,
}

impl StoredSession {
    /// The transcript's metadata line. Fails when it is unreadable or names
    /// a different session than the file name.
    pub async fn metadata(&self) -> io::Result<SessionMetadata> {
        let metadata = load_session_metadata(&self.path, &self.project_id).await?;
        if metadata.session_id != self.session_id {
            return Err(io::Error::other(format!(
                "transcript {} records session {}",
                self.path.display(),
                metadata.session_id
            )));
        }
        Ok(metadata)
    }

    /// The first user message, read without loading the whole transcript.
    pub async fn first_user_preview(&self) -> io::Result<Option<String>> {
        load_first_user_preview_from_path(&self.path).await
    }
}

impl TranscriptLoader {
    /// Every `transcripts/by-project/*/sessions/<uuid>.jsonl` file, most
    /// recently written first. Only reads directories and file times;
    /// unreadable entries are logged and skipped.
    pub async fn list_stored_sessions(&self) -> io::Result<Vec<StoredSession>> {
        let mut projects = match tokio::fs::read_dir(self.transcripts_base()).await {
            Ok(projects) => projects,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut sessions = Vec::new();
        while let Some(project) = next_entry(&mut projects).await {
            let project_id = project.file_name().to_string_lossy().into_owned();
            let Ok(mut files) = tokio::fs::read_dir(project.path().join(SESSIONS_DIR)).await else {
                continue;
            };
            while let Some(file) = next_entry(&mut files).await {
                let path = file.path();
                // Only `<canonical uuid>.jsonl` files are session transcripts.
                let Some(session_id) = path
                    .extension()
                    .is_some_and(|extension| extension == "jsonl")
                    .then(|| path.file_stem()?.to_str())
                    .flatten()
                    .filter(|stem| {
                        uuid::Uuid::parse_str(stem).is_ok_and(|id| id.to_string() == *stem)
                    })
                    .map(str::to_owned)
                else {
                    continue;
                };
                if let Some(session) = stored(path, session_id, &project_id).await {
                    sessions.push(session);
                }
            }
        }
        sessions.sort_by(|a, b| (b.updated_at, &b.session_id).cmp(&(a.updated_at, &a.session_id)));
        Ok(sessions)
    }

    /// The transcript file of one session, found by its file name in any
    /// project. Returns `None` for an ID that is not a UUID.
    pub async fn find_stored_session(&self, session_id: &str) -> io::Result<Option<StoredSession>> {
        let Ok(session_id) = uuid::Uuid::parse_str(session_id) else {
            return Ok(None);
        };
        let session_id = session_id.to_string();
        let mut projects = match tokio::fs::read_dir(self.transcripts_base()).await {
            Ok(projects) => projects,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        while let Some(project) = next_entry(&mut projects).await {
            let path = project
                .path()
                .join(SESSIONS_DIR)
                .join(format!("{session_id}.jsonl"));
            let project_id = project.file_name().to_string_lossy().into_owned();
            if let Some(session) = stored(path, session_id.clone(), &project_id).await {
                return Ok(Some(session));
            }
        }
        Ok(None)
    }
}

async fn next_entry(entries: &mut tokio::fs::ReadDir) -> Option<tokio::fs::DirEntry> {
    entries
        .next_entry()
        .await
        .inspect_err(|error| tracing::warn!(%error, "stopped reading a transcript folder"))
        .ok()
        .flatten()
}

async fn stored(path: PathBuf, session_id: String, project_id: &str) -> Option<StoredSession> {
    let metadata = tokio::fs::metadata(&path).await.ok()?;
    if !metadata.is_file() {
        return None;
    }
    Some(StoredSession {
        session_id,
        project_id: project_id.to_owned(),
        updated_at: metadata.modified().ok()?,
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::TranscriptRecorder;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    async fn record(home: &TempDir, cwd: &std::path::Path, text: &str) -> String {
        let recorder = TranscriptRecorder::new(
            home.path(),
            cwd,
            Some("mock-model".to_owned()),
            "0.1.0",
            None,
        )
        .await
        .unwrap();
        recorder
            .record_user_message("m", text, Vec::new())
            .await
            .unwrap();
        recorder.flush().await.unwrap();
        let id = recorder.session_id().to_owned();
        recorder.shutdown().await.unwrap();
        id
    }

    #[tokio::test]
    async fn sessions_across_projects_are_listed_and_found_by_id() {
        let home = TempDir::new().unwrap();
        let (one, two) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let first = record(&home, one.path(), "first").await;
        let second = record(&home, two.path(), "second").await;
        let base = home.path().join("transcripts/by-project");
        std::fs::write(base.join("junk.jsonl"), "not a project").unwrap();

        let loader = TranscriptLoader::new(home.path().to_path_buf());
        let mut listed = loader
            .list_stored_sessions()
            .await
            .unwrap()
            .into_iter()
            .map(|session| session.session_id)
            .collect::<Vec<_>>();
        listed.sort();
        let mut expected = vec![first.clone(), second];
        expected.sort();
        assert_eq!(listed, expected);

        let found = loader.find_stored_session(&first).await.unwrap().unwrap();
        assert_eq!(found.metadata().await.unwrap().cwd, one.path());
        assert_eq!(
            found.first_user_preview().await.unwrap().as_deref(),
            Some("first")
        );
        assert!(
            loader
                .find_stored_session("../../etc/passwd")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_copied_transcript_does_not_pass_as_another_session() {
        let home = TempDir::new().unwrap();
        let cwd = TempDir::new().unwrap();
        let original = record(&home, cwd.path(), "first").await;
        let loader = TranscriptLoader::new(home.path().to_path_buf());
        let source = loader
            .find_stored_session(&original)
            .await
            .unwrap()
            .unwrap();
        let copy = uuid::Uuid::new_v4().to_string();
        std::fs::copy(
            &source.path,
            source.path.with_file_name(format!("{copy}.jsonl")),
        )
        .unwrap();

        let copied = loader.find_stored_session(&copy).await.unwrap().unwrap();

        assert!(copied.metadata().await.is_err());
    }

    #[tokio::test]
    async fn a_missing_transcripts_folder_lists_nothing() {
        let home = TempDir::new().unwrap();
        let loader = TranscriptLoader::new(home.path().to_path_buf());
        assert!(loader.list_stored_sessions().await.unwrap().is_empty());
        assert!(
            loader
                .find_stored_session(&uuid::Uuid::new_v4().to_string())
                .await
                .unwrap()
                .is_none()
        );
    }
}
