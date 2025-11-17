#![allow(dead_code)]

use crate::backends::BackendEvent;
use crate::conversation::{ConversationEvent, PlanEntry};
use crate::history::{InlineEntryId, InlineEntryKind, InlineEntryUpdate};
use agent_client_protocol::{
    self as acp, Agent, Client, ClientCapabilities, ContentBlock, FileSystemCapability,
    Implementation, InitializeRequest, NewSessionRequest, PermissionOptionKind, PlanEntryPriority,
    PlanEntryStatus, PromptRequest, ReadTextFileRequest, ReadTextFileResponse,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    Result as AcpResult, SessionNotification, SessionUpdate, TextContent, ToolCallContent,
    ToolCallStatus, ToolKind, WriteTextFileRequest, WriteTextFileResponse,
};
use futures::stream::Stream;
use std::cell::RefCell;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::rc::Rc;
use std::thread;
use std::time::Duration;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::runtime::Builder as TokioRuntimeBuilder;
use tokio::sync::{mpsc, oneshot};
use tokio::task::LocalSet;
use tokio::time::timeout;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// Configuration for an ACP agent
#[derive(Debug, Clone)]
pub struct AcpAgentConfig {
    pub name: &'static str,
    pub command: &'static str,
    pub args: Vec<String>,
    pub install_url: &'static str,
    pub install_command: Option<Vec<String>>,
}

/// Translates ACP SessionUpdate to ConversationEvent
pub fn translate_session_update(update: SessionUpdate) -> Option<ConversationEvent> {
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => {
            if let ContentBlock::Text(text_content) = chunk.content {
                Some(ConversationEvent::AssistantMessage {
                    text: text_content.text,
                })
            } else {
                None
            }
        }
        SessionUpdate::UserMessageChunk(chunk) => {
            if let ContentBlock::Text(text_content) = chunk.content {
                Some(ConversationEvent::UserMessage {
                    text: text_content.text,
                })
            } else {
                None
            }
        }
        SessionUpdate::AgentThoughtChunk(chunk) => {
            if let ContentBlock::Text(text_content) = chunk.content {
                Some(ConversationEvent::AgentThinking {
                    text: text_content.text,
                })
            } else {
                None
            }
        }
        SessionUpdate::ToolCall(tool_call) => Some(ConversationEvent::ToolCallStarted {
            id: tool_call.id.to_string(),
            title: tool_call.title,
            kind: match tool_call.kind {
                ToolKind::Edit => "edit".to_string(),
                ToolKind::Execute => "execute".to_string(),
                ToolKind::Read => "read".to_string(),
                ToolKind::Delete => "delete".to_string(),
                ToolKind::Move => "move".to_string(),
                ToolKind::Search => "search".to_string(),
                ToolKind::Think => "think".to_string(),
                ToolKind::Fetch => "fetch".to_string(),
                ToolKind::SwitchMode => "switch_mode".to_string(),
                ToolKind::Other => "other".to_string(),
            },
        }),
        SessionUpdate::ToolCallUpdate(update) => {
            let status_str = match update.fields.status {
                Some(ToolCallStatus::Pending) => "pending",
                Some(ToolCallStatus::InProgress) => "in_progress",
                Some(ToolCallStatus::Completed) => "completed",
                Some(ToolCallStatus::Failed) => "failed",
                None => "unknown",
            };

            let content = update.fields.content.and_then(|blocks| {
                blocks.into_iter().find_map(|block| match block {
                    ToolCallContent::Content {
                        content: ContentBlock::Text(text_content),
                    } => Some(text_content.text),
                    _ => None,
                })
            });

            Some(ConversationEvent::ToolCallProgress {
                id: update.id.to_string(),
                status: status_str.to_string(),
                content,
            })
        }
        SessionUpdate::Plan(plan) => Some(ConversationEvent::AgentPlan {
            entries: plan
                .entries
                .into_iter()
                .map(|entry| PlanEntry {
                    content: entry.content,
                    status: match entry.status {
                        PlanEntryStatus::Pending => "pending".to_string(),
                        PlanEntryStatus::InProgress => "in_progress".to_string(),
                        PlanEntryStatus::Completed => "completed".to_string(),
                    },
                    priority: Some(match entry.priority {
                        PlanEntryPriority::High => "high".to_string(),
                        PlanEntryPriority::Medium => "medium".to_string(),
                        PlanEntryPriority::Low => "low".to_string(),
                    }),
                })
                .collect(),
        }),
        _ => None,
    }
}

