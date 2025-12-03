//! Mock ACP agent for testing nori-cli

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use agent_client_protocol::Client as _;
use agent_client_protocol::{self as acp};
use serde_json::json;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::time::Duration;
use tokio::time::sleep;
use tokio_util::compat::TokioAsyncReadCompatExt as _;
use tokio_util::compat::TokioAsyncWriteCompatExt as _;

enum MockClientRequest {
    ReadFile {
        session_id: acp::SessionId,
        path: PathBuf,
        responder: oneshot::Sender<Result<String, acp::Error>>,
    },
    RequestPermission {
        session_id: acp::SessionId,
        tool_call: acp::ToolCallUpdate,
        options: Vec<acp::PermissionOption>,
        responder: oneshot::Sender<Result<acp::RequestPermissionResponse, acp::Error>>,
    },
}

struct MockAgent {
    session_update_tx: mpsc::UnboundedSender<(acp::SessionNotification, oneshot::Sender<()>)>,
    client_request_tx: mpsc::UnboundedSender<MockClientRequest>,
    next_session_id: Cell<u64>,
    cancel_requested: Cell<bool>,
}

impl MockAgent {
    fn new(
        session_update_tx: mpsc::UnboundedSender<(acp::SessionNotification, oneshot::Sender<()>)>,
        client_request_tx: mpsc::UnboundedSender<MockClientRequest>,
    ) -> Self {
        Self {
            session_update_tx,
            next_session_id: Cell::new(0),
            client_request_tx,
            cancel_requested: Cell::new(false),
        }
    }

    async fn send_update(
        &self,
        session_id: acp::SessionId,
        update: acp::SessionUpdate,
    ) -> Result<(), acp::Error> {
        let (tx, rx) = oneshot::channel();
        self.session_update_tx
            .send((
                acp::SessionNotification {
                    session_id,
                    update,
                    meta: None,
                },
                tx,
            ))
            .map_err(|_| acp::Error::internal_error())?;
        rx.await.map_err(|_| acp::Error::internal_error())?;
        Ok(())
    }

    async fn send_text_chunk(
        &self,
        session_id: acp::SessionId,
        text: &str,
    ) -> Result<(), acp::Error> {
        self.send_update(
            session_id,
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk {
                content: acp::ContentBlock::Text(acp::TextContent {
                    annotations: None,
                    text: text.to_string(),
                    meta: None,
                }),
                meta: None,
            }),
        )
        .await
    }

    /// Send a tool call notification
    async fn send_tool_call(
        &self,
        session_id: acp::SessionId,
        tool_call: acp::ToolCall,
    ) -> Result<(), acp::Error> {
        self.send_update(session_id, acp::SessionUpdate::ToolCall(tool_call))
            .await
    }

    /// Send a tool call update notification
    async fn send_tool_call_update(
        &self,
        session_id: acp::SessionId,
        update: acp::ToolCallUpdate,
    ) -> Result<(), acp::Error> {
        self.send_update(session_id, acp::SessionUpdate::ToolCallUpdate(update))
            .await
    }

    async fn read_file_via_client(
        &self,
        session_id: acp::SessionId,
        path: PathBuf,
    ) -> Result<String, acp::Error> {
        let (tx, rx) = oneshot::channel();
        self.client_request_tx
            .send(MockClientRequest::ReadFile {
                session_id,
                path,
                responder: tx,
            })
            .map_err(|_| acp::Error::internal_error())?;
        rx.await.map_err(|_| acp::Error::internal_error())?
    }

    /// Request permission from the client for a tool call
    async fn request_permission_via_client(
        &self,
        session_id: acp::SessionId,
        tool_call: acp::ToolCallUpdate,
        options: Vec<acp::PermissionOption>,
    ) -> Result<acp::RequestPermissionResponse, acp::Error> {
        let (tx, rx) = oneshot::channel();
        self.client_request_tx
            .send(MockClientRequest::RequestPermission {
                session_id,
                tool_call,
                options,
                responder: tx,
            })
            .map_err(|_| acp::Error::internal_error())?;
        rx.await.map_err(|_| acp::Error::internal_error())?
    }
}

