//! ACP Connection management
//!
//! Provides `AcpConnection` for communicating with ACP agents over the
//! Agent Client Protocol via stdin/stdout (local subprocess via `spawn()`).

use nori_protocol::AcpEvent;
use nori_protocol::acp::v1 as acp;
use tokio::sync::oneshot;

pub mod acp_connection;
mod child_lifecycle;
pub mod mcp;
mod wire_log;

#[cfg(test)]
mod acp_connection_tests;

/// Raw events emitted by the ACP transport adapter in source order.
#[derive(Debug)]
pub enum ConnectionEvent {
    /// Raw ACP traffic retained for the public harness boundary.
    Acp(Box<AcpEvent>),
    /// The active session was released successfully through ACP `session/close`.
    SessionClosed,
    /// Private reducer input paired with the preceding raw notification.
    SessionUpdate(acp::SessionUpdate),
    /// A broker-projected terminal boundary for a turn owned by another client.
    ObservedTurnEnd {
        session_id: acp::SessionId,
        stop_reason: String,
    },
    DelegatedRequest(DelegatedRequest),
    /// The agent subprocess exited on its own. `status` is the exit code
    /// (`None` when killed by a signal); `stderr_tail` carries the child's
    /// most recent stderr output for error reporting.
    ChildExited {
        status: Option<i32>,
        stderr_tail: String,
    },
}

pub fn session_update_kind(update: &acp::SessionUpdate) -> &'static str {
    match update {
        acp::SessionUpdate::AgentMessageChunk(_) => "agent_message_chunk",
        acp::SessionUpdate::AgentThoughtChunk(_) => "agent_thought_chunk",
        acp::SessionUpdate::UserMessageChunk(_) => "user_message_chunk",
        acp::SessionUpdate::Plan(_) => "plan",
        acp::SessionUpdate::ToolCall(_) => "tool_call",
        acp::SessionUpdate::ToolCallUpdate(_) => "tool_call_update",
        acp::SessionUpdate::AvailableCommandsUpdate(_) => "available_commands_update",
        acp::SessionUpdate::CurrentModeUpdate(_) => "current_mode_update",
        acp::SessionUpdate::ConfigOptionUpdate(_) => "config_option_update",
        acp::SessionUpdate::SessionInfoUpdate(_) => "session_info_update",
        acp::SessionUpdate::UsageUpdate(_) => "usage_update",
        _ => "other",
    }
}

/// A schema-native agent request paired with its transport responder.
#[derive(Debug)]
pub struct DelegatedRequest {
    pub request_id: acp::RequestId,
    pub request: acp::AgentRequest,
    pub response_tx: oneshot::Sender<Result<acp::ClientResponse, acp::Error>>,
}

/// Session config state captured from ACP session setup and updates.
///
/// This stores the complete current `configOptions` snapshot for the active
/// session. ACP responses and notifications replace the full list.
#[derive(Debug, Clone, Default)]
pub(crate) struct AcpSessionConfigState {
    pub config_options: Vec<acp::SessionConfigOption>,
}

impl AcpSessionConfigState {
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_update_kind_labels_usage_update() {
        let update = acp::SessionUpdate::UsageUpdate(acp::UsageUpdate::new(12, 100));
        assert_eq!(session_update_kind(&update), "usage_update");
    }
}