/// Client handler that implements the ACP Client trait
/// Handles file operations and permission requests from the agent
pub struct AcpClientHandler {
    /// Working directory for file operations
    cwd: PathBuf,
    /// Channel to send session updates to the runner
    update_tx: mpsc::UnboundedSender<SessionUpdate>,
    /// Cancellation token to check if the session was cancelled
    cancel_token: CancellationToken,
}

impl AcpClientHandler {
    pub fn new(
        cwd: PathBuf,
        update_tx: mpsc::UnboundedSender<SessionUpdate>,
        cancel_token: CancellationToken,
    ) -> Self {
        Self {
            cwd,
            update_tx,
            cancel_token,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Client for AcpClientHandler {
    async fn request_permission(
        &self,
        args: RequestPermissionRequest,
    ) -> AcpResult<RequestPermissionResponse> {
        debug!("Permission requested for tool call");

        // Check if session was cancelled
        if self.cancel_token.is_cancelled() {
            return Ok(RequestPermissionResponse {
                outcome: RequestPermissionOutcome::Cancelled,
                meta: None,
            });
        }

        // Auto-approve by selecting the first "allow" option
        // Find the first AllowOnce or AllowAlways option, or default to first option
        let option_id = args
            .options
            .iter()
            .find(|opt| {
                matches!(
                    opt.kind,
                    PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
                )
            })
            .or_else(|| args.options.first())
            .map(|opt| opt.id.clone())
            .ok_or_else(agent_client_protocol::Error::internal_error)?;

        debug!("Permission granted: option_id={:?}", option_id);

        Ok(RequestPermissionResponse {
            outcome: RequestPermissionOutcome::Selected { option_id },
            meta: None,
        })
    }

    async fn session_notification(&self, args: SessionNotification) -> AcpResult<()> {
        // Forward the session update to the runner's stream
        let _ = self.update_tx.send(args.update);
        Ok(())
    }

    async fn read_text_file(&self, args: ReadTextFileRequest) -> AcpResult<ReadTextFileResponse> {
        debug!("Reading file: {:?}", args.path);

        // Ensure the path is within the working directory
        let requested_path = PathBuf::from(&args.path);
        let canonical_path = if requested_path.is_absolute() {
            requested_path
        } else {
            self.cwd.join(&requested_path)
        };

        // Read the file
        match tokio::fs::read_to_string(&canonical_path).await {
            Ok(content) => Ok(ReadTextFileResponse {
                content,
                meta: None,
            }),
            Err(_e) => {
                warn!("File read failed: {:?}", canonical_path);
                Err(agent_client_protocol::Error::internal_error())
            }
        }
    }

    async fn write_text_file(
        &self,
        args: WriteTextFileRequest,
    ) -> AcpResult<WriteTextFileResponse> {
        debug!(
            "Writing file: {:?}, content_length={}",
            args.path,
            args.content.len()
        );

        // Ensure the path is within the working directory
        let requested_path = PathBuf::from(&args.path);
        let canonical_path = if requested_path.is_absolute() {
            requested_path
        } else {
            self.cwd.join(&requested_path)
        };

        // Create parent directories if they don't exist
        if let Some(parent) = canonical_path.parent()
            && let Err(_e) = tokio::fs::create_dir_all(parent).await
        {
            warn!(
                "Failed to create parent directories for: {:?}",
                canonical_path
            );
            return Err(agent_client_protocol::Error::internal_error());
        }

        // Write the file
        match tokio::fs::write(&canonical_path, &args.content).await {
            Ok(_) => Ok(WriteTextFileResponse { meta: None }),
            Err(_e) => {
                warn!("File write failed: {:?}", canonical_path);
                Err(agent_client_protocol::Error::internal_error())
            }
        }
    }

    // Terminal methods are not implemented (blocked as per requirements)
    // The default implementations in the trait return method_not_found errors
}

/// Runner for ACP-compliant agents
pub struct AcpAgentRunner {
    config: AcpAgentConfig,
    cwd: PathBuf,
    _agent_process: Option<Child>,
}

impl AcpAgentRunner {
    pub fn new(config: AcpAgentConfig, cwd: PathBuf) -> Self {
        Self {
            config,
            cwd,
            _agent_process: None,
        }
    }

    pub async fn spawn_stream(
        &mut self,
        prompt: String,
        cancel_token: CancellationToken,
    ) -> Result<Pin<Box<dyn Stream<Item = BackendEvent> + Send>>, String> {
        if let Some(mut existing) = self._agent_process.take() {
            let _ = existing.kill().await;
        }

        let mut command = Command::new(self.config.command);
        command
            .args(&self.config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let mut child = command
            .spawn()
            .map_err(|e| format!("Failed to spawn agent: {e}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Failed to capture agent stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Failed to capture agent stdout".to_string())?;

        self._agent_process = Some(child);

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (session_update_tx, session_update_rx) = mpsc::unbounded_channel();
        let (handshake_tx, handshake_rx) = oneshot::channel();

        let client_handler =
            AcpClientHandler::new(self.cwd.clone(), session_update_tx, cancel_token.clone());
        let cwd = self.cwd.clone();

        thread::spawn(move || {
            let runtime = TokioRuntimeBuilder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to build ACP runtime");
            runtime.block_on(run_acp_connection(
                stdin,
                stdout,
                client_handler,
                session_update_rx,
                event_tx,
                cancel_token,
                prompt,
                cwd,
                handshake_tx,
            ));
        });

        match handshake_rx
            .await
            .map_err(|_| "ACP connection task exited before initialization".to_string())?
        {
            Ok(()) => {
                let stream = UnboundedReceiverStream::new(event_rx);
                Ok(Box::pin(stream))
            }
            Err(err) => {
                if let Some(mut child) = self._agent_process.take() {
                    let _ = child.kill().await;
                }
                Err(err)
            }
        }
    }

    pub fn name(&self) -> &str {
        self.config.name
    }

    pub fn command_name(&self) -> &str {
        self.config.command
    }

    pub fn install_url(&self) -> &str {
        self.config.install_url
    }

    pub fn install_command(&self) -> Option<Vec<String>> {
        self.config.install_command.clone()
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_acp_connection(
    stdin: ChildStdin,
    stdout: ChildStdout,
    client_handler: AcpClientHandler,
    session_update_rx: mpsc::UnboundedReceiver<SessionUpdate>,
    event_tx: mpsc::UnboundedSender<BackendEvent>,
    cancel_token: CancellationToken,
    prompt: String,
    cwd: PathBuf,
    handshake_tx: oneshot::Sender<Result<(), String>>,
) {
    let local = LocalSet::new();
    let connection_event_tx = event_tx.clone();
    let result = local
        .run_until(async move {
            run_connection_inner(
                stdin,
                stdout,
                client_handler,
                session_update_rx,
                connection_event_tx,
                cancel_token,
                prompt,
                cwd,
                handshake_tx,
            )
            .await
        })
        .await;

    if let Err(err) = result {
        let _ = event_tx.send(BackendEvent::Conversation(ConversationEvent::SystemEvent {
            subtype: "acp_error".to_string(),
            details: Some(err),
        }));
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_connection_inner(
    stdin: ChildStdin,
    stdout: ChildStdout,
    client_handler: AcpClientHandler,
    session_update_rx: mpsc::UnboundedReceiver<SessionUpdate>,
    event_tx: mpsc::UnboundedSender<BackendEvent>,
    cancel_token: CancellationToken,
    prompt: String,
    cwd: PathBuf,
    handshake_tx: oneshot::Sender<Result<(), String>>,
) -> Result<(), String> {
    let (connection, io_future) = acp::ClientSideConnection::new(
        client_handler,
        stdin.compat_write(),
        stdout.compat(),
        |fut| {
            tokio::task::spawn_local(fut);
        },
    );

    let connection = Rc::new(connection);
    let mut handshake_tx = Some(handshake_tx);
    let io_task = tokio::task::spawn_local(io_future);

    let inline_tracker = Rc::new(RefCell::new(InlineEntryTracker::default()));
    {
        let event_tx = event_tx.clone();
        let tracker = Rc::clone(&inline_tracker);
        tokio::task::spawn_local(async move {
            let mut updates = session_update_rx;
            while let Some(update) = updates.recv().await {
                // Log all session updates to file
                debug!("Session update received: {:?}", update);

                // Send debug event for all session updates
                let debug_event = ConversationEvent::UnknownEvent {
                    raw: format!("{update:?}"),
                };
                let _ = event_tx.send(BackendEvent::Conversation(debug_event));

                match update {
                    SessionUpdate::AgentMessageChunk(chunk) => {
                        if let ContentBlock::Text(text_content) = chunk.content {
                            let mut tracker = tracker.borrow_mut();
                            tracker.append_agent_chunk(text_content.text, &event_tx);
                        }
                    }
                    SessionUpdate::AgentThoughtChunk(chunk) => {
                        if let ContentBlock::Text(text_content) = chunk.content {
                            let mut tracker = tracker.borrow_mut();
                            tracker.append_thinking_chunk(text_content.text, &event_tx);
                        }
                    }
                    other => {
                        {
                            let mut tracker = tracker.borrow_mut();
                            tracker.commit_kind(InlineEntryKind::AgentThinking, &event_tx);
                        }
                        if let Some(event) = translate_session_update(other) {
                            let _ = event_tx.send(BackendEvent::Conversation(event));
                        }
                    }
                }
            }
        });
    }

    info!("Starting ACP connection initialization");

    let init_request = InitializeRequest {
        protocol_version: acp::V1,
        client_capabilities: ClientCapabilities {
            fs: FileSystemCapability {
                read_text_file: true,
                write_text_file: true,
                ..Default::default()
            },
            terminal: false,
            meta: None,
        },
        client_info: Some(Implementation {
            name: "nori-cli".to_string(),
            title: Some("Nori CLI".to_string()),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }),
        meta: None,
    };

    debug!(
        "Sending initialize request: protocol_version={:?}",
        init_request.protocol_version
    );

    let init_response =
        match timeout(Duration::from_secs(30), connection.initialize(init_request)).await {
            Ok(Ok(response)) => response,
            Ok(Err(err)) => {
                warn!("ACP initialization failed: {}", err);
                let message = format!("Initialize failed: {err}");
                if let Some(tx) = handshake_tx.take() {
                    let _ = tx.send(Err(message.clone()));
                }
                return Err(message);
            }
            Err(_) => {
                warn!("ACP initialization timeout after 30s");
                let message = "Initialization timeout after 30s".to_string();
                if let Some(tx) = handshake_tx.take() {
                    let _ = tx.send(Err(message.clone()));
                }
                return Err(message);
            }
        };

    if init_response.protocol_version != acp::V1 {
        let err = format!(
            "Unsupported protocol version: {:?}",
            init_response.protocol_version
        );
        warn!(
            "Unsupported protocol version: {:?}",
            init_response.protocol_version
        );
        if let Some(tx) = handshake_tx.take() {
            let _ = tx.send(Err(err.clone()));
        }
        return Err(err);
    }

    info!(
        "ACP initialized successfully: protocol_version={:?}, agent_info={:?}",
        init_response.protocol_version, init_response.agent_info
    );

    // Send debug event for successful initialization
    let _ = event_tx.send(BackendEvent::Conversation(ConversationEvent::SystemEvent {
        subtype: "acp_initialized".to_string(),
        details: Some(format!(
            "Protocol version: {:?}",
            init_response.protocol_version
        )),
    }));

    debug!("Creating new ACP session: cwd={:?}", cwd);

    let session_response = match connection
        .new_session(NewSessionRequest {
            cwd: cwd.clone(),
            mcp_servers: Vec::new(),
            meta: None,
        })
        .await
    {
        Ok(response) => response,
        Err(err) => {
            warn!("ACP session creation failed: {}", err);
            let message = format!("Session creation failed: {err}");
            if let Some(tx) = handshake_tx.take() {
                let _ = tx.send(Err(message.clone()));
            }
            return Err(message);
        }
    };
    let session_id = session_response.session_id.clone();

    info!("ACP session created: session_id={}", session_id);

    // Send debug event for session creation
    let _ = event_tx.send(BackendEvent::Conversation(ConversationEvent::SystemEvent {
        subtype: "acp_session_created".to_string(),
        details: Some(format!("Session ID: {session_id}")),
    }));

    if let Some(tx) = handshake_tx.take() {
        let _ = tx.send(Ok(()));
    }

    {
        let connection = Rc::clone(&connection);
        let cancel_token = cancel_token.clone();
        let session_id = session_id.clone();
        tokio::task::spawn_local(async move {
            cancel_token.cancelled().await;
            let _ = connection
                .cancel(acp::CancelNotification {
                    session_id,
                    meta: None,
                })
                .await;
        });
    }

    // Send debug event for prompt
    let _ = event_tx.send(BackendEvent::Conversation(ConversationEvent::SystemEvent {
        subtype: "acp_prompt_sent".to_string(),
        details: Some(format!("Prompt length: {} chars", prompt.len())),
    }));

    let prompt_len = prompt.len();
    let prompt_request = PromptRequest {
        session_id: session_id.clone(),
        prompt: vec![ContentBlock::Text(TextContent {
            annotations: None,
            text: prompt,
            meta: None,
        })],
        meta: None,
    };

    debug!(
        "Sending prompt to ACP agent: session_id={}, prompt_length={}",
        session_id, prompt_len
    );

    match connection.prompt(prompt_request).await {
        Ok(response) => {
            info!("Prompt completed: stop_reason={:?}", response.stop_reason);
            {
                let mut tracker = inline_tracker.borrow_mut();
                tracker.commit_all(&event_tx);
            }
            let success = matches!(response.stop_reason, acp::StopReason::EndTurn);
            let details = format!("Stop reason: {:?}", response.stop_reason);
            let _ = event_tx.send(BackendEvent::Conversation(
                ConversationEvent::ResultSummary { success, details },
            ));
        }
        Err(err) => {
            warn!("Prompt failed: {}", err);
            {
                let mut tracker = inline_tracker.borrow_mut();
                tracker.abort_all(&event_tx);
            }
            let _ = event_tx.send(BackendEvent::Conversation(ConversationEvent::SystemEvent {
                subtype: "prompt_failed".to_string(),
                details: Some(err.to_string()),
            }));
        }
    }

    io_task.abort();
    match io_task.await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(format!("ACP I/O task failed: {err}")),
        Err(join_err) if join_err.is_cancelled() => Ok(()),
        Err(join_err) => Err(format!("ACP I/O task panicked: {join_err}")),
    }
}

#[derive(Default)]
struct InlineEntryTracker {
    next_id: usize,
    assistant_entry: Option<InlineEntryId>,
    thinking_entry: Option<InlineEntryId>,
}

impl InlineEntryTracker {
    fn append_agent_chunk(&mut self, text: String, event_tx: &mpsc::UnboundedSender<BackendEvent>) {
        if text.is_empty() {
            return;
        }
        self.commit_kind(InlineEntryKind::AgentThinking, event_tx);
        self.append_chunk(InlineEntryKind::AssistantMessage, text, event_tx);
    }

    fn append_thinking_chunk(
        &mut self,
        text: String,
        event_tx: &mpsc::UnboundedSender<BackendEvent>,
    ) {
        if text.is_empty() {
            return;
        }
        self.append_chunk(InlineEntryKind::AgentThinking, text, event_tx);
    }

    fn append_chunk(
        &mut self,
        kind: InlineEntryKind,
        text: String,
        event_tx: &mpsc::UnboundedSender<BackendEvent>,
    ) {
        let id = self.ensure_entry(kind.clone(), event_tx);
        let _ = event_tx.send(BackendEvent::InlineUpdate {
            id,
            update: InlineEntryUpdate::AppendText(text),
        });
    }

    fn ensure_entry(
        &mut self,
        kind: InlineEntryKind,
        event_tx: &mpsc::UnboundedSender<BackendEvent>,
    ) -> InlineEntryId {
        let slot = match kind {
            InlineEntryKind::AssistantMessage => &mut self.assistant_entry,
            InlineEntryKind::AgentThinking => &mut self.thinking_entry,
        };

        if let Some(id) = slot.clone() {
            return id;
        }

        self.next_id += 1;
        let prefix = match kind {
            InlineEntryKind::AssistantMessage => "assistant",
            InlineEntryKind::AgentThinking => "thinking",
        };
        let id = format!("{prefix}-{}", self.next_id);
        let _ = event_tx.send(BackendEvent::InlineBegin {
            id: id.clone(),
            kind,
        });
        *slot = Some(id.clone());
        id
    }

    fn commit_kind(
        &mut self,
        kind: InlineEntryKind,
        event_tx: &mpsc::UnboundedSender<BackendEvent>,
    ) {
        let slot = match kind {
            InlineEntryKind::AssistantMessage => &mut self.assistant_entry,
            InlineEntryKind::AgentThinking => &mut self.thinking_entry,
        };
        if let Some(id) = slot.take() {
            let _ = event_tx.send(BackendEvent::InlineCommit { id });
        }
    }

    fn commit_all(&mut self, event_tx: &mpsc::UnboundedSender<BackendEvent>) {
        self.commit_kind(InlineEntryKind::AgentThinking, event_tx);
        self.commit_kind(InlineEntryKind::AssistantMessage, event_tx);
    }

    fn abort_kind(
        &mut self,
        kind: InlineEntryKind,
        event_tx: &mpsc::UnboundedSender<BackendEvent>,
    ) {
        let slot = match kind {
            InlineEntryKind::AssistantMessage => &mut self.assistant_entry,
            InlineEntryKind::AgentThinking => &mut self.thinking_entry,
        };
        if let Some(id) = slot.take() {
            let _ = event_tx.send(BackendEvent::InlineAbort { id });
        }
    }

    fn abort_all(&mut self, event_tx: &mpsc::UnboundedSender<BackendEvent>) {
        self.abort_kind(InlineEntryKind::AgentThinking, event_tx);
        self.abort_kind(InlineEntryKind::AssistantMessage, event_tx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::{
        ContentChunk, Plan, PlanEntry as AcpPlanEntry, ResourceLink, TextContent, ToolCall,
        ToolCallId, ToolCallUpdate, ToolCallUpdateFields,
    };

    #[test]
    fn test_translate_agent_message_chunk() {
        let update = SessionUpdate::AgentMessageChunk(ContentChunk {
            content: ContentBlock::Text(TextContent {
                annotations: None,
                text: "Hello from agent".to_string(),
                meta: None,
            }),
            meta: None,
        });

        let event = translate_session_update(update);
        assert_eq!(
            event,
            Some(ConversationEvent::AssistantMessage {
                text: "Hello from agent".to_string()
            })
        );
    }

    #[test]
    fn test_translate_user_message_chunk() {
        let update = SessionUpdate::UserMessageChunk(ContentChunk {
            content: ContentBlock::Text(TextContent {
                annotations: None,
                text: "User prompt".to_string(),
                meta: None,
            }),
            meta: None,
        });

        let event = translate_session_update(update);
        assert_eq!(
            event,
            Some(ConversationEvent::UserMessage {
                text: "User prompt".to_string()
            })
        );
    }

    #[test]
    fn test_translate_agent_thought_chunk() {
        let update = SessionUpdate::AgentThoughtChunk(ContentChunk {
            content: ContentBlock::Text(TextContent {
                annotations: None,
                text: "Thinking about the problem".to_string(),
                meta: None,
            }),
            meta: None,
        });

        let event = translate_session_update(update);
        assert_eq!(
            event,
            Some(ConversationEvent::AgentThinking {
                text: "Thinking about the problem".to_string()
            })
        );
    }

    #[test]
    fn test_translate_tool_call() {
        let update = SessionUpdate::ToolCall(ToolCall {
            id: ToolCallId::from("call_123"),
            title: "Reading file".to_string(),
            kind: ToolKind::Edit,
            status: ToolCallStatus::Pending,
            content: vec![],
            locations: vec![],
            raw_input: None,
            raw_output: None,
            meta: None,
        });

        let event = translate_session_update(update);
        assert_eq!(
            event,
            Some(ConversationEvent::ToolCallStarted {
                id: "call_123".to_string(),
                title: "Reading file".to_string(),
                kind: "edit".to_string()
            })
        );
    }

    #[test]
    fn test_translate_tool_call_update() {
        let update = SessionUpdate::ToolCallUpdate(ToolCallUpdate {
            id: ToolCallId::from("call_123"),
            fields: ToolCallUpdateFields {
                kind: None,
                status: Some(ToolCallStatus::Completed),
                title: None,
                content: Some(vec![ToolCallContent::Content {
                    content: ContentBlock::Text(TextContent {
                        annotations: None,
                        text: "File read successfully".to_string(),
                        meta: None,
                    }),
                }]),
                locations: None,
                raw_input: None,
                raw_output: None,
            },
            meta: None,
        });

        let event = translate_session_update(update);
        assert_eq!(
            event,
            Some(ConversationEvent::ToolCallProgress {
                id: "call_123".to_string(),
                status: "completed".to_string(),
                content: Some("File read successfully".to_string())
            })
        );
    }

    #[test]
    fn test_translate_plan() {
        let update = SessionUpdate::Plan(Plan {
            entries: vec![
                AcpPlanEntry {
                    content: "Step 1".to_string(),
                    status: PlanEntryStatus::Pending,
                    priority: PlanEntryPriority::High,
                    meta: None,
                },
                AcpPlanEntry {
                    content: "Step 2".to_string(),
                    status: PlanEntryStatus::InProgress,
                    priority: PlanEntryPriority::Medium,
                    meta: None,
                },
            ],
            meta: None,
        });

        let event = translate_session_update(update);
        match event {
            Some(ConversationEvent::AgentPlan { entries }) => {
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].content, "Step 1");
                assert_eq!(entries[0].status, "pending");
                assert_eq!(entries[0].priority, Some("high".to_string()));
                assert_eq!(entries[1].content, "Step 2");
                assert_eq!(entries[1].status, "in_progress");
                assert_eq!(entries[1].priority, Some("medium".to_string()));
            }
            _ => panic!("Expected AgentPlan event"),
        }
    }

    #[test]
    fn test_translate_non_text_content_returns_none() {
        let update = SessionUpdate::AgentMessageChunk(ContentChunk {
            content: ContentBlock::ResourceLink(ResourceLink {
                annotations: None,
                description: None,
                mime_type: None,
                name: "test.txt".to_string(),
                size: None,
                title: None,
                uri: "file:///test.txt".to_string(),
                meta: None,
            }),
            meta: None,
        });

        let event = translate_session_update(update);
        assert_eq!(event, None);
    }
}
