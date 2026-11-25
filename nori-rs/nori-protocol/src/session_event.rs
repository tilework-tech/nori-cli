use std::collections::HashMap;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

use crate::acp;
use crate::acp::v1::RequestId;

/// An event emitted by one Nori harness session.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
        #[serde(deserialize_with = "deserialize_agent_response")]
        response: Result<acp::v1::AgentResponse, acp::v1::Error>,
    },
}

/// ACP's untagged `AgentResponse` lets the all-optional
/// `AuthenticateResponse` absorb any payload, so a prompt response read back
/// from the wire is recognised by its required `stopReason`.
fn deserialize_agent_response<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Result<acp::v1::AgentResponse, acp::v1::Error>, D::Error> {
    use serde::de::Error as _;
    match Result::<serde_json::Value, acp::v1::Error>::deserialize(deserializer)? {
        Err(error) => Ok(Err(error)),
        Ok(payload) if payload.get("stopReason").is_some() => serde_json::from_value(payload)
            .map(|response| Ok(acp::v1::AgentResponse::PromptResponse(response)))
            .map_err(D::Error::custom),
        Ok(payload) => serde_json::from_value(payload)
            .map(Ok)
            .map_err(D::Error::custom),
    }
}

/// Notifications owned by the Nori harness rather than ACP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "event_type", content = "event", rename_all = "snake_case")]
pub enum NoriEvent {
    SessionStarted(SessionStarted),
    SessionPhaseChanged(SessionPhase),
    ExternalTurnChanged(ExternalTurn),
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
    RequestResolved(RequestResolved),
    DelegationChanged(Delegation),
    /// The session's permission mode: sent once it starts and after every
    /// change, so each client can show the mode of the session it follows.
    ApprovalModeChanged(SessionApprovalModes),
}

/// A session's permission mode and the modes its agent can honor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionApprovalModes {
    pub current: ApprovalMode,
    /// The modes `session/set_mode` accepts for this session. Ask is left
    /// out for an agent with no mode that asks before every edit and
    /// command.
    pub available: Vec<ApprovalMode>,
}

/// How a session answers its agent's permission requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalMode {
    /// Ask before every edit and command.
    Ask,
    /// The agent asks when it judges it needs to.
    Agent,
    /// Every request is approved without asking.
    ApproveAll,
}

impl ApprovalMode {
    /// The name clients show for the mode.
    pub fn name(self) -> &'static str {
        match self {
            Self::Ask => "Ask",
            Self::Agent => "Agent",
            Self::ApproveAll => "Approve all",
        }
    }
}

/// Persistent and agent-side identities established for a harness session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum SessionPhase {
    Idle,
    Loading { request_id: RequestId },
    Prompting { request_id: RequestId },
    Cancelling { request_id: RequestId },
}

