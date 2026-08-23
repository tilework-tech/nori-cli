//! [`HostedAgent`] implemented over [`HarnessHandle`]: the harness side of
//! the remote ACP transport (`docs/specs/remote-acp-transport.md` §3).
//!
//! [`HarnessRemoteHost`] follows the launched harness session through the
//! subscribable event stream, issues the stable Nori conversation id as the
//! outward ACP session id, forwards the post-harness ACP stream to the single
//! remote consumer, and routes remote mutations through the harness handle so
//! hooks, transcripts, goals, and permission policy all still apply.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;

use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use nori_protocol::SessionPhase;
use nori_protocol::acp::v1 as acp;
use tokio::sync::Mutex;
use tokio::sync::mpsc;

pub use nori_acp_host::remote::HostedAgent;
pub use nori_acp_host::remote::HostedSubscription;
pub use nori_acp_host::remote::LoadedSession;
pub use nori_acp_host::remote::RemoteAcpServer;
pub use nori_acp_host::remote::parse_bind_addr;

use crate::runtime::HarnessHandle;
use crate::transcript::TranscriptLoader;

/// Bounded queue between the host event loop and the remote transport's
/// forward loop. Overflow drops the consumer, which closes its connection.
const REMOTE_SINK_EVENTS: usize = 256;

/// The active harness session as seen by the remote surface.
struct ActiveSession {
    handle: HarnessHandle,
    nori_home: PathBuf,
    /// Stable outward session id (Nori conversation id). `None` until
    /// `SessionStarted` arrives.
    conversation_id: Option<String>,
    cwd: Option<PathBuf>,
}

#[derive(Default)]
struct HostShared {
    session: Option<ActiveSession>,
    /// The single remote consumer: subscription id and its bounded sink.
    sink: Option<(i64, mpsc::Sender<SessionEvent>)>,
    subscription_seq: i64,
    /// Harness request ids of remote-submitted prompts awaiting completion.
    remote_turns: Vec<acp::RequestId>,
    /// Delegated requests forwarded to the remote controller, unanswered.
    forwarded_requests: Vec<acp::RequestId>,
    /// The request id owning the current turn, from `SessionPhaseChanged`.
    current_turn: Option<acp::RequestId>,
    event_task: Option<tokio::task::JoinHandle<()>>,
}

/// Harness-side host for the remote ACP transport. Create one per process,
/// attach it to each launched harness session, and hand it to
/// [`RemoteAcpServer::bind`].
#[derive(Default)]
pub struct HarnessRemoteHost {
    state: Arc<Mutex<HostShared>>,
}

static ACTIVE_HOST: OnceLock<Arc<HarnessRemoteHost>> = OnceLock::new();

/// Install the process-wide remote host (set once, at remote-mode startup).
pub fn set_active_host(host: Arc<HarnessRemoteHost>) {
    if ACTIVE_HOST.set(host).is_err() {
        tracing::warn!("remote host was already installed; keeping the first one");
    }
}

/// The process-wide remote host, when remote mode is enabled.
pub fn active_host() -> Option<Arc<HarnessRemoteHost>> {
    ACTIVE_HOST.get().cloned()
}

impl HarnessRemoteHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Follow a newly launched harness session. Call immediately after
    /// `launch_session`, before awaiting anything else, so the subscription
    /// registers ahead of the session's startup events.
    pub async fn attach(&self, handle: HarnessHandle, nori_home: PathBuf) -> anyhow::Result<()> {
        let events = handle.subscribe_events().await?;
        let mut state = self.state.lock().await;
        if let Some(task) = state.event_task.take() {
            task.abort();
        }
        state.session = Some(ActiveSession {
            handle,
            nori_home,
            conversation_id: None,
            cwd: None,
        });
        state.remote_turns.clear();
        state.forwarded_requests.clear();
        state.current_turn = None;
        state.event_task = Some(tokio::spawn(run_event_loop(self.state.clone(), events)));
        Ok(())
    }

    /// Look up the attached session, verifying the outward session id.
    async fn checked_handle(
        &self,
        session_id: &acp::SessionId,
    ) -> Result<HarnessHandle, acp::Error> {
        let state = self.state.lock().await;
        let Some(session) = state.session.as_ref() else {
            return Err(unknown_session());
        };
        if session.conversation_id.as_deref() != Some(session_id.0.as_ref()) {
            return Err(unknown_session());
        }
        Ok(session.handle.clone())
    }
}

fn unknown_session() -> acp::Error {
    acp::Error::new(
        -32002,
        "unknown session id; discover sessions with session/list",
    )
}

fn internal_error(message: impl std::fmt::Display) -> acp::Error {
    acp::Error::new(-32000, message.to_string())
}

/// Answer delegated permission requests the detached controller can no longer
/// see, so they cannot wedge the agent.
async fn cancel_forwarded(handle: &HarnessHandle, request_ids: Vec<acp::RequestId>) {
    for request_id in request_ids {
        let response = acp::ClientResponse::RequestPermissionResponse(
            acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Cancelled),
        );
        if let Err(error) = handle.respond_to_agent(request_id, Ok(response)).await {
            tracing::debug!("failed to cancel delegated request after remote detach: {error}");
        }
    }
}

