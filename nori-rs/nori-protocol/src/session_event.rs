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
///
/// On the wire a response to `session/set_config_option`, `session/load` or
/// `session/resume` also names that `method`. ACP's untagged `AgentResponse`
/// cannot tell those three apart when they carry nothing but options, and a
/// load or resume must never read back as a write.
#[derive(Debug, Clone)]
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
    /// The options a response reported, read from a journal written before
    /// responses named their method. It may answer a load, a resume or a
    /// set, so it says only what the session's options were. It is written
    /// back as the response it was read from.
    ConfigSnapshot {
        request_id: RequestId,
        config_options: Vec<acp::v1::SessionConfigOption>,
        meta: Option<serde_json::Map<String, serde_json::Value>>,
    },
}

/// The request a response answers, named on the wire for the responses
/// ACP's untagged `AgentResponse` cannot tell apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ResponseMethod {
    #[serde(rename = "session/set_config_option")]
    SetConfigOption,
    #[serde(rename = "session/load")]
    Load,
    #[serde(rename = "session/resume")]
    Resume,
}

/// The method a response names on the wire, for the responses that need one.
fn method_of(response: &acp::v1::AgentResponse) -> Option<ResponseMethod> {
    match response {
        acp::v1::AgentResponse::SetSessionConfigOptionResponse(_) => {
            Some(ResponseMethod::SetConfigOption)
        }
        acp::v1::AgentResponse::LoadSessionResponse(_) => Some(ResponseMethod::Load),
        acp::v1::AgentResponse::ResumeSessionResponse(_) => Some(ResponseMethod::Resume),
        _ => None,
    }
}

// The wire form of `AcpEvent`, which its schema describes.
/// Raw ACP traffic emitted by the agent toward its client.
#[derive(JsonSchema)]
#[serde(tag = "message_type", rename_all = "snake_case")]
#[allow(dead_code, clippy::large_enum_variant)]
enum AcpEventSchema {
    Notification(acp::v1::AgentNotification),
    Request {
        request_id: RequestId,
        request: acp::v1::AgentRequest,
    },
    Response {
        request_id: RequestId,
        /// Set for responses to `session/set_config_option`, `session/load`
        /// and `session/resume`, whose results can otherwise look alike.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        method: Option<ResponseMethod>,
        response: Result<acp::v1::AgentResponse, acp::v1::Error>,
    },
}

impl JsonSchema for AcpEvent {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "AcpEvent".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        AcpEventSchema::json_schema(generator)
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum WirePayload<'a> {
    Answer(&'a acp::v1::AgentResponse),
    Snapshot {
        #[serde(rename = "configOptions")]
        config_options: &'a [acp::v1::SessionConfigOption],
        #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
        meta: Option<&'a serde_json::Map<String, serde_json::Value>>,
    },
}

#[derive(Serialize)]
#[serde(tag = "message_type", rename_all = "snake_case")]
enum WireOut<'a> {
    Notification(&'a acp::v1::AgentNotification),
    Request {
        request_id: &'a RequestId,
        request: &'a acp::v1::AgentRequest,
    },
    Response {
        request_id: &'a RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        method: Option<ResponseMethod>,
        response: Result<WirePayload<'a>, &'a acp::v1::Error>,
    },
}

impl Serialize for AcpEvent {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            AcpEvent::Notification(notification) => WireOut::Notification(notification),
            AcpEvent::Request {
                request_id,
                request,
            } => WireOut::Request {
                request_id,
                request,
            },
            AcpEvent::Response {
                request_id,
                response,
            } => WireOut::Response {
                request_id,
                method: response.as_ref().ok().and_then(method_of),
                response: response.as_ref().map(WirePayload::Answer),
            },
            AcpEvent::ConfigSnapshot {
                request_id,
                config_options,
                meta,
            } => WireOut::Response {
                request_id,
                method: None,
                response: Ok(WirePayload::Snapshot {
                    config_options,
                    meta: meta.as_ref(),
                }),
            },
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
#[serde(tag = "message_type", rename_all = "snake_case")]
enum WireIn {
    Notification(acp::v1::AgentNotification),
    Request {
        request_id: RequestId,
        request: acp::v1::AgentRequest,
    },
    Response {
        request_id: RequestId,
        #[serde(default)]
        method: Option<String>,
        response: Result<serde_json::Value, acp::v1::Error>,
    },
}

impl<'de> Deserialize<'de> for AcpEvent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        Ok(match WireIn::deserialize(deserializer)? {
            WireIn::Notification(notification) => AcpEvent::Notification(notification),
            WireIn::Request {
                request_id,
                request,
            } => AcpEvent::Request {
                request_id,
                request,
            },
            WireIn::Response {
                request_id,
                method: _,
                response: Err(error),
            } => AcpEvent::Response {
                request_id,
                response: Err(error),
            },
            WireIn::Response {
                request_id,
                method,
                response: Ok(payload),
            } => match read_answer(method.as_deref(), payload).map_err(D::Error::custom)? {
                Answer::Typed(response) => AcpEvent::Response {
                    request_id,
                    response: Ok(response),
                },
                Answer::Snapshot {
                    config_options,
                    meta,
                } => AcpEvent::ConfigSnapshot {
                    request_id,
                    config_options,
                    meta,
                },
            },
        })
    }
}

