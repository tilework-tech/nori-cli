//! Session runtime: launches an ACP backend from a [`SessionLaunchSpec`] and
//! owns the orchestration that frontends previously had to implement
//! themselves — Nori config assembly, the connect/shutdown/timeout race, the
//! op-forwarding loop, the session-control command loop, and backend event
//! forwarding.
//!
//! Frontends build a spec from their own configuration source, call
//! [`launch_session`], and consume [`SessionEvent`]s; no terminal or UI
//! concepts appear here.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use codex_rmcp_client::OAuthCredentialsStoreMode;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::oneshot;

use crate::backend::AcpBackend;
use crate::backend::AcpBackendConfig;
use crate::backend::BackendEvent;
use crate::backend::PromptAdmission;
use crate::backend::SessionContext;
pub use crate::backend::prepared::PreparedAgent;
pub use crate::backend::prepared::SessionCatalog;
use crate::session_event_fanout::SessionEventFanout;
use crate::transcript::Transcript;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::Notice;
use nori_protocol::RequestFailure;
use nori_protocol::RequestFailureKind;
use nori_protocol::SessionEndReason;
use nori_protocol::SessionEnded;
pub use nori_protocol::SessionEvent;
use nori_protocol::acp;
use nori_protocol::acp::v1::SessionConfigOption;

/// Duration before showing a warning that connection is taking too long.
const CONNECT_WARNING_SECS: u64 = 8;
/// Duration after the warning before forcibly aborting the connection attempt.
const CONNECT_ABORT_SECS: u64 = 30;

/// Two-phase timeout: warn after `CONNECT_WARNING_SECS`, abort after an
/// additional `CONNECT_ABORT_SECS`.
async fn spawn_timeout_sequence(event_tx: &SessionEventFanout) {
    tokio::time::sleep(Duration::from_secs(CONNECT_WARNING_SECS)).await;
    let _ = event_tx.send(SessionEvent::Nori(NoriEvent::Notice(Notice {
        message: format!(
            "Connection is taking longer than expected. \
             Will abort in {CONNECT_ABORT_SECS}s if still unresponsive."
        ),
    })));
    tokio::time::sleep(Duration::from_secs(CONNECT_ABORT_SECS)).await;
}

fn forward_connect_event(
    event_tx: &SessionEventFanout,
    event: SessionEvent,
    raw_acp_error_observed: &mut bool,
) {
    *raw_acp_error_observed |= matches!(
        &event,
        SessionEvent::Acp(AcpEvent::Response {
            response: Err(_),
            ..
        })
    );
    let _ = event_tx.send(event);
}

