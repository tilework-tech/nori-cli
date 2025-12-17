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
use codex_protocol::parse_command::ParsedCommand;
use codex_protocol::protocol::AskForApproval;
#[cfg(debug_assertions)]
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
            // Unsupported operations - only show error in debug builds
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
                #[cfg(debug_assertions)]
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
            // Catch any new Op variants we haven't handled - only show error in debug builds
            _ => {
                let op_name = get_op_name(&op);
                warn!("Unknown Op in ACP mode: {op_name}");
                #[cfg(debug_assertions)]
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
                let mut event_sequence: u64 = 0;
                // Track call_ids that have already had ExecCommandBegin emitted.
                // The ACP protocol can emit multiple ToolCall events for the same call_id
                // as details become available, but the TUI expects exactly one Begin per call_id.
                let mut emitted_begin_call_ids: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                while let Some(update) = update_rx.recv().await {
                    let events = translate_session_update_to_events(&update);
                    for event_msg in events {
                        // Deduplicate ExecCommandBegin events - only emit the first one per call_id
                        if let EventMsg::ExecCommandBegin(ref begin_ev) = event_msg {
                            if emitted_begin_call_ids.contains(&begin_ev.call_id) {
                                debug!(
                                    target: "acp_event_flow",
                                    call_id = %begin_ev.call_id,
                                    "ACP dispatch: skipping duplicate ExecCommandBegin"
                                );
                                continue;
                            }
                            emitted_begin_call_ids.insert(begin_ev.call_id.clone());
                        }
                        event_sequence += 1;
                        debug!(
                            target: "acp_event_flow",
                            seq = event_sequence,
                            event_type = get_event_msg_type(&event_msg),
                            "ACP dispatch: sending event to TUI"
                        );
                        let _ = event_tx_clone
                            .send(Event {
                                id: id_for_updates.clone(),
                                msg: event_msg,
                            })
                            .await;
                    }
                }
                debug!(
                    target: "acp_event_flow",
                    total_events = event_sequence,
                    "ACP dispatch: update stream completed"
                );
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

    /// Send an error event to the TUI (only used in debug builds).
    #[cfg(debug_assertions)]
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

/// Get a human-readable name for an EventMsg variant
fn get_event_msg_type(msg: &EventMsg) -> &'static str {
    match msg {
        EventMsg::SessionConfigured(_) => "SessionConfigured",
        EventMsg::TaskStarted(_) => "TaskStarted",
        EventMsg::TaskComplete(_) => "TaskComplete",
        EventMsg::AgentMessageDelta(_) => "AgentMessageDelta",
        EventMsg::AgentReasoningDelta(_) => "AgentReasoningDelta",
        EventMsg::ExecCommandBegin(_) => "ExecCommandBegin",
        EventMsg::ExecCommandEnd(_) => "ExecCommandEnd",
        EventMsg::ExecApprovalRequest(_) => "ExecApprovalRequest",
        EventMsg::TurnAborted(_) => "TurnAborted",
        EventMsg::Error(_) => "Error",
        EventMsg::ShutdownComplete => "ShutdownComplete",
        _ => "Other",
    }
}

