//! Backend adapter for ACP agents in the TUI
//!
//! This module provides `AcpBackend`, which adapts the ACP connection interface
//! to be compatible with the TUI's event-driven architecture. It translates
//! between Codex `Op` submissions and ACP protocol calls, and converts ACP
//! session updates into `codex_protocol::Event` for the TUI.

use std::path::PathBuf;
use std::sync::Arc;

use agent_client_protocol as acp;
use anyhow::Result;
use codex_protocol::ConversationId;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::ErrorEvent;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::protocol::SessionConfiguredEvent;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::protocol::TurnAbortedEvent;
use codex_protocol::user_input::UserInput;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tracing::debug;
use tracing::warn;

use crate::connection::AcpConnection;
use crate::connection::ApprovalRequest;
use crate::registry::get_agent_config;
use crate::translator;

/// Configuration for spawning an ACP backend.
///
/// This contains the subset of Codex configuration needed for ACP mode,
/// avoiding a direct dependency on codex_core.
#[derive(Debug, Clone)]
pub struct AcpBackendConfig {
    /// Model name used to look up agent in registry
    pub model: String,
    /// Working directory for the session
    pub cwd: PathBuf,
    /// Approval policy for command execution
    pub approval_policy: AskForApproval,
    /// Sandbox policy for command execution
    pub sandbox_policy: SandboxPolicy,
}

/// Backend adapter that provides a TUI-compatible interface for ACP agents.
///
/// This struct wraps an `AcpConnection` and translates between:
/// - Codex `Op` submissions → ACP protocol calls
/// - ACP `SessionUpdate` events → `codex_protocol::Event`
pub struct AcpBackend {
    connection: Arc<AcpConnection>,
    session_id: acp::SessionId,
    event_tx: mpsc::Sender<Event>,
    #[allow(dead_code)]
    cwd: PathBuf,
    /// Pending approval requests waiting for user decision
    pending_approvals: Arc<Mutex<Vec<ApprovalRequest>>>,
}

impl AcpBackend {
    /// Spawn an ACP backend for the given configuration.
    ///
    /// This will:
    /// 1. Look up the agent config from the registry
    /// 2. Spawn the ACP connection
    /// 3. Create a session
    /// 4. Send a synthetic `SessionConfigured` event
    /// 5. Start background tasks for event translation and approval handling
    ///
    /// # Arguments
    /// * `config` - The ACP backend configuration
    /// * `event_tx` - Channel to send translated events to the TUI
    ///
    /// # Returns
    /// A connected `AcpBackend` ready to receive operations.
    pub async fn spawn(config: &AcpBackendConfig, event_tx: mpsc::Sender<Event>) -> Result<Self> {
        let agent_config = get_agent_config(&config.model)?;
        let cwd = config.cwd.clone();

        debug!("Spawning ACP backend for model: {}", config.model);

        // Spawn the ACP connection
        let mut connection = AcpConnection::spawn(&agent_config, &cwd).await?;

        // Create a session
        let session_id = connection.create_session(&cwd).await?;

        debug!("ACP session created: {:?}", session_id);

        // Take the approval receiver for handling permission requests
        let approval_rx = connection.take_approval_receiver();

        let connection = Arc::new(connection);
        let pending_approvals = Arc::new(Mutex::new(Vec::new()));

        let backend = Self {
            connection,
            session_id,
            event_tx: event_tx.clone(),
            cwd: cwd.clone(),
            pending_approvals: Arc::clone(&pending_approvals),
        };

        // Send synthetic SessionConfigured event
        let session_configured = SessionConfiguredEvent {
            session_id: ConversationId::new(),
            model: config.model.clone(),
            model_provider_id: "acp".to_string(),
            approval_policy: config.approval_policy,
            sandbox_policy: config.sandbox_policy.clone(),
            cwd: cwd.clone(),
            reasoning_effort: None,
            history_log_id: 0,
            history_entry_count: 0,
            initial_messages: None,
            rollout_path: cwd.join(".codex-rollout.jsonl"),
        };

        event_tx
            .send(Event {
                id: String::new(),
                msg: EventMsg::SessionConfigured(session_configured),
            })
            .await
            .ok();

        // Spawn approval handler task
        tokio::spawn(Self::run_approval_handler(
            approval_rx,
            event_tx.clone(),
            Arc::clone(&pending_approvals),
        ));

        Ok(backend)
    }