/// Command for controlling ACP session state exposed by the agent.
enum HarnessCommand {
    /// Submit one ACP prompt and return its transport request ID.
    Prompt {
        content: Vec<acp::v1::ContentBlock>,
        meta: Option<acp::v1::Meta>,
        admission: PromptAdmission,
        response_tx: oneshot::Sender<anyhow::Result<acp::v1::RequestId>>,
    },
    FollowUp {
        submission: crate::follow_up::Submission,
        response_tx: oneshot::Sender<anyhow::Result<crate::FollowUpReceipt>>,
    },
    /// Shut down the active harness session.
    Shutdown {
        child_grace: std::time::Duration,
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
    /// Respond to a delegated agent-to-client ACP request.
    RespondToAgent {
        request_id: acp::v1::RequestId,
        response: Result<acp::v1::ClientResponse, acp::v1::Error>,
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
    Cancel {
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
    /// Let a paused queue send again.
    ResumeQueue {
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
    /// Take a queued prompt back before it is sent.
    ReclaimQueuedPrompt {
        event_id: String,
        response_tx: oneshot::Sender<anyhow::Result<Option<String>>>,
    },
    CancelRequest {
        request_id: acp::v1::RequestId,
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
    AddHistory {
        text: String,
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
    HistoryEntry {
        log_id: i64,
        offset: i64,
        response_tx: oneshot::Sender<anyhow::Result<Option<crate::HistoryEntry>>>,
    },
    SearchHistory {
        max_results: i64,
        response_tx: oneshot::Sender<anyhow::Result<Vec<crate::HistoryEntry>>>,
    },
    CustomPrompts {
        response_tx: oneshot::Sender<Vec<crate::CustomPrompt>>,
    },
    Compact {
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
    Branch {
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
    Undo {
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
    UndoSnapshots {
        response_tx: oneshot::Sender<Vec<crate::UndoSnapshot>>,
    },
    UndoTo {
        index: i64,
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
    RunUserShell {
        command: String,
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
    SetApprovalPolicy {
        policy: nori_config::AskForApproval,
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
    Goal {
        response_tx: oneshot::Sender<Option<nori_protocol::ThreadGoal>>,
    },
    SetGoal {
        objective: String,
        status: Option<nori_protocol::ThreadGoalStatus>,
        response_tx: oneshot::Sender<anyhow::Result<nori_protocol::ThreadGoal>>,
    },
    SetGoalStatus {
        status: nori_protocol::ThreadGoalStatus,
        response_tx: oneshot::Sender<anyhow::Result<nori_protocol::ThreadGoal>>,
    },
    ClearGoal {
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
    /// Get the current ACP session config snapshot.
    GetSessionConfig {
        response_tx: oneshot::Sender<Vec<SessionConfigOption>>,
    },
    /// Set an ACP session config option.
    SetSessionConfigOption {
        config_id: String,
        value: String,
        response_tx: oneshot::Sender<anyhow::Result<Vec<SessionConfigOption>>>,
    },
    /// List the agent's known sessions via ACP `session/list`.
    ListSessions {
        cwd: PathBuf,
        response_tx: oneshot::Sender<anyhow::Result<Vec<acp::v1::SessionInfo>>>,
    },
    /// Close (release) the active session via ACP `session/close`.
    CloseSession {
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
    /// Register an additional bounded consumer of the public event stream.
    SubscribeEvents {
        response_tx: oneshot::Sender<mpsc::Receiver<SessionEvent>>,
    },
    SnapshotTranscript {
        response_tx: oneshot::Sender<anyhow::Result<String>>,
    },
    /// Flush the session transcript to disk (write barrier).
    FlushTranscript {
        response_tx: oneshot::Sender<anyhow::Result<()>>,
    },
}

/// The command transport disappeared before accepting or answering an action.
/// Backend errors retain their own types so shutdown never masks an ACP failure.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct HarnessTransportClosed(&'static str);

/// The agent never started, so a command waiting for it cannot run. Carries
/// the agent's ACP error code when it refused with one, for example
/// `-32000` (authentication required).
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct AgentStartFailed {
    pub message: String,
    pub code: Option<i32>,
}

impl AgentStartFailed {
    fn new(message: String, error: Option<&anyhow::Error>) -> Self {
        let code = error.and_then(|error| {
            error
                .chain()
                .find_map(|cause| cause.downcast_ref::<acp::v1::Error>())
                .map(|error| i32::from(error.code))
        });
        Self { message, code }
    }

    /// Answer commands that arrived while the agent was starting.
    fn fail_waiting(&self, commands: VecDeque<HarnessCommand>) {
        for command in commands {
            match command {
                HarnessCommand::Prompt { response_tx, .. } => {
                    let _ = response_tx.send(Err(anyhow::Error::new(self.clone())));
                }
                HarnessCommand::FollowUp { response_tx, .. } => {
                    let _ = response_tx.send(Err(anyhow::Error::new(self.clone())));
                }
                _ => {}
            }
        }
    }
}

/// Typed handle for controlling one Nori harness session.
#[derive(Clone)]
pub struct HarnessHandle {
    command_tx: mpsc::UnboundedSender<HarnessCommand>,
    normal_shutdown: Arc<AtomicBool>,
    first_prompt_started: Option<Arc<FirstPromptStarted>>,
    agent_response_observer: Option<AgentResponseObserver>,
    responder: Arc<str>,
    context_timeline: Arc<tokio::sync::RwLock<crate::context_timeline::ContextTimelineState>>,
}

/// Called with the request ID and the answering handle's responder label just
/// before any clone of a handle sends an answer to an agent request, so the
/// observer sees the answer before anything the agent does in reaction to it.
pub(crate) type AgentResponseObserver = Arc<dyn Fn(&acp::v1::RequestId, &str) + Send + Sync>;

/// Responder label of a handle that was never given one.
pub const DEFAULT_RESPONDER: &str = "client";

struct FirstPromptStarted {
    fired: std::sync::atomic::AtomicBool,
    callback: Box<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for HarnessHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HarnessHandle")
            .field("command_tx", &self.command_tx)
            .field(
                "has_first_prompt_started_callback",
                &self.first_prompt_started.is_some(),
            )
            .finish()
    }
}

impl HarnessHandle {
    /// Whether an action lost its command transport during this process's
    /// normal teardown. Actual backend failures remain reportable, including
    /// failures returned before a subsequent shutdown.
    pub fn is_expected_shutdown_error(&self, error: &anyhow::Error) -> bool {
        self.normal_shutdown.load(Ordering::Acquire)
            && error.downcast_ref::<HarnessTransportClosed>().is_some()
    }

    /// Return the context observations captured at prompt boundaries.
    pub async fn context_timeline(
        &self,
    ) -> anyhow::Result<crate::context_timeline::ContextTimeline> {
        let waiting = std::time::Instant::now();
        let state = self.context_timeline.read().await;
        let lock_wait_us = waiting.elapsed().as_secs_f64() * 1_000_000.0;
        let cloning = std::time::Instant::now();
        let timeline = state.timeline.clone();
        let clone_us = cloning.elapsed().as_secs_f64() * 1_000_000.0;
        drop(state);
        tracing::debug!(
            target: "nori_context_profile",
            stage = "harness_snapshot",
            lock_wait_us,
            clone_us,
            turn_count = timeline.turns.len(),
            segment_count = timeline.segments.len(),
            "captured context timeline snapshot",
        );
        Ok(timeline)
    }

    /// Attach a callback that fires once, after this session's first prompt
    /// reaches the ACP transport and receives its wire request id.
    pub fn with_first_prompt_started_callback(
        mut self,
        callback: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        self.first_prompt_started = Some(Arc::new(FirstPromptStarted {
            fired: std::sync::atomic::AtomicBool::new(false),
            callback: Box::new(callback),
        }));
        self
    }

    /// Label this handle's answers to agent requests with `responder`, so
    /// clients following the session can tell who answered.
    pub fn with_responder(mut self, responder: impl Into<String>) -> Self {
        self.responder = Arc::from(responder.into());
        self
    }

    pub(crate) fn with_agent_response_observer(mut self, observer: AgentResponseObserver) -> Self {
        self.agent_response_observer = Some(observer);
        self
    }

    /// Submit a prompt and return the ACP request ID used on the wire.
    pub async fn prompt(
        &self,
        content: Vec<acp::v1::ContentBlock>,
    ) -> anyhow::Result<acp::v1::RequestId> {
        self.prompt_with_meta(content, None).await
    }

    /// Submit against the observed wire turn. Commands enter the session FIFO
    /// immediately, even if the caller awaits their receipts concurrently.
    pub fn follow_up(
        &self,
        message_id: String,
        expected_turn: acp::v1::RequestId,
        intent: crate::FollowUpIntent,
        prompt: Vec<acp::v1::ContentBlock>,
        sender: String,
    ) -> impl std::future::Future<Output = anyhow::Result<crate::FollowUpReceipt>> + Send + 'static
    {
        self.submit_follow_up(crate::follow_up::Submission {
            message_id,
            sender,
            expected_turn,
            intent,
            prompt,
            origin: crate::follow_up::PromptOrigin::External,
        })
    }

    /// Submit a follow-up from the local composer. Only its own plain-text
    /// queued messages can be taken back for editing.
    pub fn local_follow_up(
        &self,
        message_id: String,
        expected_turn: acp::v1::RequestId,
        intent: crate::FollowUpIntent,
        prompt: Vec<acp::v1::ContentBlock>,
    ) -> impl std::future::Future<Output = anyhow::Result<crate::FollowUpReceipt>> + Send + 'static
    {
        self.submit_follow_up(crate::follow_up::Submission {
            message_id,
            sender: "local".into(),
            expected_turn,
            intent,
            prompt,
            origin: crate::follow_up::PromptOrigin::LocalComposer,
        })
    }

    fn submit_follow_up(
        &self,
        submission: crate::follow_up::Submission,
    ) -> impl std::future::Future<Output = anyhow::Result<crate::FollowUpReceipt>> + Send + 'static
    {
        let (response_tx, response_rx) = oneshot::channel();
        let submitted = self
            .command_tx
            .send(HarnessCommand::FollowUp {
                submission,
                response_tx,
            })
            .is_ok();
        async move {
            anyhow::ensure!(submitted, "session is closed");
            response_rx
                .await
                .map_err(|_| anyhow::anyhow!("follow-up receipt lost; delivery unknown"))?
        }
    }

    pub(crate) async fn prompt_with_meta(
        &self,
        content: Vec<acp::v1::ContentBlock>,
        meta: Option<acp::v1::Meta>,
    ) -> anyhow::Result<acp::v1::RequestId> {
        self.prompt_with_meta_and_admission(content, meta, PromptAdmission::Queue)
            .await
    }

    /// Submit only when idle, rejecting busy sessions atomically at admission.
    pub async fn prompt_with_meta_if_idle(
        &self,
        content: Vec<acp::v1::ContentBlock>,
        meta: Option<acp::v1::Meta>,
    ) -> anyhow::Result<acp::v1::RequestId> {
        self.submit_prompt_if_idle(content, meta).await
    }

    /// Enqueue the idle-only command now and resolve on transport admission.
    pub fn submit_prompt_if_idle(
        &self,
        content: Vec<acp::v1::ContentBlock>,
        meta: Option<acp::v1::Meta>,
    ) -> impl std::future::Future<Output = anyhow::Result<acp::v1::RequestId>> + Send + 'static
    {
        self.prompt_with_meta_and_admission(content, meta, PromptAdmission::RejectIfBusy)
    }

    fn prompt_with_meta_and_admission(
        &self,
        content: Vec<acp::v1::ContentBlock>,
        meta: Option<acp::v1::Meta>,
        admission: PromptAdmission,
    ) -> impl std::future::Future<Output = anyhow::Result<acp::v1::RequestId>> + Send + 'static
    {
        let (response_tx, response_rx) = oneshot::channel();
        let sent = self
            .command_tx
            .send(HarnessCommand::Prompt {
                content,
                meta,
                admission,
                response_tx,
            })
            .is_ok();
        let first_prompt_started = self.first_prompt_started.clone();
        async move {
            if !sent {
                return Err(anyhow::Error::new(HarnessTransportClosed(
                    "ACP agent command channel closed",
                )));
            }
            let request_id = response_rx.await.map_err(|_| {
                anyhow::Error::new(HarnessTransportClosed(
                    "ACP agent did not accept the prompt",
                ))
            })??;
            if let Some(first_prompt_started) = &first_prompt_started
                && !first_prompt_started
                    .fired
                    .swap(true, std::sync::atomic::Ordering::AcqRel)
            {
                (first_prompt_started.callback)();
            }
            Ok(request_id)
        }
    }

    /// Shut down the active harness session.
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        self.shutdown_with_grace(std::time::Duration::ZERO).await
    }

    /// Shut down the active harness session after allowing the ACP child to
    /// process stdin EOF for `child_grace`.
    pub async fn shutdown_with_grace(
        &self,
        child_grace: std::time::Duration,
    ) -> anyhow::Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(HarnessCommand::Shutdown {
                child_grace,
                response_tx,
            })
            .map_err(|_| {
                anyhow::Error::new(HarnessTransportClosed("ACP agent command channel closed"))
            })?;
        response_rx.await.map_err(|_| {
            anyhow::Error::new(HarnessTransportClosed("ACP agent did not shut down"))
        })?
    }

    /// Complete a delegated ACP request using its original request ID.
    pub async fn respond_to_agent(
        &self,
        request_id: acp::v1::RequestId,
        response: Result<acp::v1::ClientResponse, acp::v1::Error>,
    ) -> anyhow::Result<()> {
        if let Some(observer) = &self.agent_response_observer {
            observer(&request_id, &self.responder);
        }
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(HarnessCommand::RespondToAgent {
                request_id,
                response,
                response_tx,
            })
            .map_err(|_| {
                anyhow::Error::new(HarnessTransportClosed("ACP agent command channel closed"))
            })?;
        response_rx.await.map_err(|_| {
            anyhow::Error::new(HarnessTransportClosed(
                "ACP request response was not accepted",
            ))
        })?
    }

    pub async fn cancel(&self) -> anyhow::Result<()> {
        self.request(|response_tx| HarnessCommand::Cancel { response_tx })
            .await
    }

    /// Let a queue paused by an interrupted or failed turn send again. The
    /// next queued prompt starts at once when the session is idle; otherwise
    /// the queue drains after the running turn completes. A queue that is not
    /// paused is left alone.
    pub async fn resume_queue(&self) -> anyhow::Result<()> {
        self.request(|response_tx| HarnessCommand::ResumeQueue { response_tx })
            .await
    }

    /// Remove the queued prompt `event_id` (a [`nori_protocol::QueueEntry`]
    /// ID) before it is sent and return the text the user typed. `None` means
    /// it already started, was already reclaimed, or is not editable. Its
    /// pending `prompt` call fails with [`crate::backend::PromptWithdrawn`].
    pub async fn reclaim_queued_prompt(&self, event_id: String) -> anyhow::Result<Option<String>> {
        self.request(|response_tx| HarnessCommand::ReclaimQueuedPrompt {
            event_id,
            response_tx,
        })
        .await
    }

    pub(crate) async fn cancel_request(
        &self,
        request_id: acp::v1::RequestId,
    ) -> anyhow::Result<()> {
        self.request(|response_tx| HarnessCommand::CancelRequest {
            request_id,
            response_tx,
        })
        .await
    }

    pub async fn add_history(&self, text: String) -> anyhow::Result<()> {
        self.request(|response_tx| HarnessCommand::AddHistory { text, response_tx })
            .await
    }

    pub async fn history_entry(
        &self,
        log_id: i64,
        offset: i64,
    ) -> anyhow::Result<Option<crate::HistoryEntry>> {
        self.request(|response_tx| HarnessCommand::HistoryEntry {
            log_id,
            offset,
            response_tx,
        })
        .await
    }

    pub async fn search_history(
        &self,
        max_results: i64,
    ) -> anyhow::Result<Vec<crate::HistoryEntry>> {
        self.request(|response_tx| HarnessCommand::SearchHistory {
            max_results,
            response_tx,
        })
        .await
    }

    pub async fn custom_prompts(&self) -> anyhow::Result<Vec<crate::CustomPrompt>> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(HarnessCommand::CustomPrompts { response_tx })
            .map_err(|_| {
                anyhow::Error::new(HarnessTransportClosed("ACP agent command channel closed"))
            })?;
        response_rx
            .await
            .map_err(|_| anyhow::Error::new(HarnessTransportClosed("ACP agent did not respond")))
    }

    pub async fn compact(&self) -> anyhow::Result<()> {
        self.request(|response_tx| HarnessCommand::Compact { response_tx })
            .await
    }

    /// Branch the conversation at its current head via ACP `session/fork`.
    pub async fn branch(&self) -> anyhow::Result<()> {
        self.request(|response_tx| HarnessCommand::Branch { response_tx })
            .await
    }

    pub async fn undo(&self) -> anyhow::Result<()> {
        self.request(|response_tx| HarnessCommand::Undo { response_tx })
            .await
    }

    pub async fn undo_snapshots(&self) -> anyhow::Result<Vec<crate::UndoSnapshot>> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(HarnessCommand::UndoSnapshots { response_tx })
            .map_err(|_| {
                anyhow::Error::new(HarnessTransportClosed("ACP agent command channel closed"))
            })?;
        response_rx
            .await
            .map_err(|_| anyhow::Error::new(HarnessTransportClosed("ACP agent did not respond")))
    }

    pub async fn undo_to(&self, index: i64) -> anyhow::Result<()> {
        self.request(|response_tx| HarnessCommand::UndoTo { index, response_tx })
            .await
    }

    pub async fn run_user_shell(&self, command: String) -> anyhow::Result<()> {
        self.request(|response_tx| HarnessCommand::RunUserShell {
            command,
            response_tx,
        })
        .await
    }

    pub async fn set_approval_policy(
        &self,
        policy: nori_config::AskForApproval,
    ) -> anyhow::Result<()> {
        self.request(|response_tx| HarnessCommand::SetApprovalPolicy {
            policy,
            response_tx,
        })
        .await
    }

    pub async fn goal(&self) -> anyhow::Result<Option<nori_protocol::ThreadGoal>> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(HarnessCommand::Goal { response_tx })
            .map_err(|_| {
                anyhow::Error::new(HarnessTransportClosed("ACP agent command channel closed"))
            })?;
        response_rx
            .await
            .map_err(|_| anyhow::Error::new(HarnessTransportClosed("ACP agent did not respond")))
    }

    pub async fn set_goal(
        &self,
        objective: String,
        status: Option<nori_protocol::ThreadGoalStatus>,
    ) -> anyhow::Result<nori_protocol::ThreadGoal> {
        self.request(|response_tx| HarnessCommand::SetGoal {
            objective,
            status,
            response_tx,
        })
        .await
    }

    pub async fn set_goal_status(
        &self,
        status: nori_protocol::ThreadGoalStatus,
    ) -> anyhow::Result<nori_protocol::ThreadGoal> {
        self.request(|response_tx| HarnessCommand::SetGoalStatus {
            status,
            response_tx,
        })
        .await
    }

    pub async fn clear_goal(&self) -> anyhow::Result<()> {
        self.request(|response_tx| HarnessCommand::ClearGoal { response_tx })
            .await
    }

    async fn request<T>(
        &self,
        command: impl FnOnce(oneshot::Sender<anyhow::Result<T>>) -> HarnessCommand,
    ) -> anyhow::Result<T> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx.send(command(response_tx)).map_err(|_| {
            anyhow::Error::new(HarnessTransportClosed("ACP agent command channel closed"))
        })?;
        response_rx
            .await
            .map_err(|_| anyhow::Error::new(HarnessTransportClosed("ACP agent did not respond")))?
    }

    /// Get the current ACP session config snapshot from the agent.
    pub async fn get_session_config(&self) -> Option<Vec<SessionConfigOption>> {
        let (response_tx, response_rx) = oneshot::channel();
        if self
            .command_tx
            .send(HarnessCommand::GetSessionConfig { response_tx })
            .is_err()
        {
            return None;
        }
        response_rx.await.ok()
    }

    /// Set an ACP session config option value.
    pub async fn set_session_config_option(
        &self,
        config_id: String,
        value: String,
    ) -> anyhow::Result<Vec<SessionConfigOption>> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(HarnessCommand::SetSessionConfigOption {
                config_id,
                value,
                response_tx,
            })
            .map_err(|_| {
                anyhow::Error::new(HarnessTransportClosed("ACP agent command channel closed"))
            })?;
        response_rx
            .await
            .map_err(|_| anyhow::Error::new(HarnessTransportClosed("ACP agent did not respond")))?
    }

    /// List the agent's known sessions via ACP `session/list`.
    pub async fn list_sessions(&self, cwd: PathBuf) -> anyhow::Result<Vec<acp::v1::SessionInfo>> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(HarnessCommand::ListSessions { cwd, response_tx })
            .map_err(|_| {
                anyhow::Error::new(HarnessTransportClosed("ACP agent command channel closed"))
            })?;
        response_rx
            .await
            .map_err(|_| anyhow::Error::new(HarnessTransportClosed("ACP agent did not respond")))?
    }

    /// Close (release) the active session via ACP `session/close`.
    pub async fn close_session(&self) -> anyhow::Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(HarnessCommand::CloseSession { response_tx })
            .map_err(|_| {
                anyhow::Error::new(HarnessTransportClosed("ACP agent command channel closed"))
            })?;
        response_rx
            .await
            .map_err(|_| anyhow::Error::new(HarnessTransportClosed("ACP agent did not respond")))?
    }

    /// Subscribe to the ordered public event stream as an additional bounded
    /// consumer. The primary receiver from [`launch_session`] is unaffected.
    /// A subscriber that falls a full queue behind is dropped and its
    /// receiver closes.
    pub async fn subscribe_events(&self) -> anyhow::Result<mpsc::Receiver<SessionEvent>> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(HarnessCommand::SubscribeEvents { response_tx })
            .map_err(|_| {
                anyhow::Error::new(HarnessTransportClosed("ACP agent command channel closed"))
            })?;
        response_rx
            .await
            .map_err(|_| anyhow::Error::new(HarnessTransportClosed("ACP agent did not respond")))
    }

    pub(crate) async fn snapshot_transcript(&self) -> anyhow::Result<String> {
        self.request(|response_tx| HarnessCommand::SnapshotTranscript { response_tx })
            .await
    }

    /// Flush the session transcript to disk and wait for the write barrier.
    pub async fn flush_transcript(&self) -> anyhow::Result<()> {
        self.request(|response_tx| HarnessCommand::FlushTranscript { response_tx })
            .await
    }
}

/// Resume parameters for reattaching to a previous session.
#[derive(Debug, Clone)]
pub struct SessionResume {
    /// The agent-side ACP session id, when known (enables `session/load`).
    pub acp_session_id: Option<String>,
    /// Transcript for client-side replay when the agent can't load sessions.
    pub transcript: Option<Transcript>,
}

/// Everything a frontend must supply to initialize and inspect an ACP agent.
pub struct AgentPrepareSpec {
    /// Fully resolved process configuration supplied by the frontend.
    pub config: Arc<crate::config::NoriConfig>,
    /// Frontend version recorded in transcript metadata after activation.
    pub cli_version: String,
    /// Product-level context injected into the first prompt after activation.
    pub session_context: Option<SessionContext>,
    /// Conversation history injected into the first prompt after activation.
    pub initial_context: Option<String>,
}

/// Explicit session directive applied to a prepared ACP connection.
#[derive(Debug, Clone)]
#[expect(
    clippy::large_enum_variant,
    reason = "SessionStart is a one-shot control value and keeps the public resume API direct"
)]
pub enum SessionStart {
    /// Create a fresh ACP session.
    New,
    /// Load, resume, or replay an existing session.
    Resume(SessionResume),
}

/// Everything a frontend must supply to activate a prepared connection.
pub struct SessionLaunchSpec {
    /// Uniquely owned initialized ACP connection.
    pub agent: PreparedAgent,
    /// Session action chosen after inspecting the agent.
    pub start: SessionStart,
}

/// A running session and its ordered event stream.
pub struct LaunchedSession {
    /// Typed handle for ACP and Nori session operations.
    pub handle: HarnessHandle,
    /// Session event stream; closes when the backend shuts down.
    pub events: UnboundedReceiver<SessionEvent>,
}

/// Initialize an ACP connection and inspect its session catalog without
/// creating, loading, or resuming a session.
pub async fn prepare_agent(spec: AgentPrepareSpec) -> anyhow::Result<PreparedAgent> {
    PreparedAgent::prepare(build_backend_config(spec))
        .await
        .map_err(|failure| failure.error)
}

/// Refresh session-time configuration after preparation but before activation.
///
/// The initialized transport remains unchanged. The agent identity and working
/// directory must still match the connection that was prepared.
pub fn refresh_prepared_agent(
    agent: &mut PreparedAgent,
    spec: AgentPrepareSpec,
) -> anyhow::Result<()> {
    agent.replace_activation_config(build_backend_config(spec))
}

pub(crate) fn build_backend_config(spec: AgentPrepareSpec) -> AcpBackendConfig {
    let AgentPrepareSpec {
        config,
        cli_version,
        session_context,
        initial_context,
    } = spec;

    // Detect auto-worktree repo root from the cwd path.
    // When auto_worktree is enabled, cwd is {repo_root}/.worktrees/{name},
    // so we can derive repo_root by going up two directories.
    let auto_worktree_repo_root = if config.auto_worktree.is_enabled() {
        config
            .cwd
            .parent()
            .filter(|path| path.file_name().is_some_and(|name| name == ".worktrees"))
            .and_then(std::path::Path::parent)
            .map(std::path::Path::to_path_buf)
    } else {
        None
    };
    // Resolve to Off if no worktree actually exists (e.g. "ask" mode
    // where the user declined).
    let auto_worktree = if auto_worktree_repo_root.is_some() {
        config.auto_worktree
    } else {
        crate::config::AutoWorktree::Off
    };
    let (agent, saved_model) =
        crate::registry::resolve_agent_selection(&config.active_agent, &config.default_models);
    let default_model = config
        .default_model_override_keys
        .iter()
        .find(|key| crate::resolve_agent_slug(key) == agent)
        .and_then(|key| config.default_models.get(key))
        .cloned()
        .or(saved_model);

    let token_provider = config.agent_token_providers.get(&agent).cloned();
    let default_model = token_provider
        .as_ref()
        .map(|binding| nori_acp_host::token_provider::bound_model_id(&agent, binding))
        .or(default_model);
    AcpBackendConfig {
        agent_token_providers: Arc::new(config.agent_token_providers.clone()),
        agent,
        multi_session: config.multi_session,
        delegation_context: None,
        cwd: config.cwd.clone(),
        approval_policy: config.approval_policy,
        sandbox_policy: config.sandbox_policy.clone(),
        notify: config.notify.clone(),
        os_notifications: config.os_notifications,
        notify_after_idle: config.notify_after_idle,
        nori_home: config.nori_home.clone(),
        history_persistence: config.history_persistence,
        acp_proxy: config.acp_proxy.clone(),
        cli_version,
        auto_worktree,
        auto_worktree_repo_root,
        prompt_summary_enabled: config.footer_segment_config.prompt_summary,
        session_start_hooks: config.session_start_hooks.clone(),
        session_end_hooks: config.session_end_hooks.clone(),
        pre_user_prompt_hooks: config.pre_user_prompt_hooks.clone(),
        post_user_prompt_hooks: config.post_user_prompt_hooks.clone(),
        pre_tool_call_hooks: config.pre_tool_call_hooks.clone(),
        post_tool_call_hooks: config.post_tool_call_hooks.clone(),
        pre_agent_response_hooks: config.pre_agent_response_hooks.clone(),
        post_agent_response_hooks: config.post_agent_response_hooks.clone(),
        async_session_start_hooks: config.async_session_start_hooks.clone(),
        async_session_end_hooks: config.async_session_end_hooks.clone(),
        async_pre_user_prompt_hooks: config.async_pre_user_prompt_hooks.clone(),
        async_post_user_prompt_hooks: config.async_post_user_prompt_hooks.clone(),
        async_pre_tool_call_hooks: config.async_pre_tool_call_hooks.clone(),
        async_post_tool_call_hooks: config.async_post_tool_call_hooks.clone(),
        async_pre_agent_response_hooks: config.async_pre_agent_response_hooks.clone(),
        async_post_agent_response_hooks: config.async_post_agent_response_hooks.clone(),
        script_timeout: config.script_timeout.as_duration(),
        default_model,
        initial_context,
        session_context,
        mcp_servers: config.mcp_servers.clone(),
        mcp_oauth_credentials_store_mode: OAuthCredentialsStoreMode::default(),
        conversation_id: None,
    }
}

/// Activate a prepared ACP connection with the selected session directive.
///
/// Returns immediately; activation happens on a background task and handle
/// calls queue until the session is ready.
pub fn launch_session(spec: SessionLaunchSpec) -> LaunchedSession {
    launch_session_from(
        SessionAgentSource::Prepared(Box::new(spec.agent)),
        spec.start,
    )
}

/// Prepare an agent and activate the selected session on the same connection.
///
/// This convenience path is for products that have already made an explicit
/// session choice and therefore do not need to render a pre-session picker.
/// Preparation still happens separately and its connection is never respawned.
pub fn prepare_and_launch_session(spec: AgentPrepareSpec, start: SessionStart) -> LaunchedSession {
    launch_session_from(
        SessionAgentSource::Prepare(Box::new(build_backend_config(spec))),
        start,
    )
}

pub(crate) enum SessionAgentSource {
    Prepared(Box<PreparedAgent>),
    Prepare(Box<AcpBackendConfig>),
}

pub(crate) fn launch_session_from(
    source: SessionAgentSource,
    start: SessionStart,
) -> LaunchedSession {
    let (agent_cmd_tx, mut agent_cmd_rx) = unbounded_channel::<HarnessCommand>();
    let (primary_event_tx, event_rx) = unbounded_channel::<SessionEvent>();
    let event_tx = SessionEventFanout::new(primary_event_tx);

    let resumed_transcript_id = match &start {
        SessionStart::Resume(resume) => resume
            .transcript
            .as_ref()
            .map(|transcript| transcript.meta.session_id.clone()),
        SessionStart::New => None,
    };
    let initial_timeline = match &start {
        SessionStart::Resume(resume) => resume
            .transcript
            .as_ref()
            .map(Transcript::context_timeline_state)
            .unwrap_or_else(|| {
                let mut state = crate::context_timeline::ContextTimelineState::default();
                state.timeline.incomplete_history = true;
                state
            }),
        SessionStart::New => crate::context_timeline::ContextTimelineState::default(),
    };
    let context_timeline = Arc::new(tokio::sync::RwLock::new(initial_timeline));
    let normal_shutdown = Arc::new(AtomicBool::new(false));
    let handle = HarnessHandle {
        command_tx: agent_cmd_tx,
        normal_shutdown: Arc::clone(&normal_shutdown),
        first_prompt_started: None,
        agent_response_observer: None,
        responder: Arc::from(DEFAULT_RESPONDER),
        context_timeline: Arc::clone(&context_timeline),
    };

    tokio::spawn(async move {
        // Single ACP backend channel for both control-plane and normalized
        // session-domain events.
        let (backend_event_tx, mut backend_event_rx) = mpsc::channel(32);

        let connect = async move {
            let agent = match source {
                SessionAgentSource::Prepared(agent) => *agent,
                SessionAgentSource::Prepare(config) => {
                    match PreparedAgent::prepare(*config).await {
                        Ok(agent) => agent,
                        Err(failure) => {
                            for event in failure.setup_events {
                                let _ = backend_event_tx.send(BackendEvent::Public(event)).await;
                            }
                            let failure_label = match &start {
                                SessionStart::New => "Failed to start ACP session",
                                SessionStart::Resume(_) => "Failed to resume ACP session",
                            };
                            return Err((failure_label, failure.error));
                        }
                    }
                }
            };
            let start = agent
                .automatic_session_id()
                .map(|session_id| {
                    SessionStart::Resume(SessionResume {
                        acp_session_id: Some(session_id.to_string()),
                        transcript: None,
                    })
                })
                .unwrap_or(start);
            let failure_label = match &start {
                SessionStart::New => "Failed to start ACP session",
                SessionStart::Resume(_) => "Failed to resume ACP session",
            };
            let result = match start {
                SessionStart::New => AcpBackend::spawn(agent, backend_event_tx).await,
                SessionStart::Resume(resume) => {
                    AcpBackend::resume_session(
                        agent,
                        resume.acp_session_id.as_deref(),
                        resume.transcript.as_ref(),
                        backend_event_tx,
                    )
                    .await
                }
            };
            result.map_err(|error| (failure_label, error))
        };

        // Race backend init against shutdown requests and a timeout.
        // This ensures the user can always exit even if the backend hangs.
        let mut connect = std::pin::pin!(connect);
        let mut timeout = std::pin::pin!(spawn_timeout_sequence(&event_tx));
        let mut pending_commands = VecDeque::new();
        let mut raw_acp_error_observed = false;
        let mut startup_evidence = Vec::new();
        let backend = loop {
            tokio::select! {
            result = &mut connect => {
                match result {
                    Ok(b) => break Arc::new(b),
                    Err((failure_label, e)) => {
                        tracing::error!("{failure_label}: {e}");
                        let message = format!("{failure_label}: {e}");
                        while let Ok(BackendEvent::Public(event)) = backend_event_rx.try_recv() {
                            forward_connect_event(
                                &event_tx,
                                event,
                                &mut raw_acp_error_observed,
                            );
                        }
                        if !raw_acp_error_observed {
                            let _ = event_tx.send(SessionEvent::Nori(NoriEvent::RequestFailed(
                                RequestFailure {
                                    request_id: None,
                                    message: message.clone(),
                                    kind: RequestFailureKind::Fatal,
                                },
                            )));
                        }
                        AgentStartFailed::new(message.clone(), Some(&e))
                            .fail_waiting(std::mem::take(&mut pending_commands));
                        let _ = event_tx.send(SessionEvent::Nori(NoriEvent::SessionEnded(
                            SessionEnded {
                                reason: SessionEndReason::SpawnFailed,
                                message: Some(message),
                            },
                        )));
                        return;
                    }
                }
            }
            command = agent_cmd_rx.recv() => {
                match command {
                    Some(HarnessCommand::Shutdown { response_tx, .. }) => {
                        normal_shutdown.store(true, Ordering::Release);
                        let _ = response_tx.send(Ok(()));
                        let _ = event_tx.send(SessionEvent::Nori(NoriEvent::SessionEnded(
                            SessionEnded {
                                reason: SessionEndReason::Shutdown,
                                message: None,
                            },
                        )));
                        return;
                    }
                    // Register immediately (not queued) so a subscriber
                    // attached right after launch cannot miss startup events
                    // such as `SessionStarted`.
                    Some(HarnessCommand::SubscribeEvents { response_tx }) => {
                        let _ = response_tx.send(event_tx.subscribe());
                    }
                    Some(command) => pending_commands.push_back(command),
                    None => return,
                }
            }
            Some(BackendEvent::Public(event)) = backend_event_rx.recv() => {
                startup_evidence.push(observe_context_event(&context_timeline, &event).await);
                forward_connect_event(&event_tx, event, &mut raw_acp_error_observed);
            }
            () = &mut timeout => {
                tracing::warn!("ACP backend connection timed out");
                let message = "Connection timed out. The agent did not respond.".to_string();
                let _ = event_tx.send(SessionEvent::Nori(NoriEvent::RequestFailed(
                    RequestFailure {
                        request_id: None,
                        message: message.clone(),
                        kind: RequestFailureKind::Retryable,
                    },
                )));
                AgentStartFailed::new(message.clone(), None)
                    .fail_waiting(std::mem::take(&mut pending_commands));
                let _ = event_tx.send(SessionEvent::Nori(NoriEvent::SessionEnded(
                    SessionEnded {
                        reason: SessionEndReason::TimedOut,
                        message: Some(message),
                    },
                )));
                return;
            }
            }
        };

        // Hold the shared recorder cell (not a one-time clone) so a branch-at-head
        // fork swap redirects recording to the new transcript; otherwise post-fork
        // events would keep landing in the frozen parent transcript.
        let transcript_recorder_cell = backend.transcript_recorder_cell();
        let history_already_seeded =
            transcript_recorder_cell
                .read()
                .await
                .as_ref()
                .is_some_and(|recorder| {
                    resumed_transcript_id.as_deref() == Some(recorder.session_id())
                });
        let mut replaying = false;
        if let Some(recorder) = transcript_recorder_cell.read().await.clone() {
            for evidence in startup_evidence {
                if matches!(
                    evidence.event,
                    SessionEvent::Nori(NoriEvent::ReplayStarted(_))
                ) {
                    replaying = true;
                }
                if !(history_already_seeded && replaying)
                    && let Err(error) = recorder.record_session_event(&evidence).await
                {
                    tracing::warn!(%error, "failed to record startup context evidence");
                }
                if matches!(
                    evidence.event,
                    SessionEvent::Nori(NoriEvent::ReplayFinished)
                ) {
                    replaying = false;
                }
            }
        }
        let backend_for_agent = Arc::clone(&backend);
        let event_fanout = event_tx.clone();
        let command_normal_shutdown = Arc::clone(&normal_shutdown);
        tokio::spawn(async move {
            loop {
                let cmd = match pending_commands.pop_front() {
                    Some(command) => command,
                    None => match agent_cmd_rx.recv().await {
                        Some(command) => command,
                        None => break,
                    },
                };
                match cmd {
                    HarnessCommand::Prompt {
                        content,
                        meta,
                        admission,
                        response_tx,
                    } => {
                        backend_for_agent
                            .submit_prompt(content, meta, admission, response_tx)
                            .await;
                    }
                    HarnessCommand::FollowUp {
                        submission,
                        response_tx,
                    } => {
                        backend_for_agent
                            .submit_follow_up(submission, response_tx)
                            .await;
                    }
                    HarnessCommand::Shutdown {
                        child_grace,
                        response_tx,
                    } => {
                        command_normal_shutdown.store(true, Ordering::Release);
                        let result = backend_for_agent.shutdown(child_grace).await;
                        let shutdown_complete = result.is_ok();
                        command_normal_shutdown.store(shutdown_complete, Ordering::Release);
                        let _ = response_tx.send(result);
                        if shutdown_complete {
                            break;
                        }
                    }
                    HarnessCommand::RespondToAgent {
                        request_id,
                        response,
                        response_tx,
                    } => {
                        let result = backend_for_agent
                            .respond_to_agent(request_id, response)
                            .await;
                        let _ = response_tx.send(result);
                    }
                    HarnessCommand::Cancel { response_tx } => {
                        let _ = response_tx.send(backend_for_agent.cancel().await);
                    }
                    HarnessCommand::ResumeQueue { response_tx } => {
                        let _ = response_tx.send(backend_for_agent.resume_queue().await);
                    }
                    HarnessCommand::ReclaimQueuedPrompt {
                        event_id,
                        response_tx,
                    } => {
                        let _ = response_tx
                            .send(backend_for_agent.reclaim_queued_prompt(event_id).await);
                    }
                    HarnessCommand::CancelRequest {
                        request_id,
                        response_tx,
                    } => {
                        let _ =
                            response_tx.send(backend_for_agent.cancel_request(request_id).await);
                    }
                    HarnessCommand::AddHistory { text, response_tx } => {
                        let _ = response_tx.send(backend_for_agent.add_history(text).await);
                    }
                    HarnessCommand::HistoryEntry {
                        log_id,
                        offset,
                        response_tx,
                    } => {
                        let _ =
                            response_tx.send(backend_for_agent.history_entry(log_id, offset).await);
                    }
                    HarnessCommand::SearchHistory {
                        max_results,
                        response_tx,
                    } => {
                        let _ =
                            response_tx.send(backend_for_agent.search_history(max_results).await);
                    }
                    HarnessCommand::CustomPrompts { response_tx } => {
                        let _ = response_tx.send(backend_for_agent.custom_prompts().await);
                    }
                    HarnessCommand::Compact { response_tx } => {
                        let _ = response_tx.send(backend_for_agent.compact().await);
                    }
                    HarnessCommand::Branch { response_tx } => {
                        let _ = response_tx.send(backend_for_agent.branch().await);
                    }
                    HarnessCommand::Undo { response_tx } => {
                        let _ = response_tx.send(backend_for_agent.undo().await);
                    }
                    HarnessCommand::UndoSnapshots { response_tx } => {
                        let _ = response_tx.send(backend_for_agent.undo_snapshots().await);
                    }
                    HarnessCommand::UndoTo { index, response_tx } => {
                        let _ = response_tx.send(backend_for_agent.undo_to(index).await);
                    }
                    HarnessCommand::RunUserShell {
                        command,
                        response_tx,
                    } => {
                        let _ = response_tx.send(backend_for_agent.run_user_shell(command).await);
                    }
                    HarnessCommand::SetApprovalPolicy {
                        policy,
                        response_tx,
                    } => {
                        backend_for_agent.set_approval_policy(policy);
                        let _ = response_tx.send(Ok(()));
                    }
                    HarnessCommand::Goal { response_tx } => {
                        let _ = response_tx.send(backend_for_agent.current_goal().await);
                    }
                    HarnessCommand::SetGoal {
                        objective,
                        status,
                        response_tx,
                    } => {
                        let _ =
                            response_tx.send(backend_for_agent.set_goal(objective, status).await);
                    }
                    HarnessCommand::SetGoalStatus {
                        status,
                        response_tx,
                    } => {
                        let _ = response_tx.send(backend_for_agent.set_goal_status(status).await);
                    }
                    HarnessCommand::ClearGoal { response_tx } => {
                        let _ = response_tx.send(backend_for_agent.clear_goal().await);
                    }
                    HarnessCommand::GetSessionConfig { response_tx } => {
                        let state = backend_for_agent.config_options();
                        let _ = response_tx.send(state);
                    }
                    HarnessCommand::SetSessionConfigOption {
                        config_id,
                        value,
                        response_tx,
                    } => {
                        let result = backend_for_agent
                            .set_config_option(config_id, value)
                            .await
                            .map(|()| backend_for_agent.config_options());
                        let _ = response_tx.send(result);
                    }
                    HarnessCommand::ListSessions { cwd, response_tx } => {
                        let result = backend_for_agent.connection().list_sessions(&cwd).await;
                        let _ = response_tx.send(result);
                    }
                    HarnessCommand::CloseSession { response_tx } => {
                        command_normal_shutdown.store(true, Ordering::Release);
                        let result = backend_for_agent.close_active_session().await;
                        let close_complete = result.is_ok();
                        command_normal_shutdown.store(close_complete, Ordering::Release);
                        let _ = response_tx.send(result);
                        if close_complete {
                            break;
                        }
                    }
                    HarnessCommand::SubscribeEvents { response_tx } => {
                        let _ = response_tx.send(event_fanout.subscribe());
                    }
                    HarnessCommand::SnapshotTranscript { response_tx } => {
                        let result = async {
                            let recorder = backend_for_agent
                                .transcript_recorder_cell()
                                .read()
                                .await
                                .clone()
                                .ok_or_else(|| {
                                    anyhow::anyhow!("private recovery transcript is unavailable")
                                })?;
                            Ok(recorder.checkpoint().await?)
                        }
                        .await;
                        let _ = response_tx.send(result);
                    }
                    HarnessCommand::FlushTranscript { response_tx } => {
                        let _ = response_tx.send(backend_for_agent.flush_transcript().await);
                    }
                }
            }
        });

        // Drop our Arc reference; the typed command task owns the backend.
        drop(backend);

        let mut recorder_stopped = false;
        while let Some(BackendEvent::Public(event)) = backend_event_rx.recv().await {
            let session_ended = matches!(event, SessionEvent::Nori(NoriEvent::SessionEnded(_)));
            if matches!(
                &event,
                SessionEvent::Nori(NoriEvent::SessionEnded(SessionEnded {
                    reason: SessionEndReason::Shutdown | SessionEndReason::Closed,
                    ..
                }))
            ) {
                normal_shutdown.store(true, Ordering::Release);
            }
            if matches!(event, SessionEvent::Nori(NoriEvent::ReplayStarted(_))) {
                replaying = true;
            }
            let evidence = observe_context_event(&context_timeline, &event).await;
            if let Some(recorder) = transcript_recorder_cell.read().await.clone() {
                if !(history_already_seeded && replaying)
                    && let Err(error) = recorder.record_session_event(&evidence).await
                {
                    tracing::warn!(%error, "failed to record public session event");
                }
                // A consumer may restart immediately after observing SessionEnded.
                // Commit its complete transcript before publishing that boundary.
                if session_ended {
                    if let Err(error) = recorder.shutdown().await {
                        tracing::warn!(%error, "failed to shut down transcript recorder");
                    }
                    recorder_stopped = true;
                }
            }
            if matches!(event, SessionEvent::Nori(NoriEvent::ReplayFinished)) {
                replaying = false;
            }
            if event_tx.send(event).is_err() {
                break;
            }
            if session_ended {
                break;
            }
        }
        if !recorder_stopped
            && let Some(recorder) = transcript_recorder_cell.read().await.clone()
            && let Err(error) = recorder.shutdown().await
        {
            tracing::warn!(%error, "failed to shut down transcript recorder");
        }
    });

    LaunchedSession {
        handle,
        events: event_rx,
    }
}

async fn observe_context_event(
    timeline: &tokio::sync::RwLock<crate::context_timeline::ContextTimelineState>,
    event: &SessionEvent,
) -> crate::transcript::SessionEventEntry {
    let mut evidence = crate::transcript::SessionEventEntry::new(event.clone());
    let mut state = timeline.write().await;
    if let (Some(id), Some(at)) = (&evidence.observation_id, &evidence.observed_at) {
        state.observe(event, id, at);
    }
    evidence.history_incomplete = state.timeline.incomplete_history;
    evidence
}

#[cfg(test)]
mod tests {
    use super::*;
    use nori_protocol::acp::v1::SessionConfigOptionCategory;
    use nori_protocol::acp::v1::SessionConfigSelectOption;
    use pretty_assertions::assert_eq;

