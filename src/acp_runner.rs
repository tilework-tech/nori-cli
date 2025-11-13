#![allow(dead_code)]

use crate::conversation::{ConversationEvent, PlanEntry};
use agent_client_protocol::{
    Client, ContentBlock, PermissionOptionKind, PlanEntryPriority, PlanEntryStatus,
    ReadTextFileRequest, ReadTextFileResponse, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, Result as AcpResult, SessionNotification, SessionUpdate,
    ToolCallContent, ToolCallStatus, ToolKind, WriteTextFileRequest, WriteTextFileResponse,
};
use futures::stream::Stream;
use std::path::PathBuf;
use std::pin::Pin;
use tokio::process::Child;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

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
            Err(_e) => Err(agent_client_protocol::Error::internal_error()),
        }
    }

    async fn write_text_file(
        &self,
        args: WriteTextFileRequest,
    ) -> AcpResult<WriteTextFileResponse> {
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
            return Err(agent_client_protocol::Error::internal_error());
        }

        // Write the file
        match tokio::fs::write(&canonical_path, &args.content).await {
            Ok(_) => Ok(WriteTextFileResponse { meta: None }),
            Err(_e) => Err(agent_client_protocol::Error::internal_error()),
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
        _prompt: String,
        _cancel_token: CancellationToken,
    ) -> Result<Pin<Box<dyn Stream<Item = ConversationEvent> + Send>>, String> {
        // TODO: Implement actual ACP protocol flow
        Err("Not implemented".to_string())
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
