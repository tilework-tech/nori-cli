//! ACP-faithful session runtime types.
//!
//! These types implement the turn-state model defined in the ACP Turn State
//! spec. `SessionRuntime` is the single source of truth for whether a session
//! is idle, loading, or processing a prompt.

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

use agent_client_protocol_schema as acp;
use serde::Deserialize;
use serde::Serialize;

use crate::AgentCommandInfo;
use crate::PlanSnapshot;
use crate::ToolSnapshot;

// ---------------------------------------------------------------------------
// Session phase
// ---------------------------------------------------------------------------

/// The phase of the ACP session. This is the single source of truth for
/// whether a prompt is active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionPhase {
    /// No ACP request currently owns streamed content.
    Idle,
    /// `session/load` owns replay content until its response arrives.
    Loading { request_id: String },
    /// `session/prompt` owns turn content until its response arrives.
    /// `cancelling` means `session/cancel` has been sent but the response
    /// has not yet arrived.
    Prompt {
        request_id: String,
        cancelling: bool,
    },
}

/// Flattened view of session phase for TUI consumption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhaseView {
    Idle,
    Loading,
    Prompt,
    Cancelling,
}

impl From<&SessionPhase> for SessionPhaseView {
    fn from(phase: &SessionPhase) -> Self {
        match phase {
            SessionPhase::Idle => Self::Idle,
            SessionPhase::Loading { .. } => Self::Loading,
            SessionPhase::Prompt {
                cancelling: true, ..
            } => Self::Cancelling,
            SessionPhase::Prompt {
                cancelling: false, ..
            } => Self::Prompt,
        }
    }
}

// ---------------------------------------------------------------------------
// Active request state
// ---------------------------------------------------------------------------

/// Whether the active request is a load or a prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveRequestKind {
    Loading,
    Prompt,
}

/// In-flight request state. Exists only while a `session/prompt` or
/// `session/load` is active. Cleared when the response arrives.
#[derive(Debug, Clone)]
pub struct ActiveRequestState {
    pub request_id: String,
    pub kind: ActiveRequestKind,
    pub prompt: Option<QueuedPrompt>,
    pub open_agent_message: Option<OpenMessage>,
    pub open_thought_message: Option<OpenMessage>,
    pub open_user_message: Option<OpenMessage>,
    /// Tool call IDs created during this request, in insertion order.
    pub tool_call_ids: Vec<String>,
    /// Permission request IDs pending for this request.
    pub pending_permission_requests: HashSet<String>,
}

impl ActiveRequestState {
    pub fn new(request_id: String, kind: ActiveRequestKind) -> Self {
        Self {
            request_id,
            kind,
            prompt: None,
            open_agent_message: None,
            open_thought_message: None,
            open_user_message: None,
            tool_call_ids: Vec::new(),
            pending_permission_requests: HashSet::new(),
        }
    }

    pub fn new_prompt(request_id: String, prompt: QueuedPrompt) -> Self {
        Self {
            prompt: Some(prompt),
            ..Self::new(request_id, ActiveRequestKind::Prompt)
        }
    }
}

// ---------------------------------------------------------------------------
// Open message buffer
// ---------------------------------------------------------------------------

/// A message being assembled from chunks during an active request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenMessage {
    pub message_id: Option<String>,
    pub chunks: Vec<String>,
}

impl OpenMessage {
    pub fn new() -> Self {
        Self {
            message_id: None,
            chunks: Vec::new(),
        }
    }

    /// Concatenate all chunks into a single string.
    pub fn text(&self) -> String {
        self.chunks.concat()
    }
}

impl Default for OpenMessage {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Persisted session state
// ---------------------------------------------------------------------------

/// Long-lived session state that survives across request boundaries.
#[derive(Debug, Clone, Default)]
pub struct PersistedSessionState {
    pub transcript: Vec<TranscriptMessage>,
    pub plan: Option<PlanSnapshot>,
    pub tool_calls: HashMap<String, ToolSnapshot>,
    pub available_commands: Vec<AgentCommandInfo>,
    pub current_mode: Option<String>,
    pub config_options: Vec<acp::SessionConfigOption>,
    pub session_info: SessionInfoState,
    pub session_usage: Option<SessionUsageState>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionInfoState {
    pub title: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionUsageState {
    pub used_tokens: i64,
    pub total_tokens: i64,
    pub cost_display: Option<String>,
}

/// A finalized message in the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptMessage {
    pub role: TranscriptRole,
    pub content: String,
}

/// Role of a transcript message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptRole {
    User,
    Agent,
    Thought,
}

// ---------------------------------------------------------------------------
// Outgoing queue
// ---------------------------------------------------------------------------

/// A user prompt waiting to be sent to ACP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuedPromptKind {
    User,
    Compact,
}

/// A user prompt waiting to be sent to ACP.
#[derive(Debug, Clone)]
pub struct QueuedPrompt {
    pub event_id: String,
    pub kind: QueuedPromptKind,
    pub text: String,
    pub display_text: Option<String>,
    pub images: Vec<acp::ContentBlock>,
}

// ---------------------------------------------------------------------------
// Session runtime
// ---------------------------------------------------------------------------

/// The single runtime object per ACP session. Everything else is derived
/// from this.
#[derive(Debug, Clone)]
pub struct SessionRuntime {
    pub phase: SessionPhase,
    pub persisted: PersistedSessionState,
    pub active: Option<ActiveRequestState>,
    pub queue: VecDeque<QueuedPrompt>,
}

impl SessionRuntime {
    pub fn new() -> Self {
        Self {
            phase: SessionPhase::Idle,
            persisted: PersistedSessionState::default(),
            active: None,
            queue: VecDeque::new(),
        }
    }

    /// Flattened view of the current phase.
    pub fn phase_view(&self) -> SessionPhaseView {
        SessionPhaseView::from(&self.phase)
    }
}

impl Default for SessionRuntime {
    fn default() -> Self {
        Self::new()
    }
}