/// Follow the harness event stream: track session identity, rewrite outward
/// session ids, decide what the remote consumer sees, and keep remote-owned
/// turn state.
async fn run_event_loop(state: Arc<Mutex<HostShared>>, mut events: mpsc::Receiver<SessionEvent>) {
    while let Some(event) = events.recv().await {
        let mut shared = state.lock().await;
        match event {
            SessionEvent::Nori(NoriEvent::SessionStarted(started)) => {
                if let Some(session) = shared.session.as_mut() {
                    session.conversation_id = Some(
                        started
                            .transcript_id
                            .clone()
                            .unwrap_or_else(|| started.acp_session_id.to_string()),
                    );
                    session.cwd = Some(started.cwd.clone());
                }
            }
            SessionEvent::Nori(NoriEvent::SessionForked(forked)) => {
                if let Some(session) = shared.session.as_mut() {
                    session.conversation_id = Some(forked.new_conversation_id.clone());
                }
            }
            SessionEvent::Nori(NoriEvent::SessionPhaseChanged(phase)) => {
                shared.current_turn = match phase {
                    SessionPhase::Idle => None,
                    SessionPhase::Loading { request_id }
                    | SessionPhase::Prompting { request_id }
                    | SessionPhase::Cancelling { request_id } => Some(request_id),
                };
            }
            SessionEvent::Acp(AcpEvent::Notification(notification)) => {
                if let acp::AgentNotification::SessionNotification(mut notification) = notification
                {
                    let outward = shared
                        .session
                        .as_ref()
                        .and_then(|session| session.conversation_id.clone());
                    if let Some(outward) = outward {
                        notification.session_id = acp::SessionId::new(outward);
                        forward_to_sink(
                            &mut shared,
                            SessionEvent::Acp(AcpEvent::Notification(
                                acp::AgentNotification::SessionNotification(notification),
                            )),
                        );
                    }
                }
            }
            SessionEvent::Acp(AcpEvent::Request {
                request_id,
                request,
            }) => {
                let remote_owns_turn = shared
                    .current_turn
                    .as_ref()
                    .is_some_and(|turn| shared.remote_turns.contains(turn));
                if remote_owns_turn && shared.sink.is_some() {
                    shared.forwarded_requests.push(request_id.clone());
                    forward_to_sink(
                        &mut shared,
                        SessionEvent::Acp(AcpEvent::Request {
                            request_id,
                            request,
                        }),
                    );
                }
            }
            SessionEvent::Acp(AcpEvent::Response {
                request_id,
                response,
            }) => {
                let position = shared.remote_turns.iter().position(|id| id == &request_id);
                if let Some(position) = position {
                    shared.remote_turns.remove(position);
                    forward_to_sink(
                        &mut shared,
                        SessionEvent::Acp(AcpEvent::Response {
                            request_id,
                            response,
                        }),
                    );
                }
            }
            SessionEvent::Nori(NoriEvent::RequestFailed(failure)) => {
                let remote_owned = failure.request_id.as_ref().is_some_and(|request_id| {
                    shared.remote_turns.iter().any(|id| id == request_id)
                });
                if remote_owned {
                    shared
                        .remote_turns
                        .retain(|id| Some(id) != failure.request_id.as_ref());
                    forward_to_sink(
                        &mut shared,
                        SessionEvent::Nori(NoriEvent::RequestFailed(failure)),
                    );
                }
            }
            SessionEvent::Nori(NoriEvent::SessionEnded(ended)) => {
                forward_to_sink(
                    &mut shared,
                    SessionEvent::Nori(NoriEvent::SessionEnded(ended)),
                );
                shared.session = None;
                shared.remote_turns.clear();
                shared.forwarded_requests.clear();
                shared.current_turn = None;
                break;
            }
            SessionEvent::Nori(_) => {}
        }
    }
}

/// Push an outward-facing event to the remote consumer. A full queue drops
/// the consumer (its receiver closes, which closes the connection).
fn forward_to_sink(shared: &mut HostShared, event: SessionEvent) {
    let Some((_, sink)) = shared.sink.as_ref() else {
        return;
    };
    match sink.try_send(event) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            tracing::warn!("remote ACP consumer fell behind; dropping it");
            shared.sink = None;
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            shared.sink = None;
        }
    }
}

impl HostedAgent for HarnessRemoteHost {
    async fn list_sessions(&self) -> Result<Vec<acp::SessionInfo>, acp::Error> {
        let state = self.state.lock().await;
        let sessions = state
            .session
            .as_ref()
            .and_then(|session| {
                let conversation_id = session.conversation_id.clone()?;
                let cwd = session.cwd.clone().unwrap_or_default();
                Some(vec![acp::SessionInfo::new(conversation_id, cwd)])
            })
            .unwrap_or_default();
        Ok(sessions)
    }

