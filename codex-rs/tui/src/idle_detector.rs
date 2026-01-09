//! Idle detection for ACP backend notifications.
//!
//! Tracks the last activity timestamp and determines when the system
//! has been idle long enough to warrant a notification.

use std::time::Duration;
use std::time::Instant;

/// Tracks idle state for notification purposes.
///
/// The detector tracks the last activity time and can determine if
/// the system has been idle for longer than a configured threshold.
/// It also ensures that only one notification is sent per idle period.
pub struct IdleDetector {
    last_activity: Instant,
    threshold: Duration,
    notified_for_current_idle: bool,
}

impl IdleDetector {
    /// Create a new idle detector with the given threshold.
    ///
    /// The threshold determines how long the system must be idle
    /// before `check_idle` returns a duration.
    pub fn new(threshold: Duration) -> Self {
        Self {
            last_activity: Instant::now(),
            threshold,
            notified_for_current_idle: false,
        }
    }

    /// Record that activity occurred, resetting the idle timer.
    ///
    /// This should be called whenever an event is received from
    /// the ACP backend (e.g., agent message, tool execution, etc.)
    pub fn record_activity(&mut self) {
        self.last_activity = Instant::now();
        self.notified_for_current_idle = false;
    }

    /// Check if the system has been idle for longer than the threshold.
    ///
    /// Returns `Some(duration)` if:
    /// - The system has been idle for longer than the threshold
    /// - A notification has not already been sent for this idle period
    ///
    /// Returns `None` if:
    /// - The system is not idle (activity occurred recently)
    /// - A notification was already sent for this idle period
    ///
    /// Calling this method when it returns `Some` will mark the idle
    /// period as notified, so subsequent calls will return `None`
    /// until `record_activity` is called.
    pub fn check_idle(&mut self) -> Option<Duration> {
        if self.notified_for_current_idle {
            return None;
        }

        let elapsed = self.last_activity.elapsed();
        if elapsed >= self.threshold {
            self.notified_for_current_idle = true;
            Some(elapsed)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn new_detector_is_not_idle() {
        let mut detector = IdleDetector::new(Duration::from_millis(100));
        // Immediately after creation, should not be idle
        assert!(detector.check_idle().is_none());
    }

    #[test]
    fn detector_becomes_idle_after_threshold() {
        let mut detector = IdleDetector::new(Duration::from_millis(50));

        // Wait for threshold to pass
        thread::sleep(Duration::from_millis(60));

        // Should now be idle
        let idle_duration = detector.check_idle();
        assert!(idle_duration.is_some());
        assert!(idle_duration.unwrap() >= Duration::from_millis(50));
    }

    #[test]
    fn check_idle_only_returns_once_per_idle_period() {
        let mut detector = IdleDetector::new(Duration::from_millis(50));

        thread::sleep(Duration::from_millis(60));

        // First check returns Some
        assert!(detector.check_idle().is_some());

        // Second check returns None (already notified)
        assert!(detector.check_idle().is_none());
    }

    #[test]
    fn record_activity_resets_idle_state() {
        let mut detector = IdleDetector::new(Duration::from_millis(50));

        thread::sleep(Duration::from_millis(60));

        // Consume the idle notification
        assert!(detector.check_idle().is_some());

        // Record activity
        detector.record_activity();

        // Should not be idle anymore
        assert!(detector.check_idle().is_none());

        // Wait for threshold again
        thread::sleep(Duration::from_millis(60));

        // Should be idle again
        assert!(detector.check_idle().is_some());
    }
}
