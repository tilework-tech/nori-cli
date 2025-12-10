//! Compatibility layer for feedback functionality.
//!
//! When the `sentry` feature is enabled, this module re-exports types from `codex_feedback`.
//! When disabled, it provides stub implementations that compile but do nothing.

#[cfg(feature = "sentry")]
pub use codex_feedback::CodexFeedback;
#[cfg(feature = "sentry")]
pub use codex_feedback::CodexLogSnapshot;

#[cfg(not(feature = "sentry"))]
mod stub {
    use std::io::Write;

    /// Stub implementation of CodexFeedback when sentry is disabled.
    #[derive(Clone, Default)]
    pub struct CodexFeedback;

    impl CodexFeedback {
        pub fn new() -> Self {
            Self
        }

        pub fn make_writer(&self) -> impl Fn() -> StubWriter + Send + Sync + 'static {
            || StubWriter
        }

        pub fn snapshot(
            &self,
            _session_id: Option<codex_protocol::ConversationId>,
        ) -> CodexLogSnapshot {
            CodexLogSnapshot {
                thread_id: String::new(),
            }
        }
    }

    /// Stub writer that discards all output.
    pub struct StubWriter;

    impl Write for StubWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Stub implementation of CodexLogSnapshot when sentry is disabled.
    #[derive(Clone, Default)]
    pub struct CodexLogSnapshot {
        /// Stub thread ID field (always empty when sentry disabled).
        pub thread_id: String,
    }

    impl CodexLogSnapshot {
        /// Stub upload_feedback that does nothing when sentry is disabled.
        #[allow(unused_variables)]
        pub fn upload_feedback(
            &self,
            classification: &str,
            reason: Option<&str>,
            include_logs: bool,
            rollout_path: Option<&std::path::Path>,
            session_source: Option<codex_core::protocol::SessionSource>,
        ) -> Result<(), String> {
            // No-op when sentry is disabled
            Ok(())
        }
    }
}

#[cfg(not(feature = "sentry"))]
pub use stub::CodexFeedback;
#[cfg(not(feature = "sentry"))]
pub use stub::CodexLogSnapshot;
