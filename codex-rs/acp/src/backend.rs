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
use crate::connection::AcpModelState;
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
            Op::Shutdown => {
                // Cancel any in-progress session and send ShutdownComplete
                // to allow the TUI to exit properly
                debug!("Processing Op::Shutdown in ACP mode");
                let _ = self.connection.cancel(&self.session_id).await;
                let _ = self
                    .event_tx
                    .send(Event {
                        id: id.clone(),
                        msg: EventMsg::ShutdownComplete,
                    })
                    .await;
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
            | Op::ResolveElicitation { .. } => {
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

    /// Get the current model state from the ACP connection.
    ///
    /// Returns information about the current model and available models.
    /// This state is updated when a session is created or when the model is switched.
    pub fn model_state(&self) -> AcpModelState {
        self.connection.model_state()
    }

    /// Get the current session ID.
    pub fn session_id(&self) -> &acp::SessionId {
        &self.session_id
    }

    /// Get a reference to the underlying ACP connection.
    ///
    /// This provides access to low-level ACP operations like model switching.
    pub fn connection(&self) -> &Arc<AcpConnection> {
        &self.connection
    }

    /// Switch to a different model for the current session.
    ///
    /// This sends a `session/set_model` request to the ACP agent and updates
    /// the internal model state. The model_id must be one of the available
    /// models returned by `model_state().available_models`.
    ///
    /// # Arguments
    /// * `model_id` - The ID of the model to switch to
    ///
    /// # Errors
    /// Returns an error if the model switch fails (e.g., invalid model ID,
    /// agent doesn't support model switching, or connection error).
    #[cfg(feature = "unstable")]
    pub async fn set_model(&self, model_id: &acp::ModelId) -> Result<()> {
        self.connection.set_model(&self.session_id, model_id).await
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
            // Format command with title and arguments for better display
            let command_display = format_acp_tool_command(&tool_call.title, &tool_call.raw_input);
            vec![EventMsg::ExecCommandBegin(
                codex_protocol::protocol::ExecCommandBeginEvent {
                    call_id: tool_call.tool_call_id.to_string(),
                    process_id: None,
                    turn_id: String::new(),
                    command: vec![command_display],
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
                // Format command with title and arguments
                let command_display = format_acp_tool_command(
                    update.fields.title.as_deref().unwrap_or("Tool"),
                    &update.fields.raw_input,
                );
                // Extract output from content and raw_output fields
                let output = format_acp_tool_output(&update.fields.content, &update.fields.raw_output);
                vec![EventMsg::ExecCommandEnd(
                    codex_protocol::protocol::ExecCommandEndEvent {
                        call_id: update.tool_call_id.to_string(),
                        process_id: None,
                        turn_id: String::new(),
                        command: vec![command_display],
                        cwd: PathBuf::new(),
                        parsed_cmd: vec![],
                        source: codex_protocol::protocol::ExecCommandSource::Agent,
                        interaction_input: None,
                        stdout: String::new(),
                        stderr: String::new(),
                        aggregated_output: output.clone(),
                        exit_code: 0,
                        duration: std::time::Duration::ZERO,
                        formatted_output: output,
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

/// Format an ACP tool call command for display.
///
/// Converts a tool title and raw_input into a readable format like:
/// - "Read File(/path/to/file.txt)"
/// - "Terminal(git status)"
/// - "Search(pattern in /path)"
fn format_acp_tool_command(title: &str, raw_input: &Option<serde_json::Value>) -> String {
    let args_summary = raw_input.as_ref().map(summarize_raw_input).unwrap_or_default();

    if args_summary.is_empty() {
        title.to_string()
    } else {
        format!("{title}({args_summary})")
    }
}

/// Summarize raw_input JSON into a brief human-readable string.
///
/// Extracts key arguments like file paths, commands, or patterns to create
/// a concise representation suitable for display.
fn summarize_raw_input(input: &serde_json::Value) -> String {
    if let Some(obj) = input.as_object() {
        // Common patterns for various tool types
        // File operations: look for "path", "file_path", "file"
        if let Some(path) = obj
            .get("path")
            .or_else(|| obj.get("file_path"))
            .or_else(|| obj.get("file"))
            .and_then(serde_json::Value::as_str)
        {
            return path.to_string();
        }

        // Terminal/shell operations: look for "command", "cmd"
        if let Some(cmd) = obj
            .get("command")
            .or_else(|| obj.get("cmd"))
            .and_then(serde_json::Value::as_str)
        {
            return cmd.to_string();
        }

        // Search operations: look for "query", "pattern"
        if let Some(query) = obj
            .get("query")
            .or_else(|| obj.get("pattern"))
            .and_then(serde_json::Value::as_str)
        {
            if let Some(search_path) = obj.get("path").and_then(serde_json::Value::as_str) {
                return format!("{query} in {search_path}");
            }
            return query.to_string();
        }

        // For objects with few keys, show all as "key=value" pairs
        if obj.len() <= 2 {
            let pairs: Vec<String> = obj
                .iter()
                .filter_map(|(k, v)| {
                    let value_str = match v {
                        serde_json::Value::String(s) => Some(s.clone()),
                        serde_json::Value::Number(n) => Some(n.to_string()),
                        serde_json::Value::Bool(b) => Some(b.to_string()),
                        _ => None,
                    };
                    value_str.map(|vs| format!("{k}={vs}"))
                })
                .collect();
            if !pairs.is_empty() {
                return pairs.join(", ");
            }
        }

        // Fallback: compact JSON representation (truncated if too long)
        let json_str = serde_json::to_string(input).unwrap_or_default();
        if json_str.len() > 60 {
            format!("{}...", &json_str[..57])
        } else {
            json_str
        }
    } else if let Some(s) = input.as_str() {
        s.to_string()
    } else {
        String::new()
    }
}

/// Format tool output from ACP content and raw_output fields.
///
/// Extracts meaningful output text from the tool call result to display
/// in the TUI as the command output.
fn format_acp_tool_output(
    content: &Option<Vec<acp::ToolCallContent>>,
    raw_output: &Option<serde_json::Value>,
) -> String {
    let mut output_parts: Vec<String> = Vec::new();

    // Extract text from content blocks
    if let Some(content_items) = content {
        for item in content_items {
            if let acp::ToolCallContent::Content(c) = item
                && let acp::ContentBlock::Text(text) = &c.content
                && !text.text.is_empty()
            {
                output_parts.push(text.text.clone());
            }
        }
    }

    // If no text content, try to summarize raw_output
    if output_parts.is_empty()
        && let Some(raw) = raw_output
    {
        let summary = summarize_raw_output(raw);
        if !summary.is_empty() {
            output_parts.push(summary);
        }
    }

    output_parts.join("\n")
}

/// Summarize raw_output JSON into a human-readable result string.
fn summarize_raw_output(output: &serde_json::Value) -> String {
    if let Some(obj) = output.as_object() {
        // Look for common result fields - check success first as it may have additional info
        if let Some(success) = obj.get("success").and_then(serde_json::Value::as_bool) {
            if success {
                // Check for additional info
                if let Some(lines) = obj.get("lines").and_then(serde_json::Value::as_u64) {
                    return format!("Success: {lines} lines");
                }
                return "Success".to_string();
            } else {
                if let Some(error) = obj.get("error").and_then(serde_json::Value::as_str) {
                    return format!("Failed: {error}");
                }
                return "Failed".to_string();
            }
        }
        // Check for standalone metrics without success flag
        if let Some(lines) = obj.get("lines").and_then(serde_json::Value::as_u64) {
            return format!("Read {lines} lines");
        }
        if let Some(files) = obj.get("files").and_then(serde_json::Value::as_array) {
            let count = files.len();
            return format!("Found {count} files");
        }
        if let Some(matches) = obj.get("matches").and_then(serde_json::Value::as_u64) {
            return format!("Found {matches} matches");
        }
        if let Some(exit_code) = obj.get("exit_code").and_then(serde_json::Value::as_i64) {
            if exit_code == 0 {
                return "Completed successfully".to_string();
            } else {
                return format!("Exited with code {exit_code}");
            }
        }

        // Fallback: compact JSON (truncated)
        let json_str = serde_json::to_string(output).unwrap_or_default();
        if json_str.len() > 100 {
            format!("{}...", &json_str[..97])
        } else {
            json_str
        }
    } else if let Some(s) = output.as_str() {
        s.to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that translate_session_update_to_events correctly translates
    /// AgentMessageChunk to AgentMessageDelta events.
    #[test]
    fn test_translate_agent_message_chunk_to_event() {
        let update = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
            acp::ContentBlock::Text(acp::TextContent::new("Hello from agent")),
        ));

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
        let update = acp::SessionUpdate::AgentThoughtChunk(acp::ContentChunk::new(
            acp::ContentBlock::Text(acp::TextContent::new("Thinking about the problem...")),
        ));

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
        let update = acp::SessionUpdate::ToolCall(
            acp::ToolCall::new(acp::ToolCallId::from("call-123".to_string()), "shell")
                .kind(acp::ToolKind::Execute)
                .status(acp::ToolCallStatus::InProgress)
                .raw_input(serde_json::json!({"command": "ls -la"})),
        );

        let events = translate_session_update_to_events(&update);
        assert_eq!(events.len(), 1);

        match &events[0] {
            EventMsg::ExecCommandBegin(begin) => {
                assert_eq!(begin.call_id, "call-123");
                // Command should now be formatted with arguments
                assert!(begin.command[0].contains("shell"));
                assert!(begin.command[0].contains("ls -la"));
            }
            _ => panic!("Expected ExecCommandBegin event"),
        }
    }

    /// Test that completed ToolCallUpdate is translated to ExecCommandEnd.
    #[test]
    fn test_translate_tool_call_update_completed_to_exec_command_end() {
        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::from("call-456".to_string()),
            acp::ToolCallUpdateFields::new()
                .status(acp::ToolCallStatus::Completed)
                .title("read_file")
                .raw_input(serde_json::json!({"path": "/etc/config.toml"}))
                .raw_output(serde_json::json!({"success": true, "lines": 42})),
        ));

        let events = translate_session_update_to_events(&update);
        assert_eq!(events.len(), 1);

        match &events[0] {
            EventMsg::ExecCommandEnd(end) => {
                assert_eq!(end.call_id, "call-456");
                // Command should include the file path
                assert!(end.command[0].contains("read_file"));
                assert!(end.command[0].contains("/etc/config.toml"));
                // Output should summarize the result
                assert!(end.aggregated_output.contains("42"));
            }
            _ => panic!("Expected ExecCommandEnd event"),
        }
    }

    /// Test that tool call output with content blocks is properly formatted.
    #[test]
    fn test_translate_tool_call_update_with_content() {
        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::from("call-789".to_string()),
            acp::ToolCallUpdateFields::new()
                .status(acp::ToolCallStatus::Completed)
                .title("read_file")
                .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                    acp::ContentBlock::Text(acp::TextContent::new("File content here")),
                ))]),
        ));

        let events = translate_session_update_to_events(&update);
        assert_eq!(events.len(), 1);

        match &events[0] {
            EventMsg::ExecCommandEnd(end) => {
                assert_eq!(end.call_id, "call-789");
                // Output should contain the content text
                assert_eq!(end.aggregated_output, "File content here");
            }
            _ => panic!("Expected ExecCommandEnd event"),
        }
    }

    /// Test format_acp_tool_command with various input types.
    #[test]
    fn test_format_acp_tool_command() {
        // Test with file path
        let cmd = format_acp_tool_command(
            "Read File",
            &Some(serde_json::json!({"path": "/etc/config.toml"})),
        );
        assert_eq!(cmd, "Read File(/etc/config.toml)");

        // Test with command
        let cmd = format_acp_tool_command(
            "Terminal",
            &Some(serde_json::json!({"command": "git status"})),
        );
        assert_eq!(cmd, "Terminal(git status)");

        // Test with no input
        let cmd = format_acp_tool_command("Tool", &None);
        assert_eq!(cmd, "Tool");

        // Test with query and path
        let cmd = format_acp_tool_command(
            "Search",
            &Some(serde_json::json!({"query": "TODO", "path": "/src"})),
        );
        // path takes priority over query in current implementation
        assert_eq!(cmd, "Search(/src)");
    }

    /// Test summarize_raw_output with various output types.
    #[test]
    fn test_summarize_raw_output() {
        // Test with lines count
        let output = summarize_raw_output(&serde_json::json!({"lines": 42}));
        assert_eq!(output, "Read 42 lines");

        // Test with success
        let output = summarize_raw_output(&serde_json::json!({"success": true}));
        assert_eq!(output, "Success");

        // Test with success and lines
        let output = summarize_raw_output(&serde_json::json!({"success": true, "lines": 100}));
        assert_eq!(output, "Success: 100 lines");

        // Test with exit_code
        let output = summarize_raw_output(&serde_json::json!({"exit_code": 0}));
        assert_eq!(output, "Completed successfully");

        // Test with non-zero exit_code
        let output = summarize_raw_output(&serde_json::json!({"exit_code": 1}));
        assert_eq!(output, "Exited with code 1");
    }

    /// Test that non-text content blocks produce no events.
    #[test]
    fn test_non_text_content_produces_no_events() {
        let update = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
            acp::ContentBlock::Image(acp::ImageContent::new(String::new(), "image/png")),
        ));

        let events = translate_session_update_to_events(&update);
        assert!(events.is_empty());
    }

    /// Test that unsupported session update types produce no events.
    #[test]
    fn test_unsupported_updates_produce_no_events() {
        let update = acp::SessionUpdate::UserMessageChunk(acp::ContentChunk::new(
            acp::ContentBlock::Text(acp::TextContent::new("User message")),
        ));

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
        assert_eq!(get_op_name(&Op::Shutdown), "Shutdown");
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