#[async_trait::async_trait(?Send)]
impl acp::Agent for MockAgent {
    async fn initialize(
        &self,
        _arguments: acp::InitializeRequest,
    ) -> Result<acp::InitializeResponse, acp::Error> {
        if std::env::var("MOCK_AGENT_HANG").is_ok() {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        }

        eprintln!("Mock agent: initialize");
        Ok(acp::InitializeResponse {
            protocol_version: acp::V1,
            agent_capabilities: acp::AgentCapabilities::default(),
            auth_methods: Vec::new(),
            agent_info: Some(acp::Implementation {
                name: "mock-agent".to_string(),
                title: Some("Mock Agent".to_string()),
                version: "0.1.0".to_string(),
            }),
            meta: None,
        })
    }

    async fn authenticate(
        &self,
        _arguments: acp::AuthenticateRequest,
    ) -> Result<acp::AuthenticateResponse, acp::Error> {
        Ok(acp::AuthenticateResponse::default())
    }

    async fn new_session(
        &self,
        _arguments: acp::NewSessionRequest,
    ) -> Result<acp::NewSessionResponse, acp::Error> {
        let session_id = self.next_session_id.get();
        self.next_session_id.set(session_id + 1);
        eprintln!("Mock agent: new_session id={}", session_id);
        Ok(acp::NewSessionResponse {
            session_id: acp::SessionId(session_id.to_string().into()),
            modes: None,
            meta: None,
        })
    }

    async fn load_session(
        &self,
        _arguments: acp::LoadSessionRequest,
    ) -> Result<acp::LoadSessionResponse, acp::Error> {
        Ok(acp::LoadSessionResponse {
            modes: None,
            meta: None,
        })
    }

