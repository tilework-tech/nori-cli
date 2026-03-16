//! ACP Connection management
//!
//! Provides `SacpConnection` for spawning and communicating with ACP agent
//! subprocesses using the SACP v10 protocol over stdin/stdout.

use codex_protocol::approvals::ApplyPatchApprovalRequestEvent;
use codex_protocol::approvals::ExecApprovalRequestEvent;
use codex_protocol::protocol::ReviewDecision;
use sacp::schema as acp;
use tokio::sync::oneshot;

pub mod sacp_connection;

#[cfg(test)]
mod sacp_connection_tests;

/// The type of approval event to send to the UI.
///
/// This enum allows us to use the more appropriate approval UI for different
/// operation types - exec approval for shell commands, patch approval for
/// file edits/writes/deletes.
#[derive(Debug)]
pub enum ApprovalEventType {
    /// Exec approval for shell commands and other operations
    Exec(ExecApprovalRequestEvent),
    /// Patch approval for file edit/write/delete operations
    Patch(ApplyPatchApprovalRequestEvent),
}

impl ApprovalEventType {
    /// Get the call_id from the event
    pub fn call_id(&self) -> &str {
        match self {
            ApprovalEventType::Exec(e) => &e.call_id,
            ApprovalEventType::Patch(e) => &e.call_id,
        }
    }
}

/// An approval request sent from the ACP layer to the UI layer.
///
/// When an ACP agent requests permission to perform an operation,
/// this struct is sent to the UI layer which should display the request
/// to the user and return their decision via the response channel.
#[derive(Debug)]
pub struct ApprovalRequest {
    /// The translated Codex approval event (either exec or patch)
    pub event: ApprovalEventType,
    /// The original ACP permission options for translating the response
    pub options: Vec<acp::PermissionOption>,
    /// Channel to send the user's decision back
    pub response_tx: oneshot::Sender<ReviewDecision>,
}

/// Model state captured from the ACP session.
///
/// This is populated when a session is created (from `NewSessionResponse`)
/// and can be updated when the model is changed.
#[derive(Debug, Clone, Default)]
pub struct AcpModelState {
    /// The ID of the currently active model
    pub current_model_id: Option<acp::ModelId>,
    /// List of available models from the agent
    pub available_models: Vec<acp::ModelInfo>,
}

impl AcpModelState {
    /// Create a new empty model state
    pub fn new() -> Self {
        Self::default()
    }

    /// Update from an ACP SessionModelState
    #[cfg(feature = "unstable")]
    pub fn from_session_model_state(state: &acp::SessionModelState) -> Self {
        Self {
            current_model_id: Some(state.current_model_id.clone()),
            available_models: state.available_models.clone(),
        }
    }
}