    /// Submit an operation to the ACP backend.
    ///
    /// Translates Codex `Op` variants to appropriate ACP actions:
    /// - `Op::UserInput` → ACP prompt
    /// - `Op::Interrupt` → ACP cancel
    /// - `Op::ExecApproval` → Resolve pending approval
    /// - Other ops → Send error event (not supported)
    pub async fn submit(&self, op: Op) -> Result<String> {
        let id = generate_id();

        match op {
            Op::UserInput { items } => {
                self.handle_user_input(items, &id).await?;
            }
            Op::Interrupt => {
                self.connection.cancel(&self.session_id).await?;
                // Send TurnAborted event to notify the TUI that the turn was interrupted
                let _ = self
                    .event_tx
                    .send(Event {
                        id: id.clone(),
                        msg: EventMsg::TurnAborted(TurnAbortedEvent {
                            reason: TurnAbortReason::Interrupted,
                        }),
                    })
                    .await;
            }
            Op::ExecApproval {
                id: call_id,
                decision,
            } => {
                self.handle_exec_approval(&call_id, decision).await;
            }
            Op::PatchApproval {
                id: call_id,
                decision,
            } => {
                self.handle_exec_approval(&call_id, decision).await;
            }
            // Unsupported operations - send error event per user decision
            Op::Compact
            | Op::Undo
            | Op::GetHistoryEntryRequest { .. }
            | Op::AddToHistory { .. }
            | Op::ListMcpTools
            | Op::ListCustomPrompts
            | Op::Review { .. }
            | Op::RunUserShellCommand { .. } => {
                let op_name = get_op_name(&op);
                warn!("Unsupported Op in ACP mode: {op_name}");
                self.send_error(&format!(
                    "Operation '{op_name}' is not supported in ACP mode"
                ))
                .await;
            }
            // These ops are internal/context-related, silently ignore
            Op::UserTurn { .. }
            | Op::OverrideTurnContext { .. }
            | Op::ResolveElicitation { .. }
            | Op::Shutdown => {
                debug!("Ignoring internal Op in ACP mode: {}", get_op_name(&op));
            }
            // Catch any new Op variants we haven't handled
            _ => {
                let op_name = get_op_name(&op);
                warn!("Unknown Op in ACP mode: {op_name}");
                self.send_error(&format!(
                    "Operation '{op_name}' is not supported in ACP mode"
                ))
                .await;
            }
        }

        Ok(id)
    }

