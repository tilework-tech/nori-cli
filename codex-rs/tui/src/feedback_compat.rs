//! Compatibility layer for feedback functionality.
//!
//! When the `feedback` feature is enabled, this module re-exports types from `codex_feedback`.
//! When disabled, it provides stub implementations that compile but do nothing.
//!
//! ## Future Nori Feedback Integration
//!
//! This stub structure provides a placeholder for future Nori-specific feedback functionality.
//! When implementing Nori feedback:
//! 1. Create a new feature flag (e.g., `nori-feedback`)
//! 2. Add Nori-specific feedback implementation alongside or replacing the stub
//! 3. Track progress at: https://github.com/tilework-tech/nori-cli/issues

#[cfg(feature = "feedback")]
pub use codex_feedback::CodexFeedback;
#[cfg(feature = "feedback")]
pub use codex_feedback::CodexLogSnapshot;

#[cfg(not(feature = "feedback"))]
#[allow(dead_code)]
mod stub {
    use std::io::Write;

    /// Stub implementation of CodexFeedback when feedback feature is disabled.
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

    /// Stub implementation of CodexLogSnapshot when feedback feature is disabled.
    #[derive(Clone, Default)]
    pub struct CodexLogSnapshot {
        /// Stub thread ID field (always empty when feedback disabled).
        pub thread_id: String,
    }

    impl CodexLogSnapshot {
        /// Stub upload_feedback that does nothing when feedback feature is disabled.
        #[allow(unused_variables)]
        pub fn upload_feedback(
            &self,
            classification: &str,
            reason: Option<&str>,
            include_logs: bool,
            rollout_path: Option<&std::path::Path>,
            session_source: Option<codex_core::protocol::SessionSource>,
        ) -> anyhow::Result<()> {
            // No-op when feedback is disabled
            Ok(())
        }
    }
}

#[cfg(not(feature = "feedback"))]
#[allow(unused_imports)]
pub use stub::CodexFeedback;
#[cfg(not(feature = "feedback"))]
#[allow(unused_imports)]
pub use stub::CodexLogSnapshot;

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that CodexFeedback can be instantiated and used without panicking.
    #[test]
    fn feedback_can_be_created() {
        let feedback = CodexFeedback::new();
        let _writer_fn = feedback.make_writer();
        let _snapshot = feedback.snapshot(None);
    }

    /// Test that stub returns empty thread_id when feedback is disabled.
    #[cfg(not(feature = "feedback"))]
    #[test]
    fn stub_snapshot_has_empty_thread_id() {
        let feedback = CodexFeedback::new();
        let snapshot = feedback.snapshot(None);
        assert!(
            snapshot.thread_id.is_empty(),
            "Stub should return empty thread_id"
        );
    }

    /// Test that stub upload_feedback returns Ok when feedback is disabled.
    #[cfg(not(feature = "feedback"))]
    #[test]
    fn stub_upload_feedback_returns_ok() {
        let feedback = CodexFeedback::new();
        let snapshot = feedback.snapshot(None);
        let result = snapshot.upload_feedback("test", Some("reason"), false, None, None);
        assert!(result.is_ok(), "Stub should always return Ok");
    }

    /// Test that the stub writer accepts writes without error.
    #[cfg(not(feature = "feedback"))]
    #[test]
    fn stub_writer_accepts_writes() {
        use std::io::Write;
        let feedback = CodexFeedback::new();
        let writer_fn = feedback.make_writer();
        let mut writer = writer_fn();
        let result = writer.write(b"test data");
        assert_eq!(result.unwrap(), 9);
        assert!(writer.flush().is_ok());
    }
}
