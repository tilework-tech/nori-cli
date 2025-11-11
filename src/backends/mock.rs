use super::AgentBackend;
use crate::conversation::ConversationEvent;
use async_stream::stream;
use futures::stream::Stream;
use std::pin::Pin;

pub struct MockBackend;

impl AgentBackend for MockBackend {
    fn spawn_stream(
        &self,
        _prompt: String,
        _cancel_token: tokio_util::sync::CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = ConversationEvent> + Send>> {
        let stream = stream! {
            // Mock stream: yield predefined events
            yield ConversationEvent::AssistantMessage {
                text: "Hello from mock".to_string(),
            };
            yield ConversationEvent::AssistantMessage {
                text: "This is a test".to_string(),
            };
            yield ConversationEvent::ResultSummary {
                success: true,
                details: "Mock completed".to_string(),
            };
        };

        Box::pin(stream)
    }

    fn name(&self) -> &str {
        "Mock Backend"
    }

    fn command_name(&self) -> &str {
        "mock"
    }

    fn install_url(&self) -> &str {
        ""
    }
}