    /// Handle user input by sending a prompt to the ACP agent.
    async fn handle_user_input(&self, items: Vec<UserInput>, id: &str) -> Result<()> {
        // Extract text from user input items
        let mut prompt_text = String::new();
        for item in items {
            match item {
                UserInput::Text { text } => {
                    if !prompt_text.is_empty() {
                        prompt_text.push('\n');
                    }
                    prompt_text.push_str(&text);
                }
                UserInput::Image { .. } | UserInput::LocalImage { .. } => {
                    // Images not yet supported in ACP mode
                    warn!("Image input not supported in ACP mode");
                }
                // Handle any future UserInput variants
                _ => {
                    warn!("Unknown UserInput variant in ACP mode");
                }
            }
        }

        if prompt_text.is_empty() {
            return Ok(());
        }

        let prompt = vec![translator::text_to_content_block(&prompt_text)];

        // Create channel for receiving session updates
        let (update_tx, mut update_rx) = mpsc::channel(32);

        // Clone what we need for the background task
        let event_tx = self.event_tx.clone();
        let session_id = self.session_id.clone();
        let connection = Arc::clone(&self.connection);
        let id_clone = id.to_string();

        // Spawn task to handle the prompt and translate events
        tokio::spawn(async move {
            // Send TaskStarted event
            let _ = event_tx
                .send(Event {
                    id: id_clone.clone(),
                    msg: EventMsg::TaskStarted(codex_protocol::protocol::TaskStartedEvent {
                        model_context_window: None,
                    }),
                })
                .await;

            // Spawn update consumer task
            let event_tx_clone = event_tx.clone();
            let id_for_updates = id_clone.clone();
            let update_handler = tokio::spawn(async move {
                while let Some(update) = update_rx.recv().await {
                    let events = translate_session_update_to_events(&update);
                    for event_msg in events {
                        let _ = event_tx_clone
                            .send(Event {
                                id: id_for_updates.clone(),
                                msg: event_msg,
                            })
                            .await;
                    }
                }
            });

            // Send the prompt
            let result = connection.prompt(session_id, prompt, update_tx).await;

            // Wait for all updates to be processed
            let _ = update_handler.await;

            // Send TaskComplete event
            let _ = event_tx
                .send(Event {
                    id: id_clone,
                    msg: EventMsg::TaskComplete(codex_protocol::protocol::TaskCompleteEvent {
                        last_agent_message: None,
                    }),
                })
                .await;

            if let Err(e) = result {
                warn!("ACP prompt failed: {}", e);
            }
        });

        Ok(())
    }

    /// Handle an exec approval decision by finding and resolving the pending approval.
    async fn handle_exec_approval(&self, call_id: &str, decision: ReviewDecision) {
        let mut pending = self.pending_approvals.lock().await;
        if let Some(pos) = pending.iter().position(|r| r.event.call_id == call_id) {
            let request = pending.remove(pos);
            let _ = request.response_tx.send(decision);
        } else {
            warn!("No pending approval found for call_id: {}", call_id);
        }
    }

    /// Send an error event to the TUI.
    async fn send_error(&self, message: &str) {
        let _ = self
            .event_tx
            .send(Event {
                id: String::new(),
                msg: EventMsg::Error(ErrorEvent {
                    message: message.to_string(),
                    codex_error_info: None,
                }),
            })
            .await;
    }

    /// Background task to handle approval requests from the ACP connection.
    async fn run_approval_handler(
        mut approval_rx: mpsc::Receiver<ApprovalRequest>,
        event_tx: mpsc::Sender<Event>,
        pending_approvals: Arc<Mutex<Vec<ApprovalRequest>>>,
    ) {
        while let Some(request) = approval_rx.recv().await {
            // Send ExecApprovalRequest event to TUI.
            // Use the call_id as the event wrapper ID so that the TUI can
            // correctly route the user's decision back to this pending request.
            let _ = event_tx
                .send(Event {
                    id: request.event.call_id.clone(),
                    msg: EventMsg::ExecApprovalRequest(request.event.clone()),
                })
                .await;

            // Store the pending approval for later resolution
            pending_approvals.lock().await.push(request);
        }
    }
}

