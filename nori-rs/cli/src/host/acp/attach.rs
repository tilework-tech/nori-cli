//! `session/load` and `session/resume`: attach a connection to a live or
//! stored session and forward its updates from the manager journal.

use std::collections::HashMap;
use std::path::Path;

use nori_harness::ConversationId;
use nori_harness::remote_agent::HostConnection;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEnvelope;
use nori_protocol::SessionEvent;
use nori_protocol::SessionStatus;
use nori_protocol::acp::v1 as acp;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;

use super::AcpError;
use crate::host::service::HostService;
use crate::host::transcripts;

/// Envelopes an attached connection may lag behind its session before the
/// manager drops it.
const ATTACHED_QUEUE: i32 = 1024;

pub(super) const SEQ_META: &str = "nori.dev/seq";

/// The outward `session/update` for a session event, if it is one.
pub(super) fn session_update(
    session: &str,
    event: &SessionEvent,
    seq: Option<i64>,
) -> Option<Value> {
    let SessionEvent::Acp(AcpEvent::Notification(acp::AgentNotification::SessionNotification(
        notification,
    ))) = event
    else {
        return None;
    };
    let mut params = serde_json::to_value(notification).ok()?;
    params["sessionId"] = json!(session);
    if let Some(seq) = seq {
        params["_meta"][SEQ_META] = json!(seq);
    }
    Some(json!({"jsonrpc":"2.0","method":"session/update","params":params}))
}

/// Forwards one session's envelopes to one connection.
struct Forwarder {
    connection: HostConnection,
    session: String,
    /// Drop history the harness replays on resume: the client either asked
    /// for none or already received it from the transcript.
    skip_replays: bool,
    replaying: bool,
    started: bool,
}

impl Forwarder {
    /// Deliver one envelope. Returns `Some(result)` when the session
    /// finished starting (`Ok`) or ended before it did (`Err`).
    fn deliver(&mut self, envelope: &SessionEnvelope) -> Option<Result<(), String>> {
        match &envelope.event {
            SessionEvent::Nori(NoriEvent::ReplayStarted(_)) => self.replaying = true,
            SessionEvent::Nori(NoriEvent::ReplayFinished) => self.replaying = false,
            SessionEvent::Nori(NoriEvent::SessionStarted(_)) => {
                self.started = true;
                return Some(Ok(()));
            }
            SessionEvent::Nori(NoriEvent::SessionEnded(ended)) => {
                return Some(Err(ended.message.clone().unwrap_or_else(|| {
                    "the session ended before it started".to_owned()
                })));
            }
            _ => {}
        }
        if self.skip_replays && self.replaying {
            return None;
        }
        if let Some(update) = session_update(&self.session, &envelope.event, Some(envelope.seq)) {
            self.connection.deliver(update);
        }
        None
    }

    /// Forward until the session has started, so the response follows the
    /// session's startup updates.
    async fn until_started(
        &mut self,
        events: &mut mpsc::Receiver<SessionEnvelope>,
    ) -> Result<(), AcpError> {
        while !self.started {
            let Some(envelope) = events.recv().await else {
                return Err(AcpError::internal("the session stopped reporting events"));
            };
            match self.deliver(&envelope) {
                Some(Ok(())) => return Ok(()),
                Some(Err(message)) => return Err(AcpError::internal(message)),
                None => {}
            }
        }
        Ok(())
    }

    async fn run(mut self, mut events: mpsc::Receiver<SessionEnvelope>) {
        while let Some(envelope) = events.recv().await {
            self.deliver(&envelope);
        }
        // The manager drops a subscriber that falls too far behind. The
        // client would silently miss updates, so close it; it reconnects
        // and loads again.
        self.connection.close.cancel();
    }
}

struct LoadParams {
    id: ConversationId,
    cwd: std::path::PathBuf,
}

fn parse(params: &Value) -> Result<LoadParams, AcpError> {
    let id = params["sessionId"]
        .as_str()
        .ok_or_else(|| AcpError::invalid_params("sessionId is required"))?;
    let id = ConversationId::from_string(id).map_err(|_| AcpError::not_found("unknown session"))?;
    let cwd = params["cwd"]
        .as_str()
        .map(Path::new)
        .filter(|cwd| cwd.is_absolute())
        .ok_or_else(|| AcpError::invalid_params("cwd must be an absolute path"))?;
    Ok(LoadParams {
        id,
        cwd: cwd.to_path_buf(),
    })
}

fn same_folder(requested: &Path, session: &Path) -> Result<(), AcpError> {
    let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if canonical(requested) == canonical(session) {
        Ok(())
    } else {
        Err(AcpError::invalid_params(format!(
            "the session works in {}, not {}",
            session.display(),
            requested.display()
        )))
    }
}