/// Translate an ACP SessionUpdate to codex_protocol::EventMsg variants.
fn translate_session_update_to_events(update: &acp::SessionUpdate) -> Vec<EventMsg> {
    match update {
        acp::SessionUpdate::AgentMessageChunk(chunk) => {
            if let acp::ContentBlock::Text(text) = &chunk.content {
                debug!(
                    target: "acp_event_flow",
                    event_type = "AgentMessageChunk",
                    delta_len = text.text.len(),
                    delta_preview = %truncate_for_log(&text.text, 50),
                    "ACP -> TUI: streaming text delta"
                );
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
                debug!(
                    target: "acp_event_flow",
                    event_type = "AgentThoughtChunk",
                    delta_len = text.text.len(),
                    "ACP -> TUI: reasoning delta"
                );
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
            // Skip Begin events that don't have useful display information.
            // The ACP protocol emits multiple ToolCall events for the same call_id:
            // 1. First event: generic (title="Read File", raw_input={} or partial)
            // 2. Second event: detailed (title="Read /path/to/file.rs", raw_input={path: "..."})
            // We only want to emit the detailed one to avoid duplicate Begin events in the TUI.
            //
            // Check for useful info in EITHER:
            // - raw_input (has path, command, pattern, etc.)
            // - title itself (contains an absolute path like "Read /home/...")
            let display_args = tool_call
                .raw_input
                .as_ref()
                .and_then(|input| extract_display_args(&tool_call.title, input));
            let title_has_path = title_contains_useful_info(&tool_call.title);
            if display_args.is_none() && !title_has_path {
                debug!(
                    target: "acp_event_flow",
                    event_type = "ToolCall",
                    call_id = %tool_call.tool_call_id,
                    title = %tool_call.title,
                    has_raw_input = tool_call.raw_input.is_some(),
                    title_has_path = title_has_path,
                    "ACP: skipping generic ToolCall (no display args), waiting for detailed event"
                );
                return vec![];
            }

            // Format command with tool name and input arguments for better display
            let command = format_tool_call_command(&tool_call.title, tool_call.raw_input.as_ref());
            // Classify the tool call to enable proper TUI rendering (Exploring vs Command mode)
            let parsed_cmd = classify_tool_to_parsed_command(
                &tool_call.title,
                Some(&tool_call.kind),
                tool_call.raw_input.as_ref(),
            );
            debug!(
                target: "acp_event_flow",
                event_type = "ToolCall",
                call_id = %tool_call.tool_call_id,
                title = %tool_call.title,
                kind = ?tool_call.kind,
                command = %command,
                parsed_cmd_count = parsed_cmd.len(),
                has_raw_input = tool_call.raw_input.is_some(),
                "ACP -> TUI: ExecCommandBegin (tool call started)"
            );
            vec![EventMsg::ExecCommandBegin(
                codex_protocol::protocol::ExecCommandBeginEvent {
                    call_id: tool_call.tool_call_id.to_string(),
                    process_id: None,
                    turn_id: String::new(),
                    command: vec![command],
                    cwd: PathBuf::new(),
                    parsed_cmd,
                    source: codex_protocol::protocol::ExecCommandSource::Agent,
                    interaction_input: None,
                },
            )]
        }
        acp::SessionUpdate::ToolCallUpdate(update) => {
            // Tool call updates can be mapped based on status
            let status = update.fields.status;
            debug!(
                target: "acp_event_flow",
                event_type = "ToolCallUpdate",
                call_id = %update.tool_call_id,
                status = ?status,
                title = ?update.fields.title,
                "ACP: tool call update received"
            );
            if status == Some(acp::ToolCallStatus::Completed) {
                // Extract output from tool call content and raw_output
                let aggregated_output = extract_tool_output(&update.fields);
                let title = update.fields.title.clone().unwrap_or_default();
                let command = format_tool_call_command(&title, update.fields.raw_input.as_ref());
                // Classify the tool call to enable proper TUI rendering (Exploring vs Command mode)
                let parsed_cmd = classify_tool_to_parsed_command(
                    &title,
                    update.fields.kind.as_ref(),
                    update.fields.raw_input.as_ref(),
                );

                debug!(
                    target: "acp_event_flow",
                    event_type = "ToolCallUpdate",
                    call_id = %update.tool_call_id,
                    title = %title,
                    command = %command,
                    output_len = aggregated_output.len(),
                    "ACP -> TUI: ExecCommandEnd (tool call completed)"
                );
                vec![EventMsg::ExecCommandEnd(
                    codex_protocol::protocol::ExecCommandEndEvent {
                        call_id: update.tool_call_id.to_string(),
                        process_id: None,
                        turn_id: String::new(),
                        command: vec![command],
                        cwd: PathBuf::new(),
                        parsed_cmd,
                        source: codex_protocol::protocol::ExecCommandSource::Agent,
                        interaction_input: None,
                        stdout: String::new(),
                        stderr: String::new(),
                        aggregated_output,
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
        other => {
            debug!(
                target: "acp_event_flow",
                event_type = ?std::mem::discriminant(other),
                "ACP: unhandled update type (no event emitted)"
            );
            vec![]
        }
    }
}

/// Truncate a string for logging purposes
fn truncate_for_log(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

/// Check if a tool call title contains useful display information.
///
/// Some ACP providers include the path/command directly in the title
/// (e.g., "Read /home/user/file.rs" or "`git status`") rather than in raw_input.
/// This function detects such cases so we don't skip them.
fn title_contains_useful_info(title: &str) -> bool {
    // Check for absolute paths (Unix or Windows style)
    if title.contains(" /") || title.contains(" C:\\") || title.contains(" ~") {
        return true;
    }
    // Check for backtick-quoted commands (e.g., "`git status`")
    if title.contains('`') {
        return true;
    }
    // Check for patterns that suggest it's not a generic title
    // Generic titles are typically just the tool name like "Read File", "Terminal", "Search"
    let generic_patterns = [
        "Read File",
        "Read file",
        "Terminal",
        "Search",
        "Grep",
        "Glob",
        "List",
        "Write",
        "Edit",
    ];
    for pattern in &generic_patterns {
        if title == *pattern {
            return false;
        }
    }
    // If the title is longer than typical generic names and contains a space,
    // it likely has useful info
    title.len() > 15 && title.contains(' ')
}

/// Format a tool call command with its input arguments for display.
///
/// Creates a display string like "Read(path/to/file.rs)" or "Terminal(git status)"
fn format_tool_call_command(title: &str, raw_input: Option<&serde_json::Value>) -> String {
    let args = raw_input
        .and_then(|input| extract_display_args(title, input))
        .unwrap_or_default();

    if args.is_empty() {
        title.to_string()
    } else {
        format!("{title}({args})")
    }
}

/// Extract display-friendly arguments from raw_input based on tool type.
fn extract_display_args(title: &str, input: &serde_json::Value) -> Option<String> {
    let title_lower = title.to_lowercase();

    // Try to extract the most relevant argument based on tool type
    // Note: Order matters - more specific matches should come first
    if title_lower.contains("search")
        || title_lower.contains("find")
        || title_lower.contains("grep")
    {
        // For search operations, show the pattern/query
        let pattern = input
            .get("pattern")
            .or_else(|| input.get("query"))
            .or_else(|| input.get("glob"))
            .and_then(|v| v.as_str());
        let path = input.get("path").and_then(|v| v.as_str());

        match (pattern, path) {
            (Some(p), Some(dir)) => Some(format!("{p} in {dir}")),
            (Some(p), None) => Some(p.to_string()),
            (None, Some(dir)) => Some(dir.to_string()),
            (None, None) => None,
        }
    } else if title_lower.contains("terminal")
        || title_lower.contains("shell")
        || title_lower.contains("bash")
        || title_lower.contains("exec")
    {
        // For shell commands, show the command
        input
            .get("command")
            .or_else(|| input.get("cmd"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    } else if title_lower.contains("list") || title_lower.contains("ls") {
        // For list operations, show the path
        input
            .get("path")
            .or_else(|| input.get("directory"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    } else if title_lower.contains("write") || title_lower.contains("edit") {
        // For write operations, show the path
        input
            .get("path")
            .or_else(|| input.get("file_path"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    } else if title_lower.contains("read") || title_lower.contains("file") {
        // For file read operations, show the path
        input
            .get("path")
            .or_else(|| input.get("file_path"))
            .or_else(|| input.get("file"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    } else {
        // Generic fallback: try common argument names
        input
            .get("path")
            .or_else(|| input.get("command"))
            .or_else(|| input.get("query"))
            .or_else(|| input.get("name"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }
}

/// Extract tool output from ToolCallUpdateFields for display.
///
/// Returns a formatted string containing the tool's output content.
fn extract_tool_output(fields: &acp::ToolCallUpdateFields) -> String {
    let mut output_parts: Vec<String> = Vec::new();

    // Extract text content from the content field
    if let Some(content) = &fields.content {
        for item in content {
            if let acp::ToolCallContent::Content(c) = item
                && let acp::ContentBlock::Text(text) = &c.content
                && !text.text.is_empty()
            {
                output_parts.push(text.text.clone());
            }
        }
    }

    // If no text content, try to extract meaningful info from raw_output
    if output_parts.is_empty()
        && let Some(raw_output) = &fields.raw_output
        && let Some(output_str) = format_raw_output(raw_output, fields.title.as_deref())
    {
        output_parts.push(output_str);
    }

    output_parts.join("\n")
}

/// Format raw_output JSON into a human-readable string based on tool type.
fn format_raw_output(raw_output: &serde_json::Value, title: Option<&str>) -> Option<String> {
    let title_lower = title.map(str::to_lowercase).unwrap_or_default();

    // Try to provide meaningful summaries based on common output patterns
    if let Some(obj) = raw_output.as_object() {
        // Check for line count (common in read operations)
        if let Some(lines) = obj.get("lines").and_then(serde_json::Value::as_u64) {
            return Some(format!("Read {lines} lines"));
        }

        // Check for file count (common in find/search operations)
        if let Some(count) = obj.get("count").and_then(serde_json::Value::as_u64) {
            if title_lower.contains("find") || title_lower.contains("search") {
                return Some(format!("Found {count} files"));
            }
            return Some(format!("{count} matches"));
        }

        // Check for files array
        if let Some(files) = obj.get("files").and_then(|v| v.as_array()) {
            let count = files.len();
            let file_list: Vec<&str> = files.iter().filter_map(|f| f.as_str()).take(5).collect();
            if count > 5 {
                return Some(format!(
                    "Found {} files\n{}...",
                    count,
                    file_list.join("\n")
                ));
            } else if !file_list.is_empty() {
                return Some(format!("Found {} files\n{}", count, file_list.join("\n")));
            }
        }

        // Check for exit_code (common in shell operations)
        if let Some(exit_code) = obj.get("exit_code").and_then(serde_json::Value::as_i64) {
            // Look for stdout/output
            let output = obj
                .get("stdout")
                .or_else(|| obj.get("output"))
                .and_then(|v| v.as_str());
            if let Some(out) = output {
                if exit_code != 0 {
                    return Some(format!("Exit code: {exit_code}\n{out}"));
                }
                return Some(out.to_string());
            }
            if exit_code != 0 {
                return Some(format!("Exit code: {exit_code}"));
            }
        }

        // Check for success boolean
        if let Some(success) = obj.get("success").and_then(serde_json::Value::as_bool)
            && !success
        {
            if let Some(error) = obj.get("error").and_then(|v| v.as_str()) {
                return Some(format!("Failed: {error}"));
            }
            return Some("Operation failed".to_string());
        }
    }

    // For arrays, show count
    if let Some(arr) = raw_output.as_array()
        && !arr.is_empty()
    {
        return Some(format!("{} items", arr.len()));
    }

    // For strings, return directly
    if let Some(s) = raw_output.as_str()
        && !s.is_empty()
    {
        return Some(s.to_string());
    }

    None
}

/// Classify a tool call into ParsedCommand variants based on ACP ToolKind.
///
/// This enables the TUI to render tool calls appropriately:
/// - `Read`, `ListFiles`, `Search` → "Exploring" mode with compact, grouped display
/// - `Unknown` → "Command" mode with full command text display
///
/// # ACP ToolKind mappings:
/// - `Read` → `ParsedCommand::Read` (exploring)
/// - `Search` → `ParsedCommand::Search` (exploring)
/// - `Edit`, `Delete`, `Move`, `Execute`, `Fetch` → `ParsedCommand::Unknown` (command)
/// - `Think`, `Other` → `ParsedCommand::Unknown` (command)
fn classify_tool_to_parsed_command(
    title: &str,
    kind: Option<&acp::ToolKind>,
    raw_input: Option<&serde_json::Value>,
) -> Vec<ParsedCommand> {
    match kind {
        // Read operations → Exploring mode
        Some(acp::ToolKind::Read) => {
            let path = raw_input
                .and_then(|i| {
                    i.get("path")
                        .or_else(|| i.get("file_path"))
                        .or_else(|| i.get("file"))
                })
                .and_then(|v| v.as_str())
                .unwrap_or("file");
            let name = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string());
            vec![ParsedCommand::Read {
                cmd: title.to_string(),
                name,
                path: std::path::PathBuf::from(path),
            }]
        }

        // Search operations → Exploring mode
        Some(acp::ToolKind::Search) => {
            let query = raw_input
                .and_then(|i| i.get("pattern").or_else(|| i.get("query")))
                .and_then(|v| v.as_str())
                .map(String::from);
            let path = raw_input
                .and_then(|i| i.get("path").or_else(|| i.get("directory")))
                .and_then(|v| v.as_str())
                .map(String::from);
            vec![ParsedCommand::Search {
                cmd: title.to_string(),
                query,
                path,
            }]
        }

        // Edit, Delete, Move → Command mode (mutating operations)
        Some(acp::ToolKind::Edit | acp::ToolKind::Delete | acp::ToolKind::Move) => {
            vec![ParsedCommand::Unknown {
                cmd: format_tool_call_command(title, raw_input),
            }]
        }

        // Execute → Command mode (shell/terminal operations)
        Some(acp::ToolKind::Execute) => {
            vec![ParsedCommand::Unknown {
                cmd: format_tool_call_command(title, raw_input),
            }]
        }

        // Fetch → Command mode (external data retrieval)
        Some(acp::ToolKind::Fetch) => {
            vec![ParsedCommand::Unknown {
                cmd: format_tool_call_command(title, raw_input),
            }]
        }

        // Think → Command mode (internal reasoning)
        Some(acp::ToolKind::Think) => {
            vec![ParsedCommand::Unknown {
                cmd: format_tool_call_command(title, raw_input),
            }]
        }

        // Other or unknown → Command mode (fallback)
        Some(acp::ToolKind::Other) | None => {
            // Try to infer from title as fallback
            classify_tool_by_title(title, raw_input)
        }

        // Catch any future ToolKind variants
        #[allow(unreachable_patterns)]
        _ => {
            vec![ParsedCommand::Unknown {
                cmd: format_tool_call_command(title, raw_input),
            }]
        }
    }
}

/// Fallback classification based on tool title when ToolKind is not available.
///
/// Uses heuristics to detect common tool patterns.
fn classify_tool_by_title(
    title: &str,
    raw_input: Option<&serde_json::Value>,
) -> Vec<ParsedCommand> {
    let title_lower = title.to_lowercase();

    // List/Glob operations → Exploring mode
    if title_lower.contains("list")
        || title_lower.contains("glob")
        || title_lower.contains("ls")
        || title_lower == "find"
        || title_lower.contains("find files")
    {
        let path = raw_input
            .and_then(|i| i.get("path").or_else(|| i.get("directory")))
            .and_then(|v| v.as_str())
            .map(String::from);
        return vec![ParsedCommand::ListFiles {
            cmd: title.to_string(),
            path,
        }];
    }

    // Search/Grep operations → Exploring mode
    if title_lower.contains("search") || title_lower.contains("grep") {
        let query = raw_input
            .and_then(|i| i.get("pattern").or_else(|| i.get("query")))
            .and_then(|v| v.as_str())
            .map(String::from);
        let path = raw_input
            .and_then(|i| i.get("path"))
            .and_then(|v| v.as_str())
            .map(String::from);
        return vec![ParsedCommand::Search {
            cmd: title.to_string(),
            query,
            path,
        }];
    }

    // Read operations → Exploring mode
    if title_lower.contains("read") || title_lower == "file" {
        let path = raw_input
            .and_then(|i| {
                i.get("path")
                    .or_else(|| i.get("file_path"))
                    .or_else(|| i.get("file"))
            })
            .and_then(|v| v.as_str())
            .unwrap_or("file");
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        return vec![ParsedCommand::Read {
            cmd: title.to_string(),
            name,
            path: std::path::PathBuf::from(path),
        }];
    }

    // Default: Command mode
    vec![ParsedCommand::Unknown {
        cmd: format_tool_call_command(title, raw_input),
    }]
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
                // Command now includes formatted arguments
                assert_eq!(begin.command[0], "shell(ls -la)");
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
                .title("read_file"),
        ));

        let events = translate_session_update_to_events(&update);
        assert_eq!(events.len(), 1);

        match &events[0] {
            EventMsg::ExecCommandEnd(end) => {
                assert_eq!(end.call_id, "call-456");
            }
            _ => panic!("Expected ExecCommandEnd event"),
        }
    }

    /// Test that ToolCallUpdate with content extracts the output text.
    #[test]
    fn test_extract_tool_output_from_content() {
        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::from("call-789".to_string()),
            acp::ToolCallUpdateFields::new()
                .status(acp::ToolCallStatus::Completed)
                .title("read_file")
                .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                    acp::ContentBlock::Text(acp::TextContent::new("File contents here")),
                ))]),
        ));

        let events = translate_session_update_to_events(&update);
        assert_eq!(events.len(), 1);

        match &events[0] {
            EventMsg::ExecCommandEnd(end) => {
                assert_eq!(end.aggregated_output, "File contents here");
            }
            _ => panic!("Expected ExecCommandEnd event"),
        }
    }

    /// Test that ToolCallUpdate with raw_output extracts meaningful info.
    #[test]
    fn test_extract_tool_output_from_raw_output() {
        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::from("call-read".to_string()),
            acp::ToolCallUpdateFields::new()
                .status(acp::ToolCallStatus::Completed)
                .title("read_file")
                .raw_output(serde_json::json!({"lines": 42})),
        ));

        let events = translate_session_update_to_events(&update);
        match &events[0] {
            EventMsg::ExecCommandEnd(end) => {
                assert_eq!(end.aggregated_output, "Read 42 lines");
            }
            _ => panic!("Expected ExecCommandEnd event"),
        }
    }

    /// Test that tool command is formatted with path argument.
    #[test]
    fn test_format_tool_call_command_with_path() {
        let cmd = format_tool_call_command(
            "Read File",
            Some(&serde_json::json!({"path": "src/main.rs"})),
        );
        assert_eq!(cmd, "Read File(src/main.rs)");
    }

    /// Test that shell command is formatted with command argument.
    #[test]
    fn test_format_tool_call_command_shell() {
        let cmd = format_tool_call_command(
            "Terminal",
            Some(&serde_json::json!({"command": "git status"})),
        );
        assert_eq!(cmd, "Terminal(git status)");
    }

    /// Test that search command is formatted with pattern and path.
    #[test]
    fn test_format_tool_call_command_search() {
        let cmd = format_tool_call_command(
            "Find Files",
            Some(&serde_json::json!({"pattern": "*.rs", "path": "src/"})),
        );
        assert_eq!(cmd, "Find Files(*.rs in src/)");
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

    // ==================== Tool Classification Tests ====================

    /// Test that ToolKind::Read produces ParsedCommand::Read (Exploring mode).
    #[test]
    fn test_classify_tool_kind_read() {
        let parsed = classify_tool_to_parsed_command(
            "Read File",
            Some(&acp::ToolKind::Read),
            Some(&serde_json::json!({"path": "src/main.rs"})),
        );
        assert_eq!(parsed.len(), 1);
        match &parsed[0] {
            ParsedCommand::Read { cmd, name, path } => {
                assert_eq!(cmd, "Read File");
                assert_eq!(name, "main.rs");
                assert_eq!(path.to_string_lossy(), "src/main.rs");
            }
            _ => panic!("Expected ParsedCommand::Read"),
        }
    }

    /// Test that ToolKind::Search produces ParsedCommand::Search (Exploring mode).
    #[test]
    fn test_classify_tool_kind_search() {
        let parsed = classify_tool_to_parsed_command(
            "Search Files",
            Some(&acp::ToolKind::Search),
            Some(&serde_json::json!({"pattern": "TODO", "path": "src/"})),
        );
        assert_eq!(parsed.len(), 1);
        match &parsed[0] {
            ParsedCommand::Search { cmd, query, path } => {
                assert_eq!(cmd, "Search Files");
                assert_eq!(query.as_deref(), Some("TODO"));
                assert_eq!(path.as_deref(), Some("src/"));
            }
            _ => panic!("Expected ParsedCommand::Search"),
        }
    }

    /// Test that ToolKind::Execute produces ParsedCommand::Unknown (Command mode).
    #[test]
    fn test_classify_tool_kind_execute() {
        let parsed = classify_tool_to_parsed_command(
            "Terminal",
            Some(&acp::ToolKind::Execute),
            Some(&serde_json::json!({"command": "git status"})),
        );
        assert_eq!(parsed.len(), 1);
        match &parsed[0] {
            ParsedCommand::Unknown { cmd } => {
                assert_eq!(cmd, "Terminal(git status)");
            }
            _ => panic!("Expected ParsedCommand::Unknown"),
        }
    }

    /// Test that ToolKind::Edit produces ParsedCommand::Unknown (Command mode).
    #[test]
    fn test_classify_tool_kind_edit() {
        let parsed = classify_tool_to_parsed_command(
            "Edit File",
            Some(&acp::ToolKind::Edit),
            Some(&serde_json::json!({"path": "src/lib.rs"})),
        );
        assert_eq!(parsed.len(), 1);
        match &parsed[0] {
            ParsedCommand::Unknown { cmd } => {
                assert!(cmd.contains("Edit File"));
            }
            _ => panic!("Expected ParsedCommand::Unknown"),
        }
    }

    /// Test that ToolKind::Delete produces ParsedCommand::Unknown (Command mode).
    #[test]
    fn test_classify_tool_kind_delete() {
        let parsed = classify_tool_to_parsed_command(
            "Delete File",
            Some(&acp::ToolKind::Delete),
            Some(&serde_json::json!({"path": "temp.txt"})),
        );
        assert_eq!(parsed.len(), 1);
        match &parsed[0] {
            ParsedCommand::Unknown { .. } => {}
            _ => panic!("Expected ParsedCommand::Unknown"),
        }
    }

    /// Test that ToolKind::Move produces ParsedCommand::Unknown (Command mode).
    #[test]
    fn test_classify_tool_kind_move() {
        let parsed = classify_tool_to_parsed_command(
            "Move File",
            Some(&acp::ToolKind::Move),
            Some(&serde_json::json!({"from": "a.txt", "to": "b.txt"})),
        );
        assert_eq!(parsed.len(), 1);
        match &parsed[0] {
            ParsedCommand::Unknown { .. } => {}
            _ => panic!("Expected ParsedCommand::Unknown"),
        }
    }

    /// Test that ToolKind::Fetch produces ParsedCommand::Unknown (Command mode).
    #[test]
    fn test_classify_tool_kind_fetch() {
        let parsed = classify_tool_to_parsed_command(
            "Fetch URL",
            Some(&acp::ToolKind::Fetch),
            Some(&serde_json::json!({"url": "https://example.com"})),
        );
        assert_eq!(parsed.len(), 1);
        match &parsed[0] {
            ParsedCommand::Unknown { .. } => {}
            _ => panic!("Expected ParsedCommand::Unknown"),
        }
    }

    /// Test that ToolKind::Think produces ParsedCommand::Unknown (Command mode).
    #[test]
    fn test_classify_tool_kind_think() {
        let parsed = classify_tool_to_parsed_command("Think", Some(&acp::ToolKind::Think), None);
        assert_eq!(parsed.len(), 1);
        match &parsed[0] {
            ParsedCommand::Unknown { .. } => {}
            _ => panic!("Expected ParsedCommand::Unknown"),
        }
    }

    /// Test title-based fallback for ToolKind::Other with "list" in title.
    #[test]
    fn test_classify_fallback_list_by_title() {
        let parsed = classify_tool_to_parsed_command(
            "List Directory",
            Some(&acp::ToolKind::Other),
            Some(&serde_json::json!({"path": "src/"})),
        );
        assert_eq!(parsed.len(), 1);
        match &parsed[0] {
            ParsedCommand::ListFiles { cmd, path } => {
                assert_eq!(cmd, "List Directory");
                assert_eq!(path.as_deref(), Some("src/"));
            }
            _ => panic!("Expected ParsedCommand::ListFiles"),
        }
    }

    /// Test title-based fallback for ToolKind::Other with "grep" in title.
    #[test]
    fn test_classify_fallback_grep_by_title() {
        let parsed = classify_tool_to_parsed_command(
            "Grep Files",
            Some(&acp::ToolKind::Other),
            Some(&serde_json::json!({"pattern": "error", "path": "logs/"})),
        );
        assert_eq!(parsed.len(), 1);
        match &parsed[0] {
            ParsedCommand::Search { cmd, query, path } => {
                assert_eq!(cmd, "Grep Files");
                assert_eq!(query.as_deref(), Some("error"));
                assert_eq!(path.as_deref(), Some("logs/"));
            }
            _ => panic!("Expected ParsedCommand::Search"),
        }
    }

    /// Test title-based fallback for ToolKind::Other with "read" in title.
    #[test]
    fn test_classify_fallback_read_by_title() {
        let parsed = classify_tool_to_parsed_command(
            "Read Config",
            Some(&acp::ToolKind::Other),
            Some(&serde_json::json!({"file_path": "config.toml"})),
        );
        assert_eq!(parsed.len(), 1);
        match &parsed[0] {
            ParsedCommand::Read { cmd, name, .. } => {
                assert_eq!(cmd, "Read Config");
                assert_eq!(name, "config.toml");
            }
            _ => panic!("Expected ParsedCommand::Read"),
        }
    }

    /// Test that None kind falls back to title-based classification.
    #[test]
    fn test_classify_none_kind_fallback() {
        let parsed = classify_tool_to_parsed_command(
            "Search Code",
            None,
            Some(&serde_json::json!({"query": "fn main"})),
        );
        assert_eq!(parsed.len(), 1);
        match &parsed[0] {
            ParsedCommand::Search { cmd, query, .. } => {
                assert_eq!(cmd, "Search Code");
                assert_eq!(query.as_deref(), Some("fn main"));
            }
            _ => panic!("Expected ParsedCommand::Search"),
        }
    }

    /// Test that ToolCall with Read kind generates parsed_cmd in ExecCommandBegin.
    #[test]
    fn test_tool_call_read_generates_exploring_parsed_cmd() {
        let update = acp::SessionUpdate::ToolCall(
            acp::ToolCall::new(acp::ToolCallId::from("call-read".to_string()), "Read File")
                .kind(acp::ToolKind::Read)
                .status(acp::ToolCallStatus::InProgress)
                .raw_input(serde_json::json!({"path": "src/lib.rs"})),
        );

        let events = translate_session_update_to_events(&update);
        assert_eq!(events.len(), 1);

        match &events[0] {
            EventMsg::ExecCommandBegin(begin) => {
                assert_eq!(begin.parsed_cmd.len(), 1);
                match &begin.parsed_cmd[0] {
                    ParsedCommand::Read { name, .. } => {
                        assert_eq!(name, "lib.rs");
                    }
                    _ => panic!("Expected ParsedCommand::Read"),
                }
            }
            _ => panic!("Expected ExecCommandBegin event"),
        }
    }

    /// Test that ToolCall with Execute kind generates command-mode parsed_cmd.
    #[test]
    fn test_tool_call_execute_generates_command_parsed_cmd() {
        let update = acp::SessionUpdate::ToolCall(
            acp::ToolCall::new(acp::ToolCallId::from("call-exec".to_string()), "Terminal")
                .kind(acp::ToolKind::Execute)
                .status(acp::ToolCallStatus::InProgress)
                .raw_input(serde_json::json!({"command": "cargo test"})),
        );

        let events = translate_session_update_to_events(&update);
        assert_eq!(events.len(), 1);

        match &events[0] {
            EventMsg::ExecCommandBegin(begin) => {
                assert_eq!(begin.parsed_cmd.len(), 1);
                match &begin.parsed_cmd[0] {
                    ParsedCommand::Unknown { cmd } => {
                        assert!(cmd.contains("cargo test"));
                    }
                    _ => panic!("Expected ParsedCommand::Unknown"),
                }
            }
            _ => panic!("Expected ExecCommandBegin event"),
        }
    }

    /// Test that ToolCallUpdate with Read kind generates exploring parsed_cmd in ExecCommandEnd.
    #[test]
    fn test_tool_call_update_read_generates_exploring_parsed_cmd() {
        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::from("call-read-end".to_string()),
            acp::ToolCallUpdateFields::new()
                .status(acp::ToolCallStatus::Completed)
                .title("Read File")
                .kind(acp::ToolKind::Read)
                .raw_input(serde_json::json!({"path": "Cargo.toml"})),
        ));

        let events = translate_session_update_to_events(&update);
        assert_eq!(events.len(), 1);

        match &events[0] {
            EventMsg::ExecCommandEnd(end) => {
                assert_eq!(end.parsed_cmd.len(), 1);
                match &end.parsed_cmd[0] {
                    ParsedCommand::Read { name, .. } => {
                        assert_eq!(name, "Cargo.toml");
                    }
                    _ => panic!("Expected ParsedCommand::Read"),
                }
            }
            _ => panic!("Expected ExecCommandEnd event"),
        }
    }
}