    async fn prompt(
        &self,
        arguments: acp::PromptRequest,
    ) -> Result<acp::PromptResponse, acp::Error> {
        eprintln!("Mock agent: prompt");
        self.cancel_requested.set(false);
        let session_id = arguments.session_id.clone();

        // Support configurable stderr output for testing stderr capture
        if let Ok(count_str) = std::env::var("MOCK_AGENT_STDERR_COUNT")
            && let Ok(count) = count_str.parse::<usize>()
        {
            for i in 0..count {
                eprintln!("MOCK_AGENT_STDERR_LINE:{}", i);
            }
        }

        // Support custom response text for TUI testing
        if let Ok(response) = std::env::var("MOCK_AGENT_RESPONSE") {
            self.send_text_chunk(session_id.clone(), &response).await?;
        } else {
            // Default behavior
            self.send_text_chunk(session_id.clone(), "Test message 1")
                .await?;

            self.send_text_chunk(session_id.clone(), "Test message 2")
                .await?;
        }

        // Support configurable delay for simulating realistic streaming
        if let Ok(delay_str) = std::env::var("MOCK_AGENT_DELAY_MS")
            && let Ok(delay) = delay_str.parse::<u64>()
        {
            sleep(Duration::from_millis(delay)).await;
        }

        // Support requesting permission from client for testing approval bridging
        if std::env::var("MOCK_AGENT_REQUEST_PERMISSION").is_ok() {
            eprintln!("Mock agent: requesting permission from client");

            // Create a tool call update describing the operation
            let tool_call_id = acp::ToolCallId("permission-test-001".to_string().into());
            let tool_call = acp::ToolCallUpdate {
                id: tool_call_id,
                fields: acp::ToolCallUpdateFields {
                    title: Some("Execute shell command".to_string()),
                    kind: Some(acp::ToolKind::Execute),
                    status: Some(acp::ToolCallStatus::Pending),
                    content: Some(vec![acp::ToolCallContent::Content {
                        content: acp::ContentBlock::Text(acp::TextContent {
                            text: "echo 'Hello from permission test'".to_string(),
                            annotations: None,
                            meta: None,
                        }),
                    }]),
                    locations: None,
                    raw_input: Some(
                        json!({"command": "echo", "args": ["Hello from permission test"]}),
                    ),
                    raw_output: None,
                },
                meta: None,
            };

            // Create permission options: allow once and reject once
            let options = vec![
                acp::PermissionOption {
                    id: acp::PermissionOptionId("allow".into()),
                    name: "Allow".to_string(),
                    kind: acp::PermissionOptionKind::AllowOnce,
                    meta: None,
                },
                acp::PermissionOption {
                    id: acp::PermissionOptionId("reject".into()),
                    name: "Reject".to_string(),
                    kind: acp::PermissionOptionKind::RejectOnce,
                    meta: None,
                },
            ];

            // Request permission from client
            match self
                .request_permission_via_client(session_id.clone(), tool_call, options)
                .await
            {
                Ok(response) => {
                    eprintln!(
                        "Mock agent: permission response received: {:?}",
                        response.outcome
                    );
                    match response.outcome {
                        acp::RequestPermissionOutcome::Selected { option_id, .. } => {
                            let msg = format!("Permission granted with option: {}", option_id);
                            self.send_text_chunk(session_id.clone(), &msg).await?;
                        }
                        _ => {
                            // Handles Cancelled and any future variants
                            self.send_text_chunk(
                                session_id.clone(),
                                "Permission request was cancelled",
                            )
                            .await?;
                        }
                    }
                }
                Err(err) => {
                    eprintln!("Mock agent: permission request failed: {}", err);
                    self.send_text_chunk(session_id.clone(), "Permission request failed")
                        .await?;
                }
            }
        }

        // Support sending tool calls for testing ACP tool call display
        if std::env::var("MOCK_AGENT_SEND_TOOL_CALL").is_ok() {
            eprintln!("Mock agent: sending tool call sequence");

            // Send initial tool call with pending status
            let tool_call_id = acp::ToolCallId("test-tool-call-001".to_string().into());
            self.send_tool_call(
                session_id.clone(),
                acp::ToolCall {
                    id: tool_call_id.clone(),
                    title: "Reading configuration file".to_string(),
                    kind: acp::ToolKind::Read,
                    status: acp::ToolCallStatus::Pending,
                    content: vec![],
                    locations: vec![],
                    raw_input: Some(json!({"path": "/etc/config.toml"})),
                    raw_output: None,
                    meta: None,
                },
            )
            .await?;

            // Small delay to simulate execution time
            sleep(Duration::from_millis(50)).await;

            // Send update to in_progress
            self.send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate {
                    id: tool_call_id.clone(),
                    fields: acp::ToolCallUpdateFields {
                        title: None,
                        kind: None,
                        status: Some(acp::ToolCallStatus::InProgress),
                        content: None,
                        locations: None,
                        raw_input: None,
                        raw_output: None,
                    },
                    meta: None,
                },
            )
            .await?;

            // Small delay
            sleep(Duration::from_millis(50)).await;

            // Send update to completed with content
            self.send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate {
                    id: tool_call_id.clone(),
                    fields: acp::ToolCallUpdateFields {
                        title: None,
                        kind: None,
                        status: Some(acp::ToolCallStatus::Completed),
                        content: Some(vec![acp::ToolCallContent::Content {
                            content: acp::ContentBlock::Text(acp::TextContent {
                                text: "Configuration loaded successfully".to_string(),
                                annotations: None,
                                meta: None,
                            }),
                        }]),
                        locations: None,
                        raw_input: None,
                        raw_output: Some(json!({"success": true, "lines": 42})),
                    },
                    meta: None,
                },
            )
            .await?;

