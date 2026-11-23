//! Session manager: one registry of live sessions per Nori process.

mod recovery;
pub use recovery::SessionCheckpoint;
mod delegation;
mod delegation_driver;
mod delegation_state;
pub use delegation::DelegationContext;
mod registry;
mod replay;

pub use replay::ReplayError;
pub use replay::ReplayInfo;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;
use std::time::Duration;

use nori_config::AskForApproval;
use nori_protocol::NoriEvent;
use nori_protocol::RequestResolved;
use nori_protocol::SessionEnvelope;
use nori_protocol::SessionEvent;
use nori_protocol::SessionStatus;
use tokio::sync::Notify;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use self::registry::AgentProcessId;
use self::registry::Registry;
use self::registry::SessionSettings;
use crate::AcpBackendConfig;
use crate::ConversationId;
use crate::runtime::AgentPrepareSpec;
use crate::runtime::HarnessHandle;
use crate::runtime::PreparedAgent;
use crate::runtime::SessionAgentSource;
use crate::runtime::SessionResume;
use crate::runtime::SessionStart;
use crate::runtime::build_backend_config;
use crate::runtime::launch_session_from;
use crate::transcript::Transcript;
use crate::transcript::load_transcript_from_path;

/// How a session came to exist. A user-started session, a fork and a session
/// started by another session are otherwise the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionSource {
    /// Started by a user through a client.
    User,
    /// Created by forking its parent.
    Fork,
    /// Started by its parent session.
    Session,
}

/// A snapshot of one session's settings and status.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRecord {
    pub conversation_id: ConversationId,
    pub agent: String,
    pub parent: Option<ConversationId>,
    pub source: SessionSource,
    pub cwd: PathBuf,
    pub model: Option<String>,
    /// The display name of the model the agent says it runs.
    pub model_name: Option<String>,
    pub approval_policy: AskForApproval,
    pub transcript_path: Option<PathBuf>,
    pub acp_session_id: Option<String>,
    /// Agent requests that wait on a client, in the order they arrived.
    pub pending_requests: Vec<nori_protocol::acp::v1::RequestId>,
    pub status: SessionStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StartError {
    #[error("{limit} sessions are already running; close one before starting another")]
    CapacityReached { limit: i32 },
    #[error("the session manager is shutting down")]
    ShuttingDown,
    #[error("parent session {0} is not registered")]
    UnknownParent(ConversationId),
    #[error("parent session {0} is not running")]
    ParentEnded(ConversationId),
    #[error("the prepared agent already carries a session")]
    AgentAlreadyActivated,
    #[error("session {0} has not failed; only failed sessions can be resumed")]
    NotFailed(ConversationId),
}

/// Limits applied by a [`SessionManager`].
#[derive(Debug, Clone, Copy)]
pub struct SessionManagerConfig {
    /// Most sessions that may hold an agent process at once.
    pub max_sessions: i32,
    /// Longest wait for one session to shut down.
    pub shutdown_timeout: Duration,
}

impl Default for SessionManagerConfig {
    fn default() -> Self {
        Self {
            max_sessions: 8,
            shutdown_timeout: Duration::from_secs(5),
        }
    }
}

/// The agent process a new session runs on.
#[expect(
    clippy::large_enum_variant,
    reason = "SessionAgent is a one-shot start value, like SessionStart"
)]
pub enum SessionAgent {
    /// An agent that a client already prepared, for example behind a picker.
    Prepared(PreparedAgent),
    /// Start and prepare a new agent process.
    Prepare(AgentPrepareSpec),
}

/// Everything needed to start a managed session.
pub struct StartSession {
    pub agent: SessionAgent,
    pub start: SessionStart,
    pub parent: Option<ConversationId>,
    pub source: SessionSource,
}

/// A started session: its ID, its control handle and its own event stream.
pub struct ManagedSession {
    pub conversation_id: ConversationId,
    pub handle: HarnessHandle,
    /// Every event of this agent process, in order and never dropped. After a
    /// branch-at-head fork the envelopes carry the child's conversation ID.
    pub events: mpsc::UnboundedReceiver<SessionEnvelope>,
}