    #[test]
    fn launch_uses_one_agent_and_model_across_provider_aliases() {
        for (requested, saved, expected) in [
            ("agy", "antigravity", "gemini"),
            ("gemini", "agy", "gemini"),
            ("grok", "grok-build", "grok-build"),
            ("grok-build", "grok", "grok-build"),
        ] {
            let config = crate::config::NoriConfig {
                active_agent: requested.into(),
                default_models: [(saved.into(), "selected".into())].into(),
                ..Default::default()
            };
            let backend = build_backend_config(AgentPrepareSpec {
                config: Arc::new(config),
                cli_version: "test".into(),
                initial_context: None,
                session_context: None,
            });
            assert_eq!(
                (backend.agent.as_str(), backend.default_model.as_deref()),
                (expected, Some("selected"))
            );
        }
    }

    #[test]
    fn last_explicit_model_override_wins_across_aliases() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("config.toml");
        std::fs::write(&path, "[default_models]\ngemini = 'saved'\n").unwrap();
        let before_table = ("default_models.agy".into(), "early".into());
        let table: toml::Table = "gemini = 'canonical'\nagy = 'legacy'".parse().unwrap();
        for (raw_overrides, expected) in [
            (
                vec![("default_models.agy".into(), "explicit".into())],
                "explicit",
            ),
            (
                vec![
                    ("default_models.gemini".into(), "first".into()),
                    ("default_models.antigravity".into(), "last".into()),
                    ("default_models.codex".into(), "other-agent".into()),
                ],
                "last",
            ),
            (
                vec![before_table, ("default_models".into(), table.into())],
                "canonical",
            ),
        ] {
            let config = nori_config::NoriConfig::load_from_path_with_overrides(
                &path,
                nori_config::NoriConfigOverrides {
                    agent: Some("agy".into()),
                    raw_overrides,
                    ..Default::default()
                },
            )
            .unwrap();
            let backend = build_backend_config(AgentPrepareSpec {
                config: Arc::new(config),
                cli_version: "test".into(),
                initial_context: None,
                session_context: None,
            });
            assert_eq!(backend.default_model.as_deref(), Some(expected));
        }
    }