            // Send text message after tool call
            self.send_text_chunk(session_id.clone(), "Tool call completed successfully.")
                .await?;
        }

        if let Ok(file_path) = std::env::var("MOCK_AGENT_REQUEST_FILE") {
            eprintln!("Mock agent: requesting file read: {}", file_path);
            match self
                .read_file_via_client(session_id.clone(), PathBuf::from(&file_path))
                .await
            {
                Ok(content) => {
                    let msg = format!("Read file content: {content}");
                    self.send_text_chunk(session_id.clone(), &msg).await?;
                }
                Err(err) => {
                    self.send_text_chunk(
                        session_id.clone(),
                        "Failed to read file content via client",
                    )
                    .await?;
                    return Err(err);
                }
            }
        }

        if std::env::var("MOCK_AGENT_STREAM_UNTIL_CANCEL").is_ok() {
            let mut iterations = 0usize;
            while !self.cancel_requested.get() && iterations < 10_000 {
                self.send_text_chunk(session_id.clone(), "Streaming...")
                    .await?;
                iterations += 1;
                sleep(Duration::from_millis(10)).await;
            }

            return Ok(acp::PromptResponse {
                stop_reason: if self.cancel_requested.get() {
                    acp::StopReason::Cancelled
                } else {
                    acp::StopReason::EndTurn
                },
                meta: None,
            });
        }

        Ok(acp::PromptResponse {
            stop_reason: acp::StopReason::EndTurn,
            meta: None,
        })
    }

    async fn cancel(&self, _args: acp::CancelNotification) -> Result<(), acp::Error> {
        eprintln!("Mock agent: cancel");
        self.cancel_requested.set(true);
        Ok(())
    }

    async fn set_session_mode(
        &self,
        _args: acp::SetSessionModeRequest,
    ) -> Result<acp::SetSessionModeResponse, acp::Error> {
        Ok(acp::SetSessionModeResponse::default())
    }

    async fn ext_method(&self, _args: acp::ExtRequest) -> Result<acp::ExtResponse, acp::Error> {
        Ok(serde_json::value::to_raw_value(&json!({}))?.into())
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> Result<(), acp::Error> {
        Ok(())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> acp::Result<()> {
    env_logger::init();

    let outgoing = tokio::io::stdout().compat_write();
    let incoming = tokio::io::stdin().compat();

    let local_set = tokio::task::LocalSet::new();
    local_set
        .run_until(async move {
            let (update_tx, mut update_rx) = tokio::sync::mpsc::unbounded_channel();
            let (client_request_tx, mut client_request_rx) = tokio::sync::mpsc::unbounded_channel();

            let agent = MockAgent::new(update_tx, client_request_tx);
            let (conn, handle_io) =
                acp::AgentSideConnection::new(agent, outgoing, incoming, |fut| {
                    tokio::task::spawn_local(fut);
                });

            let conn = Rc::new(conn);

            {
                let conn = Rc::clone(&conn);
                tokio::task::spawn_local(async move {
                    while let Some((session_notification, tx)) = update_rx.recv().await {
                        if let Err(e) = conn.session_notification(session_notification).await {
                            eprintln!("Mock agent error: {e}");
                            break;
                        }
                        tx.send(()).ok();
                    }
                });
            }

            {
                let conn = Rc::clone(&conn);
                tokio::task::spawn_local(async move {
                    while let Some(request) = client_request_rx.recv().await {
                        match request {
                            MockClientRequest::ReadFile {
                                session_id,
                                path,
                                responder,
                            } => {
                                let result = conn
                                    .read_text_file(acp::ReadTextFileRequest {
                                        session_id,
                                        path,
                                        line: None,
                                        limit: None,
                                        meta: None,
                                    })
                                    .await
                                    .map(|response| response.content);
                                let _ = responder.send(result);
                            }
                            MockClientRequest::RequestPermission {
                                session_id,
                                tool_call,
                                options,
                                responder,
                            } => {
                                let result = conn
                                    .request_permission(acp::RequestPermissionRequest {
                                        session_id,
                                        tool_call,
                                        options,
                                        meta: None,
                                    })
                                    .await;
                                let _ = responder.send(result);
                            }
                        }
                    }
                });
            }

            handle_io.await
        })
        .await
}