/// Generate a unique ID for operations
fn generate_id() -> String {
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!("acp-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Get a human-readable name for an Op variant
fn get_op_name(op: &Op) -> &'static str {
    match op {
        Op::Interrupt => "Interrupt",
        Op::UserInput { .. } => "UserInput",
        Op::UserTurn { .. } => "UserTurn",
        Op::OverrideTurnContext { .. } => "OverrideTurnContext",
        Op::ExecApproval { .. } => "ExecApproval",
        Op::PatchApproval { .. } => "PatchApproval",
        Op::ResolveElicitation { .. } => "ResolveElicitation",
        Op::AddToHistory { .. } => "AddToHistory",
        Op::GetHistoryEntryRequest { .. } => "GetHistoryEntryRequest",
        Op::ListMcpTools => "ListMcpTools",
        Op::ListCustomPrompts => "ListCustomPrompts",
        Op::Compact => "Compact",
        Op::Undo => "Undo",
        Op::Review { .. } => "Review",
        Op::Shutdown => "Shutdown",
        Op::RunUserShellCommand { .. } => "RunUserShellCommand",
        _ => "Unknown",
    }
}

/// Translate an ACP SessionUpdate to codex_protocol::EventMsg variants.
fn translate_session_update_to_events(update: &acp::SessionUpdate) -> Vec<EventMsg> {
    match update {
        acp::SessionUpdate::AgentMessageChunk(chunk) => {
            if let acp::ContentBlock::Text(text) = &chunk.content {
                vec![EventMsg::AgentMessageDelta(
                    codex_protocol::protocol::AgentMessageDeltaEvent {
                        delta: text.text.clone(),
                    },
                )]
            } else {
                vec![]
            }
        }
        acp::SessionUpdate::AgentThoughtChunk(chunk) => {
            if let acp::ContentBlock::Text(text) = &chunk.content {
                vec![EventMsg::AgentReasoningDelta(
                    codex_protocol::protocol::AgentReasoningDeltaEvent {
                        delta: text.text.clone(),
                    },
                )]
            } else {
                vec![]
            }
        }
        acp::SessionUpdate::ToolCall(tool_call) => {
            // Tool calls can be mapped to ExecCommandBegin events
            vec![EventMsg::ExecCommandBegin(
                codex_protocol::protocol::ExecCommandBeginEvent {
                    call_id: tool_call.id.to_string(),
                    process_id: None,
                    turn_id: String::new(),
                    command: vec![tool_call.title.clone()],
                    cwd: PathBuf::new(),
                    parsed_cmd: vec![],
                    source: codex_protocol::protocol::ExecCommandSource::Agent,
                    interaction_input: None,
                },
            )]
        }
        acp::SessionUpdate::ToolCallUpdate(update) => {
            // Tool call updates can be mapped based on status
            if update.fields.status == Some(acp::ToolCallStatus::Completed) {
                vec![EventMsg::ExecCommandEnd(
                    codex_protocol::protocol::ExecCommandEndEvent {
                        call_id: update.id.to_string(),
                        process_id: None,
                        turn_id: String::new(),
                        command: vec![update.fields.title.clone().unwrap_or_default()],
                        cwd: PathBuf::new(),
                        parsed_cmd: vec![],
                        source: codex_protocol::protocol::ExecCommandSource::Agent,
                        interaction_input: None,
                        stdout: String::new(),
                        stderr: String::new(),
                        aggregated_output: String::new(),
                        exit_code: 0,
                        duration: std::time::Duration::ZERO,
                        formatted_output: String::new(),
                    },
                )]
            } else {
                vec![]
            }
        }
        // Other update types don't have direct event mappings
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that translate_session_update_to_events correctly translates
    /// AgentMessageChunk to AgentMessageDelta events.
    #[test]
    fn test_translate_agent_message_chunk_to_event() {
        let update = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk {
            content: acp::ContentBlock::Text(acp::TextContent {
                text: "Hello from agent".to_string(),
                annotations: None,
                meta: None,
            }),
            meta: None,
        });

        let events = translate_session_update_to_events(&update);
        assert_eq!(events.len(), 1);

        match &events[0] {
            EventMsg::AgentMessageDelta(delta) => {
                assert_eq!(delta.delta, "Hello from agent");
            }
            _ => panic!("Expected AgentMessageDelta event"),
        }
    }

    /// Test that translate_session_update_to_events correctly translates
    /// AgentThoughtChunk to AgentReasoningDelta events.
    #[test]
    fn test_translate_agent_thought_to_reasoning_event() {
        let update = acp::SessionUpdate::AgentThoughtChunk(acp::ContentChunk {
            content: acp::ContentBlock::Text(acp::TextContent {
                text: "Thinking about the problem...".to_string(),
                annotations: None,
                meta: None,
            }),
            meta: None,
        });

        let events = translate_session_update_to_events(&update);
        assert_eq!(events.len(), 1);

        match &events[0] {
            EventMsg::AgentReasoningDelta(delta) => {
                assert_eq!(delta.delta, "Thinking about the problem...");
            }
            _ => panic!("Expected AgentReasoningDelta event"),
        }
    }

    /// Test that ToolCall updates are translated to ExecCommandBegin events.
    #[test]
    fn test_translate_tool_call_to_exec_command_begin() {
        let update = acp::SessionUpdate::ToolCall(acp::ToolCall {
            id: acp::ToolCallId::from("call-123".to_string()),
            title: "shell".to_string(),
            kind: acp::ToolKind::Execute,
            status: acp::ToolCallStatus::InProgress,
            content: vec![],
            locations: vec![],
            raw_input: Some(serde_json::json!({"command": "ls -la"})),
            raw_output: None,
            meta: None,
        });

        let events = translate_session_update_to_events(&update);
        assert_eq!(events.len(), 1);

        match &events[0] {
            EventMsg::ExecCommandBegin(begin) => {
                assert_eq!(begin.call_id, "call-123");
                assert!(begin.command.contains(&"shell".to_string()));
            }
            _ => panic!("Expected ExecCommandBegin event"),
        }
    }

    /// Test that completed ToolCallUpdate is translated to ExecCommandEnd.
    #[test]
    fn test_translate_tool_call_update_completed_to_exec_command_end() {
        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate {
            id: acp::ToolCallId::from("call-456".to_string()),
            fields: acp::ToolCallUpdateFields {
                kind: None,
                status: Some(acp::ToolCallStatus::Completed),
                title: Some("read_file".to_string()),
                content: None,
                locations: None,
                raw_input: None,
                raw_output: None,
            },
            meta: None,
        });

        let events = translate_session_update_to_events(&update);
        assert_eq!(events.len(), 1);

        match &events[0] {
            EventMsg::ExecCommandEnd(end) => {
                assert_eq!(end.call_id, "call-456");
            }
            _ => panic!("Expected ExecCommandEnd event"),
        }
    }

    /// Test that non-text content blocks produce no events.
    #[test]
    fn test_non_text_content_produces_no_events() {
        let update = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk {
            content: acp::ContentBlock::Image(acp::ImageContent {
                data: String::new(),
                mime_type: "image/png".to_string(),
                annotations: None,
                uri: None,
                meta: None,
            }),
            meta: None,
        });

        let events = translate_session_update_to_events(&update);
        assert!(events.is_empty());
    }

    /// Test that unsupported session update types produce no events.
    #[test]
    fn test_unsupported_updates_produce_no_events() {
        let update = acp::SessionUpdate::UserMessageChunk(acp::ContentChunk {
            content: acp::ContentBlock::Text(acp::TextContent {
                text: "User message".to_string(),
                annotations: None,
                meta: None,
            }),
            meta: None,
        });

        let events = translate_session_update_to_events(&update);
        assert!(events.is_empty());
    }

    /// Test that get_op_name returns correct names for various Op variants.
    #[test]
    fn test_get_op_name() {
        assert_eq!(get_op_name(&Op::Interrupt), "Interrupt");
        assert_eq!(get_op_name(&Op::Compact), "Compact");
        assert_eq!(get_op_name(&Op::Undo), "Undo");
        assert_eq!(get_op_name(&Op::UserInput { items: vec![] }), "UserInput");
    }

    /// Test that generate_id produces unique IDs.
    #[test]
    fn test_generate_id_unique() {
        let id1 = generate_id();
        let id2 = generate_id();
        let id3 = generate_id();

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert!(id1.starts_with("acp-"));
        assert!(id2.starts_with("acp-"));
    }
}