    fn mode_option(current_value: &str) -> SessionConfigOption {
        SessionConfigOption::select(
            "mode",
            "Mode",
            current_value.to_string(),
            vec![
                SessionConfigSelectOption::new("plan", "Plan"),
                SessionConfigSelectOption::new("build", "Build"),
            ],
        )
        .category(SessionConfigOptionCategory::Mode)
    }

    #[tokio::test]
    async fn set_session_config_option_returns_refreshed_config_snapshot() {
        let (command_tx, mut command_rx) = unbounded_channel::<HarnessCommand>();
        tokio::spawn(async move {
            while let Some(command) = command_rx.recv().await {
                if let HarnessCommand::SetSessionConfigOption {
                    config_id: _,
                    value,
                    response_tx,
                } = command
                {
                    let _ = response_tx.send(Ok(vec![mode_option(&value)]));
                }
            }
        });
        let handle = HarnessHandle {
            command_tx,
            normal_shutdown: Arc::default(),
            first_prompt_started: None,
            agent_response_observer: None,
            responder: Arc::from(DEFAULT_RESPONDER),
            context_timeline: Arc::default(),
        };

        let config_options = handle
            .set_session_config_option("mode".to_string(), "build".to_string())
            .await
            .unwrap();

        assert_eq!(config_options, vec![mode_option("build")]);
    }

    #[tokio::test]
    async fn first_prompt_callback_fires_once_across_cloned_handles() {
        let (command_tx, mut command_rx) = unbounded_channel::<HarnessCommand>();
        tokio::spawn(async move {
            let mut request_id = 0_i64;
            while let Some(command) = command_rx.recv().await {
                if let HarnessCommand::Prompt { response_tx, .. } = command {
                    request_id += 1;
                    let _ = response_tx.send(Ok(request_id.into()));
                }
            }
        });
        let callback_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let callback_count_for_handle = Arc::clone(&callback_count);
        let handle = HarnessHandle {
            command_tx,
            normal_shutdown: Arc::default(),
            first_prompt_started: None,
            agent_response_observer: None,
            responder: Arc::from(DEFAULT_RESPONDER),
            context_timeline: Arc::default(),
        }
        .with_first_prompt_started_callback(move || {
            callback_count_for_handle.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        let cloned_handle = handle.clone();

        handle.prompt(Vec::new()).await.expect("first prompt");
        cloned_handle
            .prompt(Vec::new())
            .await
            .expect("second prompt");

        assert_eq!(callback_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}

#[cfg(test)]
#[path = "runtime_resume_tests.rs"]
mod resume_tests;
