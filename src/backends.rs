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

    /// Check if this backend is available for use
    fn is_available(&self) -> bool;
}

/// Check if a command is available in PATH
pub fn is_available(command: &str) -> bool {
    if command.contains(std::path::MAIN_SEPARATOR) || command.contains('/') {
        Path::new(command).exists()
    } else {
        which::which(command).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acp_backend_is_available_reflects_runtime_detection() {
        // ACP backends should report availability based on JavaScript runtime detection,
        // not just whether their command (npx/bunx) is in PATH
        let backend = claude_code_acp::ClaudeCodeAcpBackend::new();

        // The backend's is_available() should match whether it detected a runtime
        // This tests actual behavior: can this backend be used?
        let has_runtime = backend.is_available();

        // If we have npm or bun installed, the backend should be available
        // If not, it should be unavailable
        let expected = is_available("bun")
            || is_available("bunx")
            || is_available("npm")
            || is_available("npx");

        assert_eq!(
            has_runtime, expected,
            "ACP backend availability should reflect JavaScript runtime detection"
        );
    }

    #[test]
    fn test_claude_backend_is_available_checks_binary() {
        // ClaudeBackend should check if the 'claude' binary is in PATH
        let backend = claude::ClaudeBackend::new();

        let available = backend.is_available();
        let expected = is_available("claude");

        assert_eq!(
            available, expected,
            "Claude backend availability should match 'claude' binary presence"
        );
    }
}
