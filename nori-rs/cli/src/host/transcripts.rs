//! Sessions this host has not run, served from their transcripts on disk.
//!
//! The manager only knows sessions started since the host began, and its
//! journal is bounded. These helpers list, replay, and resume the sessions
//! recorded under `$NORI_HOME/transcripts` by earlier hosts and local runs.

use std::collections::HashSet;
use std::path::Path;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use nori_harness::ConversationId;
use nori_harness::StoredSession;
use nori_harness::TranscriptLoader;
use nori_harness::managed_launch::session_folder;
use nori_harness::session_manager::ManagedSession;
use nori_harness::transcript::Transcript;
use nori_harness::transcript::load_transcript_from_path;
use serde_json::Value;
use serde_json::json;

use super::service::HostService;

const MAX_TITLE_CHARS: usize = 200;

pub(super) fn nanos(time: SystemTime) -> i128 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as i128)
}

/// Stored sessions not in `running`, newest write first, strictly after the
/// `(mtime nanos, id)` key when one is given.
pub(super) async fn stored_sessions(
    nori_home: &Path,
    running: &HashSet<String>,
    after: Option<(i128, String)>,
) -> Vec<StoredSession> {
    let loader = TranscriptLoader::new(nori_home.to_path_buf());
    // A damaged transcripts folder must not make live sessions unlistable.
    let stored = loader.list_stored_sessions().await.unwrap_or_else(|error| {
        tracing::warn!(%error, "could not list stored transcripts");
        Vec::new()
    });
    stored
        .into_iter()
        .filter(|stored| !running.contains(&stored.session_id))
        .filter(|stored| {
            after.as_ref().is_none_or(|(time, id)| {
                (nanos(stored.updated_at), &stored.session_id) < (*time, id)
            })
        })
        .collect()
}

/// The ACP `SessionInfo` of a stored session, or `None` when its transcript
/// cannot be read.
pub(super) async fn stored_row(stored: &StoredSession) -> Option<Value> {
    let metadata = match stored.metadata().await {
        Ok(metadata) => metadata,
        Err(error) => {
            tracing::warn!(path = %stored.path.display(), %error, "skipping unreadable transcript");
            return None;
        }
    };
    let title = stored
        .first_user_preview()
        .await
        .ok()
        .flatten()
        .and_then(|preview| {
            preview
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(|line| line.chars().take(MAX_TITLE_CHARS).collect::<String>())
        });
    let updated_at = chrono::DateTime::<chrono::Utc>::from(stored.updated_at)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    Some(json!({
        "sessionId": metadata.session_id,
        "cwd": metadata.cwd,
        "title": title,
        "updatedAt": updated_at,
    }))
}

/// A stored session's transcript, when one has this ID.
pub(super) async fn load_stored(
    nori_home: &Path,
    id: &ConversationId,
) -> anyhow::Result<Option<Transcript>> {
    let loader = TranscriptLoader::new(nori_home.to_path_buf());
    let Some(stored) = loader.find_stored_session(&id.to_string()).await? else {
        return Ok(None);
    };
    let transcript = load_transcript_from_path(&stored.path).await?;
    Ok((transcript.meta.session_id == id.to_string()).then_some(transcript))
}

impl HostService {
    /// Resume a session from its transcript under its own ID, in its
    /// recorded folder and with its recorded agent.
    pub(super) async fn resume_stored(
        &self,
        id: &ConversationId,
        mut transcript: Transcript,
    ) -> anyhow::Result<ManagedSession> {
        let mut config = self
            .config_in(session_folder(&transcript.meta.cwd)?)
            .await?;
        match &transcript.meta.agent {
            Some(agent) => config.active_agent = agent.clone(),
            // The host's default agent never issued this ACP session ID, so
            // resume by transcript replay instead of `session/load`.
            None => transcript.meta.acp_session_id = None,
        }
        Ok(self
            .manager
            .resume_stored(*id, transcript, Self::prepare(config))?)
    }
}
