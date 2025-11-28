//! Translation between ACP types and codex-protocol types
//!
//! This module provides conversion functions to bridge between the ACP
//! (Agent Client Protocol) data types and the codex internal data types.

use agent_client_protocol as acp;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use std::path::PathBuf;

/// Tool kind categories for ACP tool calls.
/// Maps to agent_client_protocol::ToolKind but owned by codex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpToolKind {
    Read,
    Edit,
    Delete,
    Move,
    Search,
    Execute,
    Think,
    Fetch,
    SwitchMode,
    Other,
}

impl From<&acp::ToolKind> for AcpToolKind {
    fn from(kind: &acp::ToolKind) -> Self {
        match kind {
            acp::ToolKind::Read => AcpToolKind::Read,
            acp::ToolKind::Edit => AcpToolKind::Edit,
            acp::ToolKind::Delete => AcpToolKind::Delete,
            acp::ToolKind::Move => AcpToolKind::Move,
            acp::ToolKind::Search => AcpToolKind::Search,
            acp::ToolKind::Execute => AcpToolKind::Execute,
            acp::ToolKind::Think => AcpToolKind::Think,
            acp::ToolKind::Fetch => AcpToolKind::Fetch,
            acp::ToolKind::SwitchMode => AcpToolKind::SwitchMode,
            acp::ToolKind::Other => AcpToolKind::Other,
        }
    }
}

impl std::fmt::Display for AcpToolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AcpToolKind::Read => write!(f, "read"),
            AcpToolKind::Edit => write!(f, "edit"),
            AcpToolKind::Delete => write!(f, "delete"),
            AcpToolKind::Move => write!(f, "move"),
            AcpToolKind::Search => write!(f, "search"),
            AcpToolKind::Execute => write!(f, "execute"),
            AcpToolKind::Think => write!(f, "think"),
            AcpToolKind::Fetch => write!(f, "fetch"),
            AcpToolKind::SwitchMode => write!(f, "switch_mode"),
            AcpToolKind::Other => write!(f, "other"),
        }
    }
}

/// Tool call execution status.
/// Maps to agent_client_protocol::ToolCallStatus but owned by codex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpToolStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl From<&acp::ToolCallStatus> for AcpToolStatus {
    fn from(status: &acp::ToolCallStatus) -> Self {
        match status {
            acp::ToolCallStatus::Pending => AcpToolStatus::Pending,
            acp::ToolCallStatus::InProgress => AcpToolStatus::InProgress,
            acp::ToolCallStatus::Completed => AcpToolStatus::Completed,
            acp::ToolCallStatus::Failed => AcpToolStatus::Failed,
        }
    }
}

impl std::fmt::Display for AcpToolStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AcpToolStatus::Pending => write!(f, "pending"),
            AcpToolStatus::InProgress => write!(f, "in_progress"),
            AcpToolStatus::Completed => write!(f, "completed"),
            AcpToolStatus::Failed => write!(f, "failed"),
        }
    }
}

/// Content produced by a tool call.
#[derive(Debug, Clone)]
pub enum AcpToolCallContent {
    /// Text content
    Text(String),
    /// File diff
    Diff {
        path: PathBuf,
        old_text: Option<String>,
        new_text: String,
    },
    /// Terminal reference
    Terminal { terminal_id: String },
}

/// A file location affected by a tool call.
#[derive(Debug, Clone)]
pub struct AcpToolCallLocation {
    pub path: PathBuf,
    pub line: Option<u32>,
}

/// An ACP tool call event with all relevant information.
#[derive(Debug, Clone)]
pub struct AcpToolCallEvent {
    pub call_id: String,
    pub title: String,
    pub kind: AcpToolKind,
    pub status: AcpToolStatus,
    pub content: Vec<AcpToolCallContent>,
    pub locations: Vec<AcpToolCallLocation>,
    pub raw_input: Option<serde_json::Value>,
    pub raw_output: Option<serde_json::Value>,
}

impl From<&acp::ToolCall> for AcpToolCallEvent {
    fn from(tc: &acp::ToolCall) -> Self {
        AcpToolCallEvent {
            call_id: tc.id.0.to_string(),
            title: tc.title.clone(),
            kind: AcpToolKind::from(&tc.kind),
            status: AcpToolStatus::from(&tc.status),
            content: tc.content.iter().filter_map(convert_tool_content).collect(),
            locations: tc.locations.iter().map(convert_tool_location).collect(),
            raw_input: tc.raw_input.clone(),
            raw_output: tc.raw_output.clone(),
        }
    }
}

