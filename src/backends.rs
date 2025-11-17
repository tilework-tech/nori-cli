pub mod claude;
pub mod claude_code_acp;
pub mod codex_acp;
pub mod gemini_acp;
pub mod javascript_runtime;
pub mod mock;

use crate::conversation::ConversationEvent;
use crate::history::{InlineEntryId, InlineEntryKind, InlineEntryUpdate};
use futures::stream::Stream;
use std::path::Path;
use std::pin::Pin;

#[derive(Debug, Clone)]
pub enum BackendEvent {
    Conversation(ConversationEvent),
    InlineBegin {
        id: InlineEntryId,
        kind: InlineEntryKind,
    },
    InlineUpdate {
        id: InlineEntryId,
        update: InlineEntryUpdate,
    },
    InlineCommit {
        id: InlineEntryId,
    },
    InlineAbort {
        id: InlineEntryId,
    },
}

pub trait AgentBackend {
    fn spawn_stream(
        &self,
        prompt: String,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = BackendEvent> + Send>>;
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
    if command.contains(std::path::MAIN_SEPARATOR) || command.contains('/') {
        Path::new(command).exists()
    } else {
        which::which(command).is_ok()
    }
}
