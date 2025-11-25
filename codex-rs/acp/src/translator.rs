//! Translation between ACP types and codex-protocol types
//!
//! This module provides conversion functions to bridge between the ACP
//! (Agent Client Protocol) data types and the codex internal data types.

use agent_client_protocol as acp;
use codex_protocol::models::{ContentItem, ResponseItem};

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
#[derive(Debug)]
pub enum TranslatedEvent {
    /// Text content from the agent
    TextDelta(String),
    /// Agent completed the message with a stop reason
    Completed(acp::StopReason),
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
        acp::SessionUpdate::ToolCall(_tool_call) => {
            // Tool calls are complex - for now, we just note them
            // The agent will send updates about tool execution via ToolCallUpdate
            vec![]
        }
        acp::SessionUpdate::ToolCallUpdate(_update) => {
            // Tool call results - could be mapped to function call outputs
            vec![]
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
