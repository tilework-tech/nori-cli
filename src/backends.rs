pub mod claude;
pub mod codex;
pub mod mock;

use crate::conversation::ConversationEvent;
use futures::stream::Stream;
use std::pin::Pin;

pub trait AgentBackend {
    fn spawn_stream(
        &self,
        prompt: String,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = ConversationEvent> + Send>>;
    fn name(&self) -> &str;
    fn command_name(&self) -> &str;
    fn install_url(&self) -> &str;

    /// Returns the command to install this backend, if available
    /// Format: [command, arg1, arg2, ...]
    fn install_command(&self) -> Option<Vec<String>> {
        None
    }
}

/// Check if a command is available in PATH
pub fn is_available(command: &str) -> bool {
    which::which(command).is_ok()
}
