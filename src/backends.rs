pub mod claude;
pub mod codex;
pub mod mock;

use crate::conversation::ConversationEvent;
use futures::stream::Stream;
use std::pin::Pin;

pub trait AgentBackend {
    fn spawn_stream(&self, prompt: String)
    -> Pin<Box<dyn Stream<Item = ConversationEvent> + Send>>;
    fn name(&self) -> &str;
}

/// Check if a command is available in PATH
pub fn is_available(command: &str) -> bool {
    which::which(command).is_ok()
}
