use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::acp;
use crate::acp::v1::RequestId;

/// An event emitted by one Nori harness session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "source", content = "event", rename_all = "snake_case")]
// Keep the approved schema-shaped API unboxed; consumers should not inherit a
// Nori-specific indirection solely because ACP's response aggregate is large.
#[allow(clippy::large_enum_variant)]
pub enum SessionEvent {
    /// Semantics owned by the Agent Client Protocol.
    Acp(AcpEvent),
    /// Harness behavior that ACP does not define.
    Nori(NoriEvent),
}

/// Raw ACP traffic emitted by the agent toward its client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "message_type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum AcpEvent {
    Notification(acp::v1::AgentNotification),
    Request {
        request_id: RequestId,
        request: acp::v1::AgentRequest,
    },
    Response {
        request_id: RequestId,
        response: Result<acp::v1::AgentResponse, acp::v1::Error>,
    },
}

/// Notifications owned by the Nori harness rather than ACP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event_type", content = "event", rename_all = "snake_case")]
pub enum NoriEvent {
    SessionStarted(SessionStarted),
    SessionPhaseChanged(SessionPhase),
    SessionEnded(SessionEnded),
    QueueChanged(QueueSnapshot),
    ReplayStarted(ReplayStarted),
    ReplayFinished,
    ContextCompacted(ContextCompactedEvent),
    SessionForked(SessionForked),
    GoalChanged(Option<ThreadGoal>),
    CapabilitiesChanged(NoriCapabilities),
    Undo(UndoEvent),
    UserShell(UserShellEvent),
    HookOutput(HookOutput),
    PromptSummaryUpdated(PromptSummary),
    Notice(Notice),
    RequestFailed(RequestFailure),
}

/// Persistent and agent-side identities established for a harness session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStarted {
    pub transcript_id: Option<String>,
    pub acp_session_id: acp::v1::SessionId,
    pub cwd: PathBuf,
    pub transcript_path: Option<PathBuf>,
    pub history_log_id: i64,
    pub history_entry_count: i64,
    /// The session configuration the agent advertised when the session was
    /// created, so clients can show the agent's model, mode, and other options
    /// from the first frame instead of waiting for an update.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config_options: Vec<acp::v1::SessionConfigOption>,
}

/// The harness-owned phase of the active streaming operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum SessionPhase {
    Idle,
    Loading { request_id: RequestId },
    Prompting { request_id: RequestId },
    Cancelling { request_id: RequestId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEnded {
    pub reason: SessionEndReason,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEndReason {
    Shutdown,
    Closed,
    ConnectionLost,
    SpawnFailed,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueSnapshot {
    pub prompts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayStarted {
    pub source: ReplaySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplaySource {
    Transcript,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextCompactedEvent {
    pub summary: Option<String>,
}

/// Emitted when branch-at-head `/fork` forks the transcript: the previous
/// conversation is frozen and a fresh conversation is seeded from it and made
/// active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionForked {
    pub previous_conversation_id: String,
    pub new_conversation_id: String,
    pub new_acp_session_id: acp::v1::SessionId,
}

/// Capabilities implemented by Nori around the ACP agent.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NoriCapabilities {
    pub goal_management: bool,
    pub builtin_commands: HashMap<String, CommandAvailability>,
}

/// Availability of a harness-owned command in the active session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandAvailability {
    pub enabled: bool,
    pub reason: Option<String>,
}

/// Persistent goal state owned by the Nori harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadGoal {
    pub objective: String,
    pub status: ThreadGoalStatus,
    pub tokens_used: i64,
    pub time_used_seconds: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThreadGoalStatus {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    BudgetLimited,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UndoEvent {
    Started {
        operation_id: String,
        message: Option<String>,
    },
    Completed {
        operation_id: String,
        success: bool,
        message: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UserShellEvent {
    Started {
        operation_id: String,
        command: String,
        cwd: PathBuf,
    },
    Output {
        operation_id: String,
        stream: UserShellStream,
        chunk: Vec<u8>,
    },
    Finished {
        operation_id: String,
        exit_code: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserShellStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookOutput {
    pub message: String,
    pub level: HookOutputLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookOutputLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptSummary {
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestFailure {
    pub request_id: Option<RequestId>,
    pub message: String,
    pub kind: RequestFailureKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestFailureKind {
    Retryable,
    Fatal,
}