/// Observed activity for which the harness has no outstanding prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExternalTurn {
    /// Harness-local observation identity, distinct from ACP request IDs.
    pub id: String,
    pub state: ExternalTurnState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExternalTurnState {
    Working,
    CancelRequested,
    /// The peer has not reported a terminal boundary since the request.
    Unconfirmed,
    /// The peer reported idle without confirming cancellation.
    Idle,
    /// A different turn took over the session.
    Superseded,
    /// The peer reported a cancelled stop reason for the same identified turn.
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionEnded {
    pub reason: SessionEndReason,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionEndReason {
    Shutdown,
    Closed,
    ConnectionLost,
    SpawnFailed,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct QueueSnapshot {
    /// Display text of each queued user prompt, oldest first.
    pub prompts: Vec<String>,
    /// The same rows with stable IDs, so a client can take one back with
    /// `HarnessHandle::reclaim_queued_prompt`. Empty from hosts that predate it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<QueueEntry>,
    /// Set while queued prompts wait for an explicit resume instead of
    /// starting on their own. Absent from hosts that predate it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused: Option<QueuePause>,
}

/// Why the queue stopped sending on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueuePause {
    /// The turn before the queued prompts was interrupted.
    Interrupted,
    /// The turn before the queued prompts failed or stopped early.
    TurnFailed,
}

/// One queued user prompt, in delivery order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct QueueEntry {
    /// The prompt's submission event ID; unique within the session.
    pub id: String,
    pub text: String,
    /// Whether the prompt is plain text that can return to the composer
    /// without losing content such as images.
    #[serde(default)]
    pub editable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReplayStarted {
    pub source: ReplaySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReplaySource {
    Transcript,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContextCompactedEvent {
    pub summary: Option<String>,
}

/// Emitted when branch-at-head `/fork` forks the transcript: the previous
/// conversation is frozen and a fresh conversation is seeded from it and made
/// active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionForked {
    pub previous_conversation_id: String,
    pub new_conversation_id: String,
    pub new_acp_session_id: acp::v1::SessionId,
}

/// Capabilities implemented by Nori around the ACP agent.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct NoriCapabilities {
    pub goal_management: bool,
    pub builtin_commands: HashMap<String, CommandAvailability>,
}

/// Availability of a harness-owned command in the active session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CommandAvailability {
    pub enabled: bool,
    pub reason: Option<String>,
}

/// Persistent goal state owned by the Nori harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ThreadGoal {
    pub objective: String,
    pub status: ThreadGoalStatus,
    pub tokens_used: i64,
    pub time_used_seconds: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ThreadGoalStatus {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    BudgetLimited,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UserShellStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HookOutput {
    pub message: String,
    pub level: HookOutputLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HookOutputLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PromptSummary {
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Notice {
    pub message: String,
}

/// A client answered a pending agent request, such as a permission request.
///
/// The session manager emits this on the session's envelope stream, so every
/// client that follows the session learns that the request no longer waits
/// on the user, whichever client answered it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RequestResolved {
    /// The ID of the agent request that was answered.
    pub request_id: acp::v1::RequestId,
    /// Label of the client that answered, for example `client` or
    /// `remote_control`.
    pub answered_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RequestFailure {
    pub request_id: Option<RequestId>,
    pub message: String,
    pub kind: RequestFailureKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RequestFailureKind {
    Retryable,
    Fatal,
}

/// Manager-owned delegation state, attributed to both participating sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Delegation {
    pub request_id: String,
    pub parent_id: String,
    pub child_id: String,
    pub parent_agent: String,
    pub agent: String,
    pub task: String,
    pub status: DelegationStatus,
    pub depth: i32,
    pub result: Option<String>,
    /// Who asked for the child. Events from older hosts omit it.
    #[serde(default)]
    pub origin: DelegationOrigin,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DelegationOrigin {
    /// The parent agent called the `delegate` tool.
    #[default]
    Agent,
    /// The user ran `/delegate` or a client called the manager directly.
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DelegationStatus {
    AwaitingApproval,
    Running,
    Declined,
    Completed,
    Failed,
    Cancelled,
}

#[cfg(test)]
mod response_wire_tests {
    use super::*;

    fn round_trip(event: &AcpEvent) -> AcpEvent {
        serde_json::from_value(serde_json::to_value(event).unwrap()).unwrap()
    }

    #[test]
    fn a_prompt_response_stays_a_prompt_response_across_the_wire() {
        let sent = AcpEvent::Response {
            request_id: RequestId::Str("synthetic-turn".into()),
            response: Ok(acp::v1::AgentResponse::PromptResponse(
                acp::v1::PromptResponse::new(acp::v1::StopReason::EndTurn),
            )),
        };
        let received = round_trip(&sent);
        assert!(
            matches!(
                &received,
                AcpEvent::Response {
                    response: Ok(acp::v1::AgentResponse::PromptResponse(response)),
                    ..
                } if response.stop_reason == acp::v1::StopReason::EndTurn
            ),
            "{received:?}"
        );
    }

    #[test]
    fn an_error_response_stays_an_error_across_the_wire() {
        let sent = AcpEvent::Response {
            request_id: RequestId::Str("synthetic-turn".into()),
            response: Err(acp::v1::Error::internal_error()),
        };
        assert!(matches!(
            round_trip(&sent),
            AcpEvent::Response {
                response: Err(_),
                ..
            }
        ));
    }
}