/// Outcome of [`SessionManager::shutdown`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ShutdownReport {
    pub closed: Vec<ConversationId>,
    pub timed_out: Vec<ConversationId>,
    pub failed: Vec<(ConversationId, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum ResumeError {
    #[error("session {0} is not registered")]
    Unknown(ConversationId),
    #[error("session {0} has not failed; only failed sessions can be resumed")]
    NotFailed(ConversationId),
    #[error("session {0} has no transcript to resume from")]
    NoTranscript(ConversationId),
    #[error("session {id} runs on agent {expected}, not {actual}")]
    AgentMismatch {
        id: ConversationId,
        expected: String,
        actual: String,
    },
    #[error("could not read the transcript of session {id}: {source}")]
    Transcript {
        id: ConversationId,
        source: std::io::Error,
    },
    #[error(transparent)]
    Start(#[from] StartError),
}

/// Registry of live sessions, keyed by conversation ID.
///
/// Each session runs on its own agent process. The manager starts sessions,
/// numbers and tags every event they emit, tracks their status, and shuts them
/// down. Clones share the same registry.
#[derive(Clone)]
pub struct SessionManager {
    shared: Arc<Shared>,
}

struct Shared {
    config: SessionManagerConfig,
    state: Mutex<State>,
    process_ended: Notify,
    admission: tokio::sync::Mutex<()>,
}

struct State {
    registry: Registry,
    handles: HashMap<AgentProcessId, HarnessHandle>,
    journal: replay::Journal,
    delegation: delegation::DelegationState,
    configs: HashMap<AgentProcessId, AcpBackendConfig>,
    primary: HashMap<ConversationId, mpsc::UnboundedSender<SessionEnvelope>>,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SessionManager {
    /// Retain at most 4096 envelopes across this manager's sessions.
    pub fn new(config: SessionManagerConfig) -> Self {
        Self::with_replay_capacity(config, 4096)
    }

    /// Configure the bounded in-process reconnect history, independently of live queue sizes.
    pub fn with_replay_capacity(config: SessionManagerConfig, capacity: i32) -> Self {
        Self {
            shared: Arc::new(Shared {
                config,
                process_ended: Notify::new(),
                admission: tokio::sync::Mutex::new(()),
                state: Mutex::new(State {
                    registry: Registry::new(config.max_sessions),
                    handles: HashMap::new(),
                    journal: replay::Journal::new(capacity),
                    delegation: delegation::DelegationState::default(),
                    configs: HashMap::new(),
                    primary: HashMap::new(),
                }),
            }),
        }
    }

    /// Replay strictly after each cursor and follow live events at one atomic boundary.
    /// `None` selects all sessions, including sessions subsequently started.
    pub fn subscribe_from(
        &self,
        session: Option<ConversationId>,
        cursors: &HashMap<String, i64>,
        capacity: i32,
    ) -> Result<nori_acp_host::remote::ManagedSubscription, ReplayError> {
        self.shared.state().journal.subscribe_from(
            session.map(|id| id.to_string()),
            cursors,
            capacity,
        )
    }

    /// Current cursors include ended conversations whose events remain replayable.
    pub fn replay_cursors(&self) -> HashMap<String, i64> {
        self.shared.state().journal.cursors()
    }

    pub fn replay_info(&self, session: &ConversationId) -> Option<ReplayInfo> {
        self.shared.state().journal.info(&session.to_string())
    }

    /// Start a session on its own agent process.
    ///
    /// A slot under the cap is reserved before the process starts and is
    /// released when the session ends.
    pub fn start(&self, spec: StartSession) -> Result<ManagedSession, StartError> {
        self.start_session(spec, None)
    }

    fn start_session(
        &self,
        spec: StartSession,
        resumed_from: Option<ConversationId>,
    ) -> Result<ManagedSession, StartError> {
        let StartSession {
            agent,
            start,
            parent,
            source,
        } = spec;
        let conversation_id = resumed_from.unwrap_or_default();
        let settings_from = |config: &AcpBackendConfig| SessionSettings {
            conversation_id,
            agent: config.agent.clone(),
            parent,
            source,
            resumed_from,
            cwd: config.cwd.clone(),
            model: config.default_model.clone(),
            approval_policy: config.approval_policy,
        };
        let (agent_source, settings, config) = match agent {
            SessionAgent::Prepared(mut agent) => {
                let config = agent
                    .activation_config_mut()
                    .ok_or(StartError::AgentAlreadyActivated)?;
                config.conversation_id = Some(conversation_id);
                let settings = settings_from(config);
                let config = config.clone();
                (
                    SessionAgentSource::Prepared(Box::new(agent)),
                    settings,
                    config,
                )
            }
            SessionAgent::Prepare(spec) => {
                let mut config = build_backend_config(spec);
                config.conversation_id = Some(conversation_id);
                let settings = settings_from(&config);
                (
                    SessionAgentSource::Prepare(Box::new(config.clone())),
                    settings,
                    config,
                )
            }
        };
        self.launch_registered(agent_source, settings, config, start)
    }

    fn launch_registered(
        &self,
        mut agent_source: SessionAgentSource,
        settings: SessionSettings,
        mut config: AcpBackendConfig,
        start: SessionStart,
    ) -> Result<ManagedSession, StartError> {
        let conversation_id = settings.conversation_id;
        let process = {
            let mut state = self.shared.state();
            if settings.source == SessionSource::Session
                && let Some(parent) = settings.parent
                && (state.registry.live_process(&parent).is_none()
                    || state.delegation.closed.contains(&parent))
            {
                return Err(StartError::ParentEnded(parent));
            }
            let reservation = state.registry.reserve()?;
            let process = state.registry.register(settings, reservation)?;
            state.journal.register(conversation_id.to_string());
            process
        };

        if config.multi_session {
            config.delegation_context = Some(DelegationContext::new(&self.shared, process));
        }
        match &mut agent_source {
            SessionAgentSource::Prepared(agent) => {
                if let Some(activation) = agent.activation_config_mut() {
                    activation.delegation_context = config.delegation_context.clone();
                }
            }
            SessionAgentSource::Prepare(activation) => **activation = config.clone(),
        }
        self.shared.state().configs.insert(process, config);
        let launched = launch_session_from(agent_source, start);
        // Answers are queued before they reach the agent, and the pump reads
        // this queue first, so an answer is numbered ahead of anything the
        // agent does in reaction to it.
        let (answers_tx, mut answers) = mpsc::unbounded_channel();
        let handle =
            launched
                .handle
                .with_agent_response_observer(Arc::new(move |request_id, responder| {
                    let _ = answers_tx.send(SessionEvent::Nori(NoriEvent::RequestResolved(
                        RequestResolved {
                            request_id: request_id.clone(),
                            answered_by: responder.to_string(),
                        },
                    )));
                }));
        let shutting_down = {
            let mut state = self.shared.state();
            state.handles.insert(process, handle.clone());
            state.registry.admission_closed()
        };
        if shutting_down {
            // `shutdown` began while this session was launching and could not
            // see it.
            let late = handle.clone();
            tokio::spawn(async move {
                if let Err(error) = late.shutdown().await {
                    tracing::warn!(%error, "failed to shut down a session started during shutdown");
                }
            });
        }

        let (primary_tx, primary_rx) = mpsc::unbounded_channel();
        self.shared
            .state()
            .primary
            .insert(conversation_id, primary_tx.clone());
        let shared = Arc::clone(&self.shared);
        let mut events = launched.events;
        tokio::spawn(async move {
            loop {
                let event = tokio::select! {
                    biased;
                    Some(answer) = answers.recv() => answer,
                    event = events.recv() => match event {
                        Some(event) => event,
                        None => break,
                    },
                };
                {
                    let mut state = shared.state();
                    let envelopes = state.registry.observe_all(process, event);
                    for envelope in &envelopes {
                        state.journal.publish(envelope);
                        if let Ok(id) = ConversationId::from_string(&envelope.conversation_id) {
                            state
                                .primary
                                .entry(id)
                                .or_insert_with(|| primary_tx.clone());
                            let _ = primary_tx.send(envelope.clone());
                        }
                        state.observe_delegation(process, envelope);
                        state.notify_child_request(envelope);
                    }
                    let ended = state
                        .registry
                        .conversation_of(process)
                        .and_then(|id| state.registry.live_process(&id))
                        .is_none();
                    if ended {
                        // Holding a handle to an ended session would keep its
                        // runtime, and so this stream, alive.
                        state.handles.remove(&process);
                        state.configs.remove(&process);
                        if let Some(id) = state.registry.conversation_of(process) {
                            state.primary.remove(&id);
                        }
                        shared.process_ended.notify_waiters();
                    }
                };
                delegation::wake(&shared);
            }
            let mut state = shared.state();
            if let Some(id) = state.registry.conversation_of(process) {
                state.primary.remove(&id);
            }
            state.registry.process_exited(process);
            state.configs.remove(&process);
            state.handles.remove(&process);
            shared.process_ended.notify_waiters();
            drop(state);
            delegation::wake(&shared);
        });

        Ok(ManagedSession {
            conversation_id,
            handle,
            events: primary_rx,
        })
    }

    /// Resume a failed session from its transcript on a new agent process.
    ///
    /// The session keeps its conversation ID, position, family links and
    /// envelope sequence while its agent process is replaced.
    pub async fn resume(
        &self,
        failed: &ConversationId,
        spec: AgentPrepareSpec,
    ) -> Result<ManagedSession, ResumeError> {
        let record = self.get(failed).ok_or(ResumeError::Unknown(*failed))?;
        if record.status != SessionStatus::Failed {
            return Err(ResumeError::NotFailed(*failed));
        }
        if crate::resolve_agent_slug(&spec.config.active_agent)
            != crate::resolve_agent_slug(&record.agent)
        {
            return Err(ResumeError::AgentMismatch {
                id: *failed,
                expected: record.agent,
                actual: spec.config.active_agent.clone(),
            });
        }
        let path = record
            .transcript_path
            .ok_or(ResumeError::NoTranscript(*failed))?;
        let transcript =
            load_transcript_from_path(&path)
                .await
                .map_err(|source| ResumeError::Transcript {
                    id: *failed,
                    source,
                })?;
        let resumed = self
            .start_session(
                StartSession {
                    agent: SessionAgent::Prepare(spec),
                    start: SessionStart::Resume(SessionResume {
                        acp_session_id: record.acp_session_id,
                        transcript: Some(transcript),
                    }),
                    parent: record.parent,
                    source: record.source,
                },
                Some(*failed),
            )
            .map_err(|error| match error {
                StartError::NotFailed(id) => ResumeError::NotFailed(id),
                error => ResumeError::Start(error),
            })?;
        Ok(resumed)
    }

    /// Resume a session this manager has not run, from its transcript on disk.
    ///
    /// The session keeps the transcript's conversation ID, so it continues
    /// the same transcript file. The harness picks native `session/load`,
    /// live `session/resume`, or client-side replay as for any resume. The
    /// manager journal starts at sequence 1 for it.
    pub fn resume_stored(
        &self,
        id: ConversationId,
        transcript: Transcript,
        spec: AgentPrepareSpec,
    ) -> Result<ManagedSession, ResumeError> {
        if transcript.meta.session_id != id.to_string() {
            return Err(ResumeError::Transcript {
                id,
                source: std::io::Error::other(format!(
                    "the transcript records session {}",
                    transcript.meta.session_id
                )),
            });
        }
        self.start_session(
            StartSession {
                agent: SessionAgent::Prepare(spec),
                start: SessionStart::Resume(SessionResume {
                    acp_session_id: transcript.meta.acp_session_id.clone(),
                    transcript: Some(transcript),
                }),
                parent: None,
                source: SessionSource::User,
            },
            Some(id),
        )
        .map_err(|error| match error {
            StartError::NotFailed(id) => ResumeError::NotFailed(id),
            error => ResumeError::Start(error),
        })
    }

    pub fn get(&self, conversation_id: &ConversationId) -> Option<SessionRecord> {
        self.shared.state().registry.record(conversation_id)
    }

    /// Every session, live or ended, in the order it was started.
    pub fn list(&self) -> Vec<SessionRecord> {
        self.shared.state().registry.records()
    }

    /// Status of the session's family: its top-level session and every
    /// descendant. If any member needs the user, the family does.
    pub fn family_status(&self, conversation_id: &ConversationId) -> Option<SessionStatus> {
        self.shared.state().registry.family_status(conversation_id)
    }

    /// Most sessions that may hold an agent process at once.
    pub fn max_sessions(&self) -> i32 {
        self.shared.state().registry.limit()
    }

    /// How many more sessions can start before the cap is reached. A session
    /// gives its slot back when its agent process ends.
    pub fn free_slots(&self) -> i32 {
        self.shared.state().registry.free_slots()
    }

    /// Control handle of a session that still has an agent process.
    pub fn handle(&self, conversation_id: &ConversationId) -> Option<HarnessHandle> {
        let state = self.shared.state();
        let process = state.registry.live_process(conversation_id)?;
        state.handles.get(&process).cloned()
    }

    /// Follow every session's envelopes from now on. A subscriber that falls
    /// `capacity` envelopes behind is dropped and its receiver closes; the
    /// sessions are never slowed down.
    pub fn subscribe(&self, capacity: i32) -> mpsc::Receiver<SessionEnvelope> {
        self.shared.state().journal.subscribe(None, capacity)
    }

    /// Shut down one session, waiting at most the configured time limit.
    /// Closing a session that has already ended does nothing. Closing a
    /// session that was forked at head closes the child that took over its
    /// agent process. On success the registry has observed its end and its
    /// capacity slot is available for a replacement.
    pub async fn close(
        &self,
        conversation_id: &ConversationId,
        child_grace: Duration,
    ) -> anyhow::Result<()> {
        let ids = {
            let _admission = self.shared.admission.lock().await;
            let mut state = self.shared.state();
            state.cancel_tree(conversation_id)
        };
        for id in ids {
            self.close_one(&id, child_grace).await?;
        }
        Ok(())
    }

    async fn close_one(
        &self,
        conversation_id: &ConversationId,
        child_grace: Duration,
    ) -> anyhow::Result<()> {
        let process_and_handle = {
            let state = self.shared.state();
            state
                .registry
                .process_following_forks(conversation_id)
                .and_then(|process| {
                    state
                        .handles
                        .get(&process)
                        .cloned()
                        .map(|handle| (process, handle))
                })
        };
        let Some((process, handle)) = process_and_handle else {
            return Ok(());
        };
        let limit = self.shared.config.shutdown_timeout;
        tokio::time::timeout(limit, async {
            handle.shutdown_with_grace(child_grace).await?;
            loop {
                // Register before reading state so an end observed between
                // the check and await cannot leave this close waiting forever.
                let ended = self.shared.process_ended.notified();
                tokio::pin!(ended);
                ended.as_mut().enable();
                if !self.shared.state().handles.contains_key(&process) {
                    return Ok(());
                }
                ended.await;
            }
        })
        .await
        .map_err(|_| {
            anyhow::anyhow!("session {conversation_id} did not shut down within {limit:?}")
        })?
    }

    /// Refuse new sessions and shut every live session down in parallel, each
    /// within the configured time limit. `child_grace` lets each agent process
    /// handle stdin EOF before it is stopped.
    pub async fn shutdown(&self, child_grace: Duration) -> ShutdownReport {
        let live: Vec<(ConversationId, HarnessHandle)> = {
            let mut state = self.shared.state();
            state.registry.close_admission();
            let ids: Vec<_> = state
                .registry
                .records()
                .into_iter()
                .map(|r| r.conversation_id)
                .collect();
            for id in ids {
                state.cancel_tree(&id);
            }
            state
                .handles
                .iter()
                .filter_map(|(process, handle)| {
                    let id = state.registry.conversation_of(*process)?;
                    (state.registry.live_process(&id) == Some(*process))
                        .then(|| (id, handle.clone()))
                })
                .collect()
        };
        let limit = self.shared.config.shutdown_timeout;
        let mut closing = JoinSet::new();
        for (id, handle) in live {
            closing.spawn(async move {
                (
                    id,
                    tokio::time::timeout(limit, handle.shutdown_with_grace(child_grace)).await,
                )
            });
        }
        let mut report = ShutdownReport::default();
        while let Some(joined) = closing.join_next().await {
            match joined {
                Ok((id, Ok(Ok(())))) => report.closed.push(id),
                Ok((id, Ok(Err(error)))) => report.failed.push((id, error.to_string())),
                Ok((id, Err(_elapsed))) => report.timed_out.push(id),
                Err(error) => tracing::warn!(%error, "session shutdown task failed"),
            }
        }
        report
    }
}
