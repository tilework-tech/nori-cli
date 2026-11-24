//! Attaching connections to sessions with `session/load` and
//! `session/resume`, and the forwarder that relays a session's journal to
//! one connection.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;

use nori_harness::ConversationId;
use nori_harness::remote_agent::HostConnection;
use nori_harness::session_manager::SessionManager;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEnvelope;
use nori_protocol::SessionEvent;
use nori_protocol::SessionStatus;
use nori_protocol::acp::v1 as acp;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;
use tokio::sync::watch;

use super::AcpError;
use super::AcpFace;
use crate::host::service::HostService;
use crate::host::transcripts;

/// Envelopes an attached connection may lag behind its session before the
/// manager drops it.
pub(super) const ATTACHED_QUEUE: i32 = 1024;

/// The outward `session/update` for a session event, if it is one.
pub(super) fn session_update(session: &str, event: &SessionEvent) -> Option<Value> {
    let SessionEvent::Acp(AcpEvent::Notification(acp::AgentNotification::SessionNotification(
        notification,
    ))) = event
    else {
        return None;
    };
    let mut params = serde_json::to_value(notification).ok()?;
    params["sessionId"] = json!(session);
    Some(json!({"jsonrpc":"2.0","method":"session/update","params":params}))
}

/// Forwards session envelopes to one connection: updates to everyone,
/// permission requests to the controller.
pub(super) struct Forwarder {
    face: AcpFace,
    manager: SessionManager,
    connection: HostConnection,
    /// Drop history the harness replays on resume: the client either asked
    /// for none or already received it from the transcript.
    skip_replays: bool,
    replaying: HashSet<String>,
    started: bool,
    /// The agent's own error while the session was starting.
    startup_error: Option<AcpError>,
    delivered: watch::Sender<i64>,
    /// Permission requests waiting on a client, by session.
    open: HashMap<String, Vec<(acp::RequestId, acp::RequestPermissionRequest)>>,
    asked: HashMap<(String, acp::RequestId), String>,
    /// False while history is being sent: requests in history may already
    /// be answered, and the ones still open are asked once attached.
    asking: bool,
}

impl Forwarder {
    pub(super) fn new(
        face: &AcpFace,
        manager: &SessionManager,
        connection: HostConnection,
    ) -> Self {
        Self {
            face: face.clone(),
            manager: manager.clone(),
            connection,
            skip_replays: true,
            replaying: HashSet::new(),
            started: false,
            startup_error: None,
            delivered: watch::channel(0).0,
            open: HashMap::new(),
            asked: HashMap::new(),
            asking: false,
        }
    }

    pub(super) fn delivered(&self) -> watch::Receiver<i64> {
        self.delivered.subscribe()
    }

    /// Deliver one envelope. Returns `Some(result)` when the session
    /// finished starting (`Ok`) or ended before it did (`Err`).
    fn deliver(&mut self, envelope: &SessionEnvelope) -> Option<Result<(), AcpError>> {
        let session = envelope.conversation_id.clone();
        let outcome = self.relay(&session, envelope);
        self.delivered.send_replace(envelope.seq);
        outcome
    }

    fn relay(&mut self, session: &str, envelope: &SessionEnvelope) -> Option<Result<(), AcpError>> {
        match &envelope.event {
            SessionEvent::Nori(NoriEvent::ReplayStarted(_)) => {
                self.replaying.insert(session.to_owned());
            }
            SessionEvent::Nori(NoriEvent::ReplayFinished) => {
                self.replaying.remove(session);
            }
            SessionEvent::Nori(NoriEvent::SessionStarted(_)) => {
                self.started = true;
                return Some(Ok(()));
            }
            SessionEvent::Nori(NoriEvent::SessionEnded(ended)) => {
                self.open.remove(session);
                if self.started {
                    return None;
                }
                // Keep the agent's own error code, with the harness's
                // explanation of it (the sign-in or install hint) when there
                // is one.
                let message = ended.message.clone();
                return Some(Err(match self.startup_error.take() {
                    Some(error) => AcpError {
                        message: message.unwrap_or(error.message),
                        ..error
                    },
                    None => AcpError::internal(
                        message.unwrap_or_else(|| "the session ended before it started".to_owned()),
                    ),
                }));
            }
            SessionEvent::Nori(NoriEvent::RequestResolved(resolved)) => {
                self.forget(session, &resolved.request_id);
            }
            SessionEvent::Acp(AcpEvent::Response { response, .. }) => {
                if !self.started
                    && let Err(error) = response
                {
                    self.startup_error = Some(AcpError {
                        code: i32::from(error.code),
                        message: error.message.clone(),
                    });
                }
            }
            SessionEvent::Acp(AcpEvent::Request {
                request_id,
                request: acp::AgentRequest::RequestPermissionRequest(request),
            }) => {
                let open = self.open.entry(session.to_owned()).or_default();
                if !open.iter().any(|(id, _)| id == request_id) {
                    open.push((request_id.clone(), request.clone()));
                }
                self.ask();
            }
            _ => {}
        }
        if self.skip_replays && self.replaying.contains(session) {
            return None;
        }
        if super::config::is_adapter_mode_update(&envelope.event) {
            return None;
        }
        if let Some(adapter) = super::config::adapter_options_update(&envelope.event) {
            // Clients see the complete option list, host options included.
            if let Ok(id) = ConversationId::from_string(session)
                && let Some(record) = self.manager.get(&id)
            {
                let mode = nori_harness::permission_mode::PermissionMode::from_approval_policy(
                    record.approval_policy,
                );
                let options = super::config::compose(&record.agent, mode, &adapter);
                self.connection.deliver(super::config::update(
                    session,
                    json!({"sessionUpdate":"config_option_update","configOptions":options}),
                ));
            }
            return None;
        }
        if let Some(update) = session_update(session, &envelope.event) {
            self.connection.deliver(update);
        }
        None
    }