impl HostService {
    /// Attach `client` to a session. With `replay`, its history arrives as
    /// `session/update` notifications before the response (`session/load`);
    /// without, only what happens next does (`session/resume`). A stored or
    /// failed session is restarted on a new agent process first.
    pub(super) async fn load_session(
        &self,
        client: i64,
        params: &Value,
        replay: bool,
    ) -> Result<Value, AcpError> {
        let LoadParams { id, cwd } = parse(params)?;
        let connection = self.acp.connection(client)?;
        let session = id.to_string();
        let mut forwarder = Forwarder {
            connection: connection.clone(),
            session: session.clone(),
            skip_replays: true,
            replaying: false,
            started: false,
        };
        let events = match self.manager.get(&id) {
            Some(record) if record.status != SessionStatus::Failed => {
                same_folder(&cwd, &record.cwd)?;
                forwarder.skip_replays = false;
                let latest = self
                    .manager
                    .replay_info(&id)
                    .map_or(0, |info| info.latest_seq);
                let from = if replay { 0 } else { latest };
                let subscription = match self.manager.subscribe_from(
                    Some(id),
                    &HashMap::from([(session.clone(), from)]),
                    ATTACHED_QUEUE,
                ) {
                    Ok(subscription) => subscription,
                    // The journal no longer holds the start of this
                    // session; its transcript does.
                    Err(_) if replay => {
                        if let Some(path) = &record.transcript_path {
                            let transcript =
                                nori_harness::transcript::load_transcript_from_path(path)
                                    .await
                                    .map_err(AcpError::internal)?;
                            self.replay_transcript(&connection, &session, &transcript);
                        }
                        self.manager
                            .subscribe_from(
                                Some(id),
                                &HashMap::from([(session.clone(), latest)]),
                                ATTACHED_QUEUE,
                            )
                            .map_err(AcpError::internal)?
                    }
                    Err(error) => return Err(AcpError::internal(error)),
                };
                for envelope in &subscription.replay {
                    forwarder.deliver(envelope);
                }
                subscription.events
            }
            Some(record) => {
                same_folder(&cwd, &record.cwd)?;
                if replay {
                    let path = record
                        .transcript_path
                        .clone()
                        .ok_or_else(|| AcpError::internal("the session has no transcript"))?;
                    let transcript = nori_harness::transcript::load_transcript_from_path(&path)
                        .await
                        .map_err(AcpError::internal)?;
                    self.replay_transcript(&connection, &session, &transcript);
                }
                let latest = self
                    .manager
                    .replay_info(&id)
                    .map_or(0, |info| info.latest_seq);
                self.resume_failed(&id).await?;
                let mut subscription = self
                    .manager
                    .subscribe_from(
                        Some(id),
                        &HashMap::from([(session.clone(), latest)]),
                        ATTACHED_QUEUE,
                    )
                    .map_err(AcpError::internal)?;
                for envelope in std::mem::take(&mut subscription.replay) {
                    if let Some(Err(message)) = forwarder.deliver(&envelope) {
                        return Err(AcpError::internal(message));
                    }
                }
                forwarder.until_started(&mut subscription.events).await?;
                subscription.events
            }
            None => {
                let transcript = transcripts::load_stored(&self.config.nori_home, &id)
                    .await?
                    .ok_or_else(|| AcpError::not_found("unknown session"))?;
                same_folder(&cwd, &transcript.meta.cwd)?;
                if replay {
                    self.replay_transcript(&connection, &session, &transcript);
                }
                self.resume_stored(&id, transcript).await?;
                let mut subscription = self
                    .manager
                    .subscribe_from(
                        Some(id),
                        &HashMap::from([(session.clone(), 0)]),
                        ATTACHED_QUEUE,
                    )
                    .map_err(AcpError::internal)?;
                for envelope in std::mem::take(&mut subscription.replay) {
                    if let Some(Err(message)) = forwarder.deliver(&envelope) {
                        return Err(AcpError::internal(message));
                    }
                }
                forwarder.until_started(&mut subscription.events).await?;
                subscription.events
            }
        };
        self.acp
            .attach(client, id, tokio::spawn(forwarder.run(events)));
        Ok(json!({}))
    }

    /// Send a transcript's history to one connection.
    fn replay_transcript(
        &self,
        connection: &HostConnection,
        session: &str,
        transcript: &nori_harness::transcript::Transcript,
    ) {
        for event in nori_harness::backend::transcript_to_replay_session_events(transcript) {
            if let Some(update) = session_update(session, &event, None) {
                connection.deliver(update);
            }
        }
    }
}