/// An update to an existing ACP tool call.
#[derive(Debug, Clone)]
pub struct AcpToolCallUpdateEvent {
    pub call_id: String,
    pub title: Option<String>,
    pub kind: Option<AcpToolKind>,
    pub status: Option<AcpToolStatus>,
    pub content: Option<Vec<AcpToolCallContent>>,
    pub locations: Option<Vec<AcpToolCallLocation>>,
    pub raw_input: Option<serde_json::Value>,
    pub raw_output: Option<serde_json::Value>,
}

impl From<&acp::ToolCallUpdate> for AcpToolCallUpdateEvent {
    fn from(update: &acp::ToolCallUpdate) -> Self {
        let fields = &update.fields;
        AcpToolCallUpdateEvent {
            call_id: update.id.0.to_string(),
            title: fields.title.clone(),
            kind: fields.kind.as_ref().map(AcpToolKind::from),
            status: fields.status.as_ref().map(AcpToolStatus::from),
            content: fields
                .content
                .as_ref()
                .map(|c| c.iter().filter_map(convert_tool_content).collect()),
            locations: fields
                .locations
                .as_ref()
                .map(|l| l.iter().map(convert_tool_location).collect()),
            raw_input: fields.raw_input.clone(),
            raw_output: fields.raw_output.clone(),
        }
    }
}

/// Convert ACP ToolCallContent to our internal representation.
fn convert_tool_content(content: &acp::ToolCallContent) -> Option<AcpToolCallContent> {
    match content {
        acp::ToolCallContent::Content { content } => match content {
            acp::ContentBlock::Text(text) => Some(AcpToolCallContent::Text(text.text.clone())),
            _ => None, // Non-text content not yet supported
        },
        acp::ToolCallContent::Diff { diff } => Some(AcpToolCallContent::Diff {
            path: diff.path.clone(),
            old_text: diff.old_text.clone(),
            new_text: diff.new_text.clone(),
        }),
        acp::ToolCallContent::Terminal { terminal_id } => Some(AcpToolCallContent::Terminal {
            terminal_id: terminal_id.0.to_string(),
        }),
    }
}

/// Convert ACP ToolCallLocation to our internal representation.
fn convert_tool_location(loc: &acp::ToolCallLocation) -> AcpToolCallLocation {
    AcpToolCallLocation {
        path: loc.path.clone(),
        line: loc.line,
    }
}

/// Convert codex ResponseItems to ACP ContentBlocks for prompting.
///
/// This extracts text content from user messages and other response items
/// to create a list of ACP content blocks that can be sent to an agent.
pub fn response_items_to_content_blocks(items: &[ResponseItem]) -> Vec<acp::ContentBlock> {
    let mut blocks = Vec::new();

    for item in items {
        match item {
            ResponseItem::Message { role, content, .. } if role == "user" => {
                // Extract text from user messages
                for content_item in content {
                    if let ContentItem::InputText { text } = content_item {
                        blocks.push(acp::ContentBlock::Text(acp::TextContent {
                            text: text.clone(),
                            annotations: None,
                            meta: None,
                        }));
                    }
                }
            }
            ResponseItem::Message { role, content, .. } if role == "assistant" => {
                // Include assistant messages for context
                for content_item in content {
                    if let ContentItem::OutputText { text } = content_item {
                        blocks.push(acp::ContentBlock::Text(acp::TextContent {
                            text: text.clone(),
                            annotations: None,
                            meta: None,
                        }));
                    }
                }
            }
            // Other item types are typically tool results which are handled separately
            _ => {}
        }
    }

    blocks
}

/// Translate a single text string to an ACP ContentBlock.
pub fn text_to_content_block(text: &str) -> acp::ContentBlock {
    acp::ContentBlock::Text(acp::TextContent {
        text: text.to_string(),
        annotations: None,
        meta: None,
    })
}