    async fn load_session(&self, session_id: &acp::SessionId) -> Result<LoadedSession, acp::Error> {
        let (handle, nori_home, conversation_id) = {
            let state = self.state.lock().await;
            let Some(session) = state.session.as_ref() else {
                return Err(unknown_session());
            };
            let Some(conversation_id) = session.conversation_id.clone() else {
                return Err(unknown_session());
            };
            if conversation_id != session_id.0.as_ref() {
                return Err(unknown_session());
            }
            (
                session.handle.clone(),
                session.nori_home.clone(),
                conversation_id,
            )
        };

        // Write barrier: everything recorded before this call is on disk.
        handle.flush_transcript().await.map_err(internal_error)?;

        let loader = TranscriptLoader::new(nori_home);
        let metadata = loader
            .find_session_metadata_by_id(&conversation_id)
            .await
            .map_err(internal_error)?
            .ok_or_else(|| internal_error("session transcript not found"))?;
        let transcript = loader
            .load_transcript(&metadata.project_id, &conversation_id)
            .await
            .map_err(internal_error)?;

        let replay = crate::backend::transcript_to_replay_session_events(&transcript)
            .into_iter()
            .filter_map(|event| match event {
                SessionEvent::Acp(AcpEvent::Notification(
                    acp::AgentNotification::SessionNotification(mut notification),
                )) => {
                    notification.session_id = session_id.clone();
                    Some(notification)
                }
                _ => None,
            })
            .collect();
        Ok(LoadedSession { replay })
    }

    async fn resume_session(&self, session_id: &acp::SessionId) -> Result<(), acp::Error> {
        self.checked_handle(session_id).await.map(|_| ())
    }

    async fn prompt(
        &self,
        session_id: &acp::SessionId,
        prompt: Vec<acp::ContentBlock>,
    ) -> Result<acp::RequestId, acp::Error> {
        // Hold the state lock across submit-and-register so the event loop
        // cannot see this turn's response before it is marked remote-owned.
        let mut state = self.state.lock().await;
        let Some(session) = state.session.as_ref() else {
            return Err(unknown_session());
        };
        if session.conversation_id.as_deref() != Some(session_id.0.as_ref()) {
            return Err(unknown_session());
        }
        let handle = session.handle.clone();
        let request_id = handle.prompt(prompt).await.map_err(internal_error)?;
        state.remote_turns.push(request_id.clone());
        Ok(request_id)
    }

    async fn cancel(&self, session_id: &acp::SessionId) -> Result<(), acp::Error> {
        let handle = self.checked_handle(session_id).await?;
        handle.cancel().await.map_err(internal_error)
    }

    async fn close_session(&self, session_id: &acp::SessionId) -> Result<(), acp::Error> {
        let handle = self.checked_handle(session_id).await?;
        handle.close_session().await.map_err(internal_error)
    }

    async fn respond(
        &self,
        request_id: acp::RequestId,
        response: Result<acp::ClientResponse, acp::Error>,
    ) -> Result<(), acp::Error> {
        let handle = {
            let mut state = self.state.lock().await;
            let position = state
                .forwarded_requests
                .iter()
                .position(|id| id == &request_id)
                .ok_or_else(|| {
                    acp::Error::new(-32600, "no delegated request with this id is pending")
                })?;
            state.forwarded_requests.remove(position);
            let Some(session) = state.session.as_ref() else {
                return Err(unknown_session());
            };
            session.handle.clone()
        };
        handle
            .respond_to_agent(request_id, response)
            .await
            .map_err(internal_error)
    }

    async fn subscribe(&self) -> HostedSubscription {
        let (sink, events) = mpsc::channel(REMOTE_SINK_EVENTS);
        let (subscription_id, replaced) = {
            let mut state = self.state.lock().await;
            state.subscription_seq += 1;
            let subscription_id = state.subscription_seq;
            state.sink = Some((subscription_id, sink));
            let replaced = if state.forwarded_requests.is_empty() {
                None
            } else {
                let request_ids = std::mem::take(&mut state.forwarded_requests);
                state
                    .session
                    .as_ref()
                    .map(|session| (session.handle.clone(), request_ids))
            };
            (subscription_id, replaced)
        };
        if let Some((handle, request_ids)) = replaced {
            cancel_forwarded(&handle, request_ids).await;
        }
        HostedSubscription {
            id: subscription_id,
            events,
        }
    }

    async fn detach(&self, subscription_id: i64) {
        let cancelled = {
            let mut state = self.state.lock().await;
            let is_current = state
                .sink
                .as_ref()
                .is_some_and(|(current, _)| *current == subscription_id);
            if !is_current {
                return;
            }
            state.sink = None;
            let request_ids = std::mem::take(&mut state.forwarded_requests);
            state
                .session
                .as_ref()
                .map(|session| (session.handle.clone(), request_ids))
        };
        if let Some((handle, request_ids)) = cancelled {
            cancel_forwarded(&handle, request_ids).await;
        }
    }
}
