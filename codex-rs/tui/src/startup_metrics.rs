//! Startup performance metrics tracking for TUI launch profiling.
//!
//! This module provides instrumentation for measuring TUI startup performance,
//! specifically tracking:
//! - Time to interactive (chat window ready)
//! - Time to session header visible
//!
//! Enable with `--features startup-profiling`. Output goes to
//! `$NORI_HOME/log/startup-profile.folded` which can be visualized with:
//! ```bash
//! cargo install inferno
//! cat ~/.nori/cli/log/startup-profile.folded | inferno-flamegraph --flamechart > startup.svg
//! ```
//!
//! For tokio-console support, build with `RUSTFLAGS="--cfg tokio_unstable"`.

use std::time::{Duration, Instant};
use tracing::info;

/// Key milestone names for startup profiling.
pub mod milestones {
    pub const CONFIG_LOADED: &str = "config_loaded";
    pub const TRACING_INITIALIZED: &str = "tracing_initialized";
    pub const TERMINAL_INITIALIZED: &str = "terminal_initialized";
    pub const CHAT_INTERACTIVE: &str = "chat_interactive";
    // Note: SESSION_HEADER_VISIBLE is emitted via tracing::info! in chatwidget.rs
    // rather than through StartupMetrics::mark(), so it's not defined here.
}

/// Tracks startup timing milestones for profiling.
///
/// Create at the start of `run_main()` and call `mark()` at each milestone.
/// Call `report()` before dropping to emit a summary.
#[derive(Debug)]
pub struct StartupMetrics {
    start: Instant,
    milestones: Vec<(&'static str, Duration)>,
}

impl StartupMetrics {
    /// Create a new metrics tracker, starting the clock now.
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            milestones: Vec::new(),
        }
    }

    /// Record a named milestone with the elapsed time since start.
    pub fn mark(&mut self, name: &'static str) {
        let elapsed = self.start.elapsed();
        info!(
            target: "startup_profiling",
            milestone = name,
            elapsed_ms = elapsed.as_millis() as u64,
            "Startup milestone reached"
        );
        self.milestones.push((name, elapsed));
    }

    /// Emit a summary of all recorded milestones.
    pub fn report(&self) {
        let total = self.start.elapsed();
        info!(
            target: "startup_profiling",
            total_startup_ms = total.as_millis() as u64,
            milestone_count = self.milestones.len(),
            "Startup profiling complete"
        );

        // Log individual milestones for easy parsing
        for (name, elapsed) in &self.milestones {
            info!(
                target: "startup_profiling",
                milestone = *name,
                elapsed_ms = elapsed.as_millis() as u64,
                "Milestone timing"
            );
        }
    }

    /// Get the elapsed time since the metrics tracker was created.
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Get a slice of all recorded milestones.
    pub fn milestones(&self) -> &[(&'static str, Duration)] {
        &self.milestones
    }
}

impl Default for StartupMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn mark_records_milestone_with_elapsed_time() {
        let mut metrics = StartupMetrics::new();

        // Wait a bit to ensure elapsed time is non-zero
        thread::sleep(Duration::from_millis(10));
        metrics.mark("test_milestone");

        let milestones = metrics.milestones();
        assert_eq!(milestones.len(), 1);
        assert_eq!(milestones[0].0, "test_milestone");
        assert!(
            milestones[0].1.as_millis() >= 10,
            "Elapsed time should be at least 10ms, got {}ms",
            milestones[0].1.as_millis()
        );
    }

    #[test]
    fn mark_records_multiple_milestones_in_order() {
        let mut metrics = StartupMetrics::new();

        metrics.mark("first");
        thread::sleep(Duration::from_millis(5));
        metrics.mark("second");
        thread::sleep(Duration::from_millis(5));
        metrics.mark("third");

        let milestones = metrics.milestones();
        assert_eq!(milestones.len(), 3);
        assert_eq!(milestones[0].0, "first");
        assert_eq!(milestones[1].0, "second");
        assert_eq!(milestones[2].0, "third");

        // Each milestone should have increasing elapsed times
        assert!(milestones[1].1 > milestones[0].1);
        assert!(milestones[2].1 > milestones[1].1);
    }

    #[test]
    fn elapsed_returns_time_since_creation() {
        let metrics = StartupMetrics::new();
        thread::sleep(Duration::from_millis(10));

        let elapsed = metrics.elapsed();
        assert!(
            elapsed.as_millis() >= 10,
            "Elapsed time should be at least 10ms, got {}ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn milestones_returns_empty_slice_when_none_recorded() {
        let metrics = StartupMetrics::new();
        assert!(metrics.milestones().is_empty());
    }

    #[test]
    fn default_creates_new_metrics() {
        let metrics = StartupMetrics::default();
        assert!(metrics.milestones().is_empty());
        // elapsed should be very small since just created
        assert!(metrics.elapsed().as_secs() < 1);
    }
}
