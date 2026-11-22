//! Sessions this host has not run, served from their transcripts on disk.
//!
//! The manager only knows sessions started since the host began, and its
//! journal is bounded. These helpers list, replay, and resume the sessions
//! recorded under `$NORI_HOME/transcripts` by earlier hosts and local runs.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use anyhow::Context;
use nori_harness::ConversationId;
use nori_harness::TranscriptLoader;
use nori_harness::managed_launch::session_folder;
use nori_harness::remote_agent::ManagedSubscription;
use nori_harness::transcript::load_transcript_from_path;
use nori_protocol::NoriEvent;
use nori_protocol::ReplaySource;
use nori_protocol::ReplayStarted;
use nori_protocol::SessionEnvelope;
use nori_protocol::SessionEvent;
use serde_json::Value;
use serde_json::json;

use super::service::HostService;

const DEFAULT_PAGE: i64 = 50;
const MAX_PAGE: i64 = 200;
const MAX_TITLE_CHARS: usize = 200;

/// Parameters `_nori/session/list` refuses with `-32602`.
#[derive(Debug)]
pub(super) struct InvalidListParams(String);

impl std::fmt::Display for InvalidListParams {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for InvalidListParams {}

fn nanos(time: SystemTime) -> i128 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as i128)
}

/// One page of stored sessions not in `running`, newest write first, and the
/// cursor of the next page when more remain.
pub(super) async fn list(
    nori_home: &Path,
    running: &HashSet<String>,
    params: &Value,
) -> anyhow::Result<(Vec<Value>, Option<String>)> {
    let limit = match params.get("transcript_limit") {
        None | Some(Value::Null) => DEFAULT_PAGE,
        Some(limit) => limit
            .as_i64()
            .filter(|limit| (1..=MAX_PAGE).contains(limit))
            .ok_or_else(|| {
                InvalidListParams(format!(
                    "transcript_limit must be an integer from 1 to {MAX_PAGE}"
                ))
            })?,
    };
    let after = match params.get("transcript_cursor") {
        None | Some(Value::Null) => None,
        Some(cursor) => Some(
            cursor
                .as_str()
                .and_then(|cursor| cursor.split_once(':'))
                .and_then(|(time, id)| Some((time.parse::<i128>().ok()?, id.to_owned())))
                .ok_or_else(|| {
                    InvalidListParams(
                        "transcript_cursor must be a transcripts_next_cursor value".to_owned(),
                    )
                })?,
        ),
    };
    let loader = TranscriptLoader::new(nori_home.to_path_buf());
    // Stored sessions supplement the reply; a damaged transcripts folder must
    // not make the host's own sessions unlistable.
    let stored = loader.list_stored_sessions().await.unwrap_or_else(|error| {
        tracing::warn!(%error, "could not list stored transcripts");
        Vec::new()
    });
    let mut remaining = stored
        .into_iter()
        .filter(|stored| !running.contains(&stored.session_id))
        .filter(|stored| {
            after.as_ref().is_none_or(|(time, id)| {
                (nanos(stored.updated_at), &stored.session_id) < (*time, id)
            })
        })
        .peekable();
    let mut rows = Vec::new();
    let mut last = None;
    while rows.len() < limit as usize
        && let Some(stored) = remaining.next()
    {
        let metadata = match stored.metadata().await {
            Ok(metadata) => metadata,
            Err(error) => {
                tracing::warn!(path = %stored.path.display(), %error, "skipping unreadable transcript");
                continue;
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
        rows.push(json!({
            "session_id": metadata.session_id,
            "cwd": metadata.cwd,
            "agent": metadata.agent,
            "title": title,
            "started_at": metadata.started_at,
            "updated_at": updated_at,
        }));
        last = Some(stored);
    }
    let next = remaining
        .peek()
        .and(last)
        .map(|last| format!("{}:{}", nanos(last.updated_at), last.session_id));
    Ok((rows, next))
}

/// Replay a stored session's transcript, numbered from 1 and bracketed by
/// `replay_started`/`replay_finished`, strictly after the client's cursor.
/// `None` when no transcript has this ID.
pub(super) async fn subscribe(
    nori_home: &Path,
    id: &ConversationId,
    cursors: &HashMap<String, i64>,
) -> anyhow::Result<Option<ManagedSubscription>> {
    let loader = TranscriptLoader::new(nori_home.to_path_buf());
    let Some(stored) = loader.find_stored_session(&id.to_string()).await? else {
        return Ok(None);
    };
    let transcript = load_transcript_from_path(&stored.path).await?;
    let conversation_id = id.to_string();
    if transcript.meta.session_id != conversation_id {
        return Ok(None);
    }
    let after = cursors.get(&conversation_id).copied().unwrap_or(0);
    let agent = transcript.meta.agent.clone().unwrap_or_default();
    let events = std::iter::once(SessionEvent::Nori(NoriEvent::ReplayStarted(
        ReplayStarted {
            source: ReplaySource::Transcript,
        },
    )))
    .chain(nori_harness::backend::transcript_to_replay_session_events(
        &transcript,
    ))
    .chain(std::iter::once(SessionEvent::Nori(
        NoriEvent::ReplayFinished,
    )));
    let replay = (1..)
        .zip(events)
        .filter(|(seq, _)| *seq > after)
        .map(|(seq, event)| SessionEnvelope {
            conversation_id: conversation_id.clone(),
            agent: agent.clone(),
            seq,
            event,
        })
        .collect();
    // A stored session has no live events. Keep the stream open until the
    // client replaces or drops this subscription, since a closed stream ends
    // the connection.
    let (sender, events) = tokio::sync::mpsc::channel(1);
    tokio::spawn(async move { sender.closed().await });
    Ok(Some(ManagedSubscription { replay, events }))
}

impl HostService {
    /// Resume a session from its transcript under its own ID, in its
    /// recorded folder and with its recorded agent.
    pub(super) async fn resume_stored(&self, id: &ConversationId) -> anyhow::Result<Value> {
        let loader = TranscriptLoader::new(self.config.nori_home.clone());
        let stored = loader
            .find_stored_session(&id.to_string())
            .await?
            .context("unknown session id")?;
        let mut transcript = load_transcript_from_path(&stored.path).await?;
        let mut config = self
            .config_in(session_folder(&transcript.meta.cwd)?)
            .await?;
        match &transcript.meta.agent {
            Some(agent) => config.active_agent = agent.clone(),
            // The host's default agent never issued this ACP session ID, so
            // resume by transcript replay instead of `session/load`.
            None => transcript.meta.acp_session_id = None,
        }
        self.manager
            .resume_stored(*id, transcript, Self::prepare(config))?;
        Ok(json!({"session_id": id.to_string()}))
    }
}