#[allow(clippy::large_enum_variant)]
enum Answer {
    Typed(acp::v1::AgentResponse),
    Snapshot {
        config_options: Vec<acp::v1::SessionConfigOption>,
        meta: Option<serde_json::Map<String, serde_json::Value>>,
    },
}

/// One successful response as written. A named method decides its type. An
/// unnamed one is recognised by a prompt's required `stopReason`, or, when
/// it carries nothing but a list of options (and `_meta`), read as a
/// snapshot of them. ACP's untagged `AgentResponse` would otherwise let the
/// all-optional `AuthenticateResponse` absorb either.
fn read_answer(method: Option<&str>, payload: serde_json::Value) -> serde_json::Result<Answer> {
    use acp::v1::AgentResponse;
    let named = method.and_then(|method| {
        serde_json::from_value::<ResponseMethod>(serde_json::Value::from(method)).ok()
    });
    match named {
        Some(ResponseMethod::SetConfigOption) => {
            return serde_json::from_value(payload).map(|response| {
                Answer::Typed(AgentResponse::SetSessionConfigOptionResponse(response))
            });
        }
        Some(ResponseMethod::Load) => {
            return serde_json::from_value(payload)
                .map(|response| Answer::Typed(AgentResponse::LoadSessionResponse(response)));
        }
        Some(ResponseMethod::Resume) => {
            return serde_json::from_value(payload)
                .map(|response| Answer::Typed(AgentResponse::ResumeSessionResponse(response)));
        }
        None => {}
    }
    if payload.get("stopReason").is_some() {
        return serde_json::from_value(payload)
            .map(|response| Answer::Typed(AgentResponse::PromptResponse(response)));
    }
    let config_only = payload.as_object().is_some_and(|fields| {
        fields
            .get("configOptions")
            .is_some_and(serde_json::Value::is_array)
            && fields
                .keys()
                .all(|key| key == "configOptions" || key == "_meta")
    });
    if method.is_none() && config_only {
        let config_options = serde_json::from_value(payload["configOptions"].clone())?;
        let meta = match payload.get("_meta") {
            None | Some(serde_json::Value::Null) => None,
            Some(meta) => Some(serde_json::from_value(meta.clone())?),
        };
        return Ok(Answer::Snapshot {
            config_options,
            meta,
        });
    }
    serde_json::from_value(payload).map(Answer::Typed)
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
    fn a_config_option_answer_keeps_its_options_across_the_wire() {
        let options = vec![
            acp::v1::SessionConfigOption::select(
                "model",
                "Model",
                "synthetic-fast",
                vec![acp::v1::SessionConfigSelectOption::new(
                    "synthetic-fast",
                    "Synthetic Fast",
                )],
            )
            .category(acp::v1::SessionConfigOptionCategory::Model),
        ];
        let sent = AcpEvent::Response {
            request_id: RequestId::Str("synthetic-set".into()),
            response: Ok(acp::v1::AgentResponse::SetSessionConfigOptionResponse(
                acp::v1::SetSessionConfigOptionResponse::new(options.clone()),
            )),
        };
        let received = round_trip(&sent);
        assert!(
            matches!(
                &received,
                AcpEvent::Response {
                    response: Ok(acp::v1::AgentResponse::SetSessionConfigOptionResponse(response)),
                    ..
                } if response.config_options == options
            ),
            "{received:?}"
        );
    }

    #[test]
    fn a_new_session_answer_with_options_is_not_taken_for_a_config_answer() {
        let sent = AcpEvent::Response {
            request_id: RequestId::Str("synthetic-new".into()),
            response: Ok(acp::v1::AgentResponse::NewSessionResponse(
                acp::v1::NewSessionResponse::new("synthetic-session").config_options(vec![]),
            )),
        };
        let received = round_trip(&sent);
        assert!(
            !matches!(
                &received,
                AcpEvent::Response {
                    response: Ok(acp::v1::AgentResponse::SetSessionConfigOptionResponse(_)),
                    ..
                }
            ),
            "{received:?}"
        );
    }

    fn synthetic_option(current: &str) -> acp::v1::SessionConfigOption {
        acp::v1::SessionConfigOption::select(
            "model",
            "Model",
            current.to_string(),
            vec![acp::v1::SessionConfigSelectOption::new(
                current.to_string(),
                "Synthetic",
            )],
        )
        .category(acp::v1::SessionConfigOptionCategory::Model)
    }

    /// Load, resume and set answers that carry nothing but options, with
    /// zero or one option and with or without `_meta`.
    fn config_only_answers() -> Vec<(&'static str, acp::v1::AgentResponse)> {
        let mut answers = Vec::new();
        for options in [vec![], vec![synthetic_option("synthetic-swift")]] {
            for meta in [
                None,
                Some(serde_json::Map::from_iter([(
                    "synthetic".to_string(),
                    serde_json::json!(true),
                )])),
            ] {
                answers.push((
                    "load",
                    acp::v1::AgentResponse::LoadSessionResponse(
                        acp::v1::LoadSessionResponse::new()
                            .config_options(options.clone())
                            .meta(meta.clone()),
                    ),
                ));
                answers.push((
                    "resume",
                    acp::v1::AgentResponse::ResumeSessionResponse(
                        acp::v1::ResumeSessionResponse::new()
                            .config_options(options.clone())
                            .meta(meta.clone()),
                    ),
                ));
                answers.push((
                    "set",
                    acp::v1::AgentResponse::SetSessionConfigOptionResponse(
                        acp::v1::SetSessionConfigOptionResponse::new(options.clone())
                            .meta(meta.clone()),
                    ),
                ));
            }
        }
        answers
    }

    fn kind(event: &AcpEvent) -> &'static str {
        match event {
            AcpEvent::Response {
                response: Ok(acp::v1::AgentResponse::LoadSessionResponse(_)),
                ..
            } => "load",
            AcpEvent::Response {
                response: Ok(acp::v1::AgentResponse::ResumeSessionResponse(_)),
                ..
            } => "resume",
            AcpEvent::Response {
                response: Ok(acp::v1::AgentResponse::SetSessionConfigOptionResponse(_)),
                ..
            } => "set",
            _ => "other",
        }
    }

    #[test]
    fn load_resume_and_set_answers_keep_their_request_across_the_wire() {
        for (sent_kind, response) in config_only_answers() {
            let sent = AcpEvent::Response {
                request_id: RequestId::Number(7),
                response: Ok(response),
            };
            let wire = serde_json::to_value(&sent).unwrap();
            assert_eq!(kind(&round_trip(&sent)), sent_kind, "{wire}");
        }
    }

    /// A journal written before answers named their request.
    fn untagged(result: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "message_type": "response",
            "request_id": 7,
            "response": {"Ok": result},
        })
    }

    #[test]
    fn an_untagged_config_payload_is_never_read_as_a_write() {
        for result in [
            serde_json::json!({"configOptions": []}),
            serde_json::json!({"configOptions": [synthetic_option("synthetic-swift")]}),
            serde_json::json!({"configOptions": null}),
            serde_json::json!({}),
        ] {
            let received: AcpEvent = serde_json::from_value(untagged(result.clone())).unwrap();
            assert_ne!(kind(&received), "set", "{result}");
        }
    }

    #[test]
    fn an_untagged_list_of_options_reads_as_a_snapshot_and_writes_back_unchanged() {
        for result in [
            serde_json::json!({"configOptions": []}),
            serde_json::json!({"configOptions": [synthetic_option("synthetic-swift")]}),
            serde_json::json!({"configOptions": [], "_meta": {"synthetic": true}}),
            serde_json::json!({"configOptions": [], "_meta": null}),
        ] {
            let record = untagged(result.clone());
            let received: AcpEvent = serde_json::from_value(record.clone()).unwrap();
            let AcpEvent::ConfigSnapshot { config_options, .. } = &received else {
                panic!("{result} is a snapshot: {received:?}");
            };
            assert_eq!(
                serde_json::to_value(config_options).unwrap(),
                result["configOptions"]
            );
            if result.get("_meta") != Some(&serde_json::Value::Null) {
                assert_eq!(serde_json::to_value(&received).unwrap(), record);
            }
        }
    }

    #[test]
    fn untagged_answers_without_a_list_of_options_are_not_snapshots() {
        for result in [
            serde_json::json!({"configOptions": null}),
            serde_json::json!({}),
            serde_json::json!({"configOptions": [], "modes": null}),
        ] {
            let received: AcpEvent = serde_json::from_value(untagged(result.clone())).unwrap();
            assert!(
                matches!(
                    received,
                    AcpEvent::Response {
                        response: Ok(_),
                        ..
                    }
                ),
                "{result}: {received:?}"
            );
        }
    }

    #[test]
    fn an_answer_naming_an_unknown_method_reads_as_an_untagged_one() {
        let mut record = untagged(serde_json::json!({"stopReason": "end_turn"}));
        record["method"] = serde_json::json!("session/synthetic");
        let received: AcpEvent = serde_json::from_value(record).unwrap();
        assert!(matches!(
            received,
            AcpEvent::Response {
                response: Ok(acp::v1::AgentResponse::PromptResponse(_)),
                ..
            }
        ));
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