/// Represents an event translated from an ACP SessionUpdate.
#[derive(Debug, Clone)]
pub enum TranslatedEvent {
    /// Text content from the agent
    TextDelta(String),
    /// Agent completed the message with a stop reason
    Completed(acp::StopReason),
    /// A new tool call has been initiated by the ACP agent
    ToolCall(AcpToolCallEvent),
    /// An existing tool call has been updated
    ToolCallUpdate(AcpToolCallUpdateEvent),
}

/// Translate an ACP SessionUpdate to a list of TranslatedEvents.
///
/// Some SessionUpdate variants may produce multiple events (e.g., tool calls),
/// while others may produce none (e.g., internal state updates).
pub fn translate_session_update(update: acp::SessionUpdate) -> Vec<TranslatedEvent> {
    match update {
        acp::SessionUpdate::AgentMessageChunk(chunk) => {
            // Extract text from the content block
            match chunk.content {
                acp::ContentBlock::Text(text) => {
                    vec![TranslatedEvent::TextDelta(text.text)]
                }
                acp::ContentBlock::Image(_)
                | acp::ContentBlock::Resource(_)
                | acp::ContentBlock::Audio(_)
                | acp::ContentBlock::ResourceLink(_) => {
                    // Non-text content types are not yet supported in the TUI
                    vec![]
                }
            }
        }
        acp::SessionUpdate::AgentThoughtChunk(chunk) => {
            // Thoughts are reasoning content - we could expose this as reasoning deltas
            match chunk.content {
                acp::ContentBlock::Text(text) => {
                    // For now, just treat thoughts as regular text
                    vec![TranslatedEvent::TextDelta(text.text)]
                }
                acp::ContentBlock::Image(_)
                | acp::ContentBlock::Resource(_)
                | acp::ContentBlock::Audio(_)
                | acp::ContentBlock::ResourceLink(_) => {
                    // Non-text content in thoughts is not supported
                    vec![]
                }
            }
        }
        acp::SessionUpdate::ToolCall(tool_call) => {
            // Convert ACP ToolCall to our internal representation
            vec![TranslatedEvent::ToolCall(AcpToolCallEvent::from(
                &tool_call,
            ))]
        }
        acp::SessionUpdate::ToolCallUpdate(update) => {
            // Convert ACP ToolCallUpdate to our internal representation
            vec![TranslatedEvent::ToolCallUpdate(
                AcpToolCallUpdateEvent::from(&update),
            )]
        }
        acp::SessionUpdate::Plan(_plan) => {
            // Plans are agent-internal state
            vec![]
        }
        acp::SessionUpdate::UserMessageChunk(_) => {
            // Echo of user message - typically ignored
            vec![]
        }
        acp::SessionUpdate::CurrentModeUpdate(_) => {
            // Mode changes are internal state
            vec![]
        }
        acp::SessionUpdate::AvailableCommandsUpdate(_) => {
            // Command updates are internal state
            vec![]
        }
    }
}

/// Convert a text delta to a ResponseItem::Message for codex.
pub fn text_to_message_response_item(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: text.to_string(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_to_content_block() {
        let block = text_to_content_block("Hello, world!");
        match block {
            acp::ContentBlock::Text(text) => {
                assert_eq!(text.text, "Hello, world!");
            }
            _ => panic!("Expected text block"),
        }
    }

    #[test]
    fn test_translate_agent_message_chunk() {
        let update = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk {
            content: acp::ContentBlock::Text(acp::TextContent {
                text: "Test response".to_string(),
                annotations: None,
                meta: None,
            }),
            meta: None,
        });

        let events = translate_session_update(update);
        assert_eq!(events.len(), 1);
        match &events[0] {
            TranslatedEvent::TextDelta(text) => {
                assert_eq!(text, "Test response");
            }
            _ => panic!("Expected TextDelta"),
        }
    }

    #[test]
    fn test_response_items_to_content_blocks() {
        let items = vec![
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "Hello".to_string(),
                }],
            },
            ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "Hi there".to_string(),
                }],
            },
        ];

        let blocks = response_items_to_content_blocks(&items);
        assert_eq!(blocks.len(), 2);

        match &blocks[0] {
            acp::ContentBlock::Text(text) => assert_eq!(text.text, "Hello"),
            _ => panic!("Expected text block"),
        }

        match &blocks[1] {
            acp::ContentBlock::Text(text) => assert_eq!(text.text, "Hi there"),
            _ => panic!("Expected text block"),
        }
    }
}