    fn forget(&mut self, session: &str, request_id: &acp::RequestId) {
        if let Some(open) = self.open.get_mut(session) {
            open.retain(|(id, _)| id != request_id);
        }
        if let Some(id) = self.asked.remove(&(session.to_owned(), request_id.clone())) {
            self.face.take_ask(self.connection.client, &id);
        }
    }

    /// Send the controller every open permission request it has not been
    /// asked on this connection.
    fn ask(&mut self) {
        if !self.asking || !self.connection.is_controller() {
            return;
        }
        for (session, requests) in &self.open {
            let Ok(conversation) = ConversationId::from_string(session) else {
                continue;
            };
            for (request_id, request) in requests {
                let key = (session.clone(), request_id.clone());
                if self.asked.contains_key(&key) {
                    continue;
                }
                let Ok(mut params) = serde_json::to_value(request) else {
                    continue;
                };
                params["sessionId"] = json!(session);
                let id = self.face.register_ask(
                    self.connection.client,
                    conversation,
                    request_id.clone(),
                );
                self.asked.insert(key, id.clone());
                self.connection.deliver(json!({
                    "jsonrpc":"2.0",
                    "id":id,
                    "method":"session/request_permission",
                    "params":params,
                }));
            }
        }
    }

    /// Learn the permission requests a session is already waiting on, so a
    /// client attaching after they were sent is still asked.
    fn seed_open_requests(&mut self, id: &ConversationId) {
        self.asking = true;
        let Some(record) = self.manager.get(id) else {
            return;
        };
        // History may hold requests answered since; keep only those the
        // session still waits on.
        if let Some(open) = self.open.get_mut(&id.to_string()) {
            open.retain(|(request_id, _)| record.pending_requests.contains(request_id));
        }
        if record.pending_requests.is_empty() {
            self.ask();
            return;
        }
        let cursors = HashMap::from([(id.to_string(), 0)]);
        let Ok(history) = self.manager.subscribe_from(Some(*id), &cursors, 1) else {
            return;
        };
        let session = id.to_string();
        for envelope in history.replay {
            if let SessionEvent::Acp(AcpEvent::Request {
                request_id,
                request: acp::AgentRequest::RequestPermissionRequest(request),
            }) = envelope.event
                && record.pending_requests.contains(&request_id)
            {
                let open = self.open.entry(session.clone()).or_default();
                if !open.iter().any(|(open_id, _)| *open_id == request_id) {
                    open.push((request_id, request));
                }
            }
        }
        self.ask();
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
            if let Some(Err(error)) = self.deliver(&envelope) {
                return Err(error);
            }
        }
        Ok(())
    }

    pub(super) async fn run(mut self, mut events: mpsc::Receiver<SessionEnvelope>) {
        let mut role = self.connection.role.clone();
        loop {
            tokio::select! {
                envelope = events.recv() => {
                    let Some(envelope) = envelope else { break };
                    self.deliver(&envelope);
                }
                changed = role.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    if self.connection.is_controller() {
                        // A client that took control is asked what is still open.
                        self.ask();
                    } else {
                        // Ask again if this client takes control back.
                        for (_, id) in self.asked.drain() {
                            self.face.take_ask(self.connection.client, &id);
                        }
                    }
                }
            }
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

/// Whether a live record no longer runs and must restart to be used.
pub(super) fn needs_restart(status: SessionStatus) -> bool {
    matches!(
        status,
        SessionStatus::Failed | SessionStatus::Closed | SessionStatus::Done
    )
}

impl HostService {
    /// Attach `client` to a session. With `replay`, its history arrives as
    /// `session/update` notifications before the response (`session/load`);
    /// without, only what happens next does (`session/resume`). A stored,
    /// failed or closed session restarts on a new agent process first.
    pub(super) async fn load_session(
        &self,
        client: i64,
        params: &Value,
        replay: bool,
    ) -> Result<Value, AcpError> {
        let LoadParams { id, cwd } = parse(params)?;
        let restarting = self
            .manager
            .get(&id)
            .is_none_or(|record| needs_restart(record.status));
        // Hold the lock through the restart; the state is read again below.
        let _restart = if restarting {
            Some(self.restarts.lock().await)
        } else {
            None
        };
        if self.is_deleted(&id) {
            return Err(AcpError::not_found("unknown session"));
        }
        let connection = self.acp.connection(client)?;
        let session = id.to_string();
        let mut forwarder = Forwarder::new(&self.acp, &self.manager, connection.clone());
        let events = match self.manager.get(&id) {
            Some(record) if !needs_restart(record.status) => {
                same_folder(&cwd, &record.cwd)?;
                forwarder.skip_replays = false;
                let latest = self
                    .manager
                    .replay_info(&id)
                    .map_or(0, |info| info.latest_seq);
                let from = if replay { 0 } else { latest };
                let mut subscription = match self.manager.subscribe_from(
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
                            replay_transcript(&connection, &session, &transcript).await?;
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
                for envelope in std::mem::take(&mut subscription.replay) {
                    forwarder.deliver(&envelope);
                }
                let starting = self
                    .manager
                    .get(&id)
                    .is_some_and(|record| record.status == SessionStatus::Starting);
                if starting {
                    forwarder.until_started(&mut subscription.events).await?;
                }
                subscription.events
            }
            Some(record) => {
                same_folder(&cwd, &record.cwd)?;
                if replay && let Some(path) = &record.transcript_path {
                    let transcript = nori_harness::transcript::load_transcript_from_path(path)
                        .await
                        .map_err(AcpError::internal)?;
                    replay_transcript(&connection, &session, &transcript).await?;
                }
                let latest = self
                    .manager
                    .replay_info(&id)
                    .map_or(0, |info| info.latest_seq);
                if record.status != SessionStatus::Failed {
                    self.manager.reopen(&id);
                }
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
                    if let Some(Err(error)) = forwarder.deliver(&envelope) {
                        return Err(error);
                    }
                }
                forwarder.until_started(&mut subscription.events).await?;
                self.reapply_options(&id).await;
                subscription.events
            }
            None => {
                let transcript = transcripts::load_stored(&self.config.nori_home, &id)
                    .await?
                    .ok_or_else(|| AcpError::not_found("unknown session"))?;
                same_folder(&cwd, &transcript.meta.cwd)?;
                if replay {
                    replay_transcript(&connection, &session, &transcript).await?;
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
                    if let Some(Err(error)) = forwarder.deliver(&envelope) {
                        return Err(error);
                    }
                }
                forwarder.until_started(&mut subscription.events).await?;
                self.reapply_options(&id).await;
                subscription.events
            }
        };
        self.settle_adapter_mode(&id).await?;
        forwarder.seed_open_requests(&id);
        let delivered = forwarder.delivered();
        self.acp
            .attach(client, id, tokio::spawn(forwarder.run(events)), delivered);
        let (options, modes) = self.session_configuration(&id).await?;
        Ok(json!({"configOptions": options, "modes": modes}))
    }

    /// Attach `client` to a session that just started, from its first event,
    /// and wait until it has started.
    pub(super) async fn attach_new(
        &self,
        client: i64,
        id: &ConversationId,
    ) -> Result<(), AcpError> {
        let connection = self.acp.connection(client)?;
        let mut forwarder = Forwarder::new(&self.acp, &self.manager, connection);
        forwarder.skip_replays = false;
        let mut subscription = self
            .manager
            .subscribe_from(
                Some(*id),
                &HashMap::from([(id.to_string(), 0)]),
                ATTACHED_QUEUE,
            )
            .map_err(AcpError::internal)?;
        for envelope in std::mem::take(&mut subscription.replay) {
            if let Some(Err(error)) = forwarder.deliver(&envelope) {
                return Err(error);
            }
        }
        forwarder.until_started(&mut subscription.events).await?;
        forwarder.seed_open_requests(id);
        let delivered = forwarder.delivered();
        self.acp.attach(
            client,
            *id,
            tokio::spawn(forwarder.run(subscription.events)),
            delivered,
        );
        Ok(())
    }
}

/// Send a transcript's history to one connection, waiting for room rather
/// than treating a long history as a client that fell behind.
async fn replay_transcript(
    connection: &HostConnection,
    session: &str,
    transcript: &nori_harness::transcript::Transcript,
) -> Result<(), AcpError> {
    for event in nori_harness::backend::transcript_to_replay_session_events(transcript) {
        if let Some(update) = session_update(session, &event) {
            connection
                .outbound
                .send(update)
                .await
                .map_err(|_| AcpError::internal("the connection closed"))?;
        }
    }
    Ok(())
}
