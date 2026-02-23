//! Tracks the effective current working directory based on tool call locations.
//!
//! The ACP protocol sets CWD at session creation and it is immutable. However,
//! when the agent works in different directories (e.g., git worktrees), the TUI
//! footer should reflect the current working context. This module tracks the
//! "effective" CWD by monitoring tool call locations and debouncing updates.

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

/// Find the git repository root for a given path by walking up the directory tree.
///
/// Returns the directory containing `.git` (either as a directory or a file,
/// since git worktrees use a `.git` file pointing to the main repository).
///
/// Returns `None` if no git root is found or if the starting path doesn't exist.
pub(crate) fn find_git_root(start: &Path) -> Option<PathBuf> {
    // Start from the directory containing the path (or the path itself if it's a directory)
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent()?.to_path_buf()
    };

    loop {
        let git_marker = current.join(".git");
        // .git can be a directory (regular repo) or a file (worktree)
        if git_marker.is_dir() || git_marker.is_file() {
            return Some(current);
        }

        // Move to parent directory
        match current.parent() {
            Some(parent) if parent != current => {
                current = parent.to_path_buf();
            }
            _ => return None, // Reached filesystem root
        }
    }
}

/// Debounce threshold for CWD updates - only update if the same directory
/// is observed consistently for this duration.
const DEBOUNCE_THRESHOLD: Duration = Duration::from_millis(500);

/// Tracks the effective current working directory based on tool call locations.
///
/// The tracker uses a debounce mechanism to avoid flickering when tool calls
/// happen in different directories in quick succession. A new directory must
/// be consistently observed for at least `DEBOUNCE_THRESHOLD` before being
/// promoted to the effective CWD.
#[derive(Debug)]
pub(crate) struct EffectiveCwdTracker {
    /// The currently confirmed effective CWD.
    effective_cwd: Option<PathBuf>,

    /// A candidate directory that is being observed but not yet confirmed.
    candidate_cwd: Option<PathBuf>,

    /// When the candidate directory was first observed.
    candidate_first_seen: Option<Instant>,
}

impl Default for EffectiveCwdTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl EffectiveCwdTracker {
    /// Creates a new tracker with no initial CWD.
    pub(crate) fn new() -> Self {
        Self {
            effective_cwd: None,
            candidate_cwd: None,
            candidate_first_seen: None,
        }
    }

    /// Creates a new tracker with an initial CWD.
    pub(crate) fn with_initial_cwd(cwd: PathBuf) -> Self {
        Self {
            effective_cwd: Some(cwd),
            candidate_cwd: None,
            candidate_first_seen: None,
        }
    }

    /// Returns the current effective CWD, if any.
    #[allow(dead_code)]
    pub(crate) fn effective_cwd(&self) -> Option<&PathBuf> {
        self.effective_cwd.as_ref()
    }

    /// Updates the tracker with an observed directory from a tool call.
    ///
    /// Returns `true` if the effective CWD changed as a result of this update.
    pub(crate) fn observe_directory(&mut self, dir: PathBuf) -> bool {
        self.observe_directory_at(dir, Instant::now())
    }

    /// Updates the tracker with an observed file path from a tool call.
    /// Extracts the parent directory and observes it.
    ///
    /// Returns `true` if the effective CWD changed as a result of this update.
    /// Returns `false` if the file path has no parent directory.
    pub(crate) fn observe_file_path(&mut self, file_path: &Path) -> bool {
        self.observe_file_path_at(file_path, Instant::now())
    }

    /// Updates the tracker with an observed file path at a specific time.
    /// This variant is primarily for testing.
    fn observe_file_path_at(&mut self, file_path: &Path, now: Instant) -> bool {
        if let Some(parent) = file_path.parent() {
            // Only observe if parent is non-empty (i.e., not a bare filename)
            if !parent.as_os_str().is_empty() {
                return self.observe_directory_at(parent.to_path_buf(), now);
            }
        }
        false
    }

    /// Updates the tracker with an observed directory at a specific time.
    /// This variant is primarily for testing.
    fn observe_directory_at(&mut self, dir: PathBuf, now: Instant) -> bool {
        // Reject empty directory paths - these are invalid and would cause
        // git commands to fail, clearing the footer info
        if dir.as_os_str().is_empty() {
            return false;
        }

        tracing::debug!(
            target: "system_info",
            observed_dir = ?dir,
            effective_cwd = ?self.effective_cwd,
            candidate_cwd = ?self.candidate_cwd,
            "observe_directory: processing observed directory"
        );

        // If this matches the current effective CWD, nothing to do
        if self.effective_cwd.as_ref() == Some(&dir) {
            // Clear any pending candidate since we're back to the effective CWD
            self.candidate_cwd = None;
            self.candidate_first_seen = None;
            return false;
        }

        // Check if this matches the current candidate
        if self.candidate_cwd.as_ref() == Some(&dir) {
            // Check if debounce threshold has passed
            if let Some(first_seen) = self.candidate_first_seen
                && now.duration_since(first_seen) >= DEBOUNCE_THRESHOLD
            {
                // Promote candidate to effective CWD
                self.effective_cwd = Some(dir);
                self.candidate_cwd = None;
                self.candidate_first_seen = None;
                return true;
            }
            // Still waiting for debounce threshold
            return false;
        }

        // New candidate directory - start tracking it
        self.candidate_cwd = Some(dir);
        self.candidate_first_seen = Some(now);
        false
    }

    /// Resets the tracker to a new CWD, clearing any pending candidate.
    /// This is useful when starting a new session or when the user explicitly
    /// changes the working directory.
    #[allow(dead_code)]
    pub(crate) fn reset(&mut self, cwd: Option<PathBuf>) {
        self.effective_cwd = cwd;
        self.candidate_cwd = None;
        self.candidate_first_seen = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_find_git_root_from_subdirectory() {
        // This test runs in the actual git repo
        let current_dir = std::env::current_dir().expect("should have current dir");
        let git_root = find_git_root(&current_dir);

        // Should find a git root
        assert!(git_root.is_some(), "should find git root from current dir");

        let root = git_root.unwrap();
        // The root should contain a .git directory or file
        let git_marker = root.join(".git");
        assert!(
            git_marker.is_dir() || git_marker.is_file(),
            ".git should exist at root"
        );
    }

    #[test]
    fn test_find_git_root_from_file_path() {
        // Test with a file path (not just a directory)
        let current_dir = std::env::current_dir().expect("should have current dir");
        let file_path = current_dir.join("some_file.rs");

        let git_root = find_git_root(&file_path);
        assert!(git_root.is_some(), "should find git root from file path");
    }

    #[test]
    fn test_find_git_root_non_git_directory() {
        // /tmp is typically not a git repository
        let temp_dir = std::env::temp_dir();
        let git_root = find_git_root(&temp_dir);

        // Should return None for non-git directories
        assert!(
            git_root.is_none(),
            "should not find git root in temp directory"
        );
    }

    #[test]
    fn test_find_git_root_nonexistent_path() {
        // A path that doesn't exist
        let nonexistent = PathBuf::from("/nonexistent/path/that/does/not/exist");
        let git_root = find_git_root(&nonexistent);

        // Should return None for nonexistent paths
        assert!(
            git_root.is_none(),
            "should not find git root for nonexistent path"
        );
    }

    #[test]
    fn test_new_tracker_has_no_effective_cwd() {
        let tracker = EffectiveCwdTracker::new();
        assert!(tracker.effective_cwd().is_none());
    }

    #[test]
    fn test_tracker_with_initial_cwd() {
        let initial = PathBuf::from("/home/user/project");
        let tracker = EffectiveCwdTracker::with_initial_cwd(initial.clone());
        assert_eq!(tracker.effective_cwd(), Some(&initial));
    }

    #[test]
    fn test_observe_same_directory_as_effective_returns_false() {
        let initial = PathBuf::from("/home/user/project");
        let mut tracker = EffectiveCwdTracker::with_initial_cwd(initial.clone());

        let changed = tracker.observe_directory(initial);
        assert!(!changed);
    }

    #[test]
    fn test_observe_new_directory_starts_candidate() {
        let initial = PathBuf::from("/home/user/project");
        let mut tracker = EffectiveCwdTracker::with_initial_cwd(initial);

        let new_dir = PathBuf::from("/home/user/worktree");
        let changed = tracker.observe_directory(new_dir.clone());

        // Should not change yet - debounce threshold not met
        assert!(!changed);
        assert_ne!(tracker.effective_cwd(), Some(&new_dir));
    }

    #[test]
    fn test_observe_directory_after_debounce_threshold() {
        let initial = PathBuf::from("/home/user/project");
        let mut tracker = EffectiveCwdTracker::with_initial_cwd(initial);

        let new_dir = PathBuf::from("/home/user/worktree");
        let start = Instant::now();

        // First observation - starts the candidate
        let changed = tracker.observe_directory_at(new_dir.clone(), start);
        assert!(!changed);

        // Second observation before threshold - should not change
        let before_threshold = start + Duration::from_millis(400);
        let changed = tracker.observe_directory_at(new_dir.clone(), before_threshold);
        assert!(!changed);

        // Third observation after threshold - should change
        let after_threshold = start + Duration::from_millis(600);
        let changed = tracker.observe_directory_at(new_dir.clone(), after_threshold);
        assert!(changed);
        assert_eq!(tracker.effective_cwd(), Some(&new_dir));
    }

    #[test]
    fn test_observe_different_directory_resets_candidate() {
        let initial = PathBuf::from("/home/user/project");
        let mut tracker = EffectiveCwdTracker::with_initial_cwd(initial);

        let dir1 = PathBuf::from("/home/user/worktree1");
        let dir2 = PathBuf::from("/home/user/worktree2");
        let start = Instant::now();

        // First observation of dir1
        tracker.observe_directory_at(dir1.clone(), start);

        // Observation of dir2 before threshold - resets candidate to dir2
        let before_threshold = start + Duration::from_millis(400);
        tracker.observe_directory_at(dir2, before_threshold);

        // Observation of dir1 after original threshold would have passed
        // Should not promote dir1 because we switched to dir2
        let after_threshold = start + Duration::from_millis(600);
        let changed = tracker.observe_directory_at(dir1, after_threshold);
        assert!(!changed); // dir1 was reset, so it's a new candidate now
    }

    #[test]
    fn test_observe_effective_cwd_clears_candidate() {
        let initial = PathBuf::from("/home/user/project");
        let mut tracker = EffectiveCwdTracker::with_initial_cwd(initial.clone());

        let new_dir = PathBuf::from("/home/user/worktree");
        let start = Instant::now();

        // Start tracking new directory as candidate
        tracker.observe_directory_at(new_dir.clone(), start);

        // Go back to effective CWD - should clear candidate
        let mid = start + Duration::from_millis(300);
        tracker.observe_directory_at(initial, mid);

        // Now observe new_dir again after what would have been the threshold
        // It should start fresh because candidate was cleared
        let after = start + Duration::from_millis(600);
        let changed = tracker.observe_directory_at(new_dir, after);
        assert!(!changed); // New candidate started, not promoted yet
    }

    #[test]
    fn test_reset_clears_everything() {
        let initial = PathBuf::from("/home/user/project");
        let mut tracker = EffectiveCwdTracker::with_initial_cwd(initial);

        let new_dir = PathBuf::from("/home/user/worktree");
        tracker.observe_directory(new_dir);

        let reset_dir = PathBuf::from("/home/user/new-project");
        tracker.reset(Some(reset_dir.clone()));

        assert_eq!(tracker.effective_cwd(), Some(&reset_dir));
    }

    #[test]
    fn test_reset_to_none() {
        let initial = PathBuf::from("/home/user/project");
        let mut tracker = EffectiveCwdTracker::with_initial_cwd(initial);

        tracker.reset(None);
        assert!(tracker.effective_cwd().is_none());
    }

    #[test]
    fn test_observe_file_path_extracts_parent_directory() {
        let initial = PathBuf::from("/home/user/project");
        let mut tracker = EffectiveCwdTracker::with_initial_cwd(initial);

        let file_path = PathBuf::from("/home/user/worktree/src/main.rs");
        let start = Instant::now();

        // First observation - starts the candidate with parent dir
        let changed = tracker.observe_file_path_at(&file_path, start);
        assert!(!changed);

        // Second observation after threshold - should change to parent dir
        let after_threshold = start + Duration::from_millis(600);
        let changed = tracker.observe_file_path_at(&file_path, after_threshold);
        assert!(changed);

        // Effective CWD should be the parent directory, not the file path
        let expected_dir = PathBuf::from("/home/user/worktree/src");
        assert_eq!(tracker.effective_cwd(), Some(&expected_dir));
    }

    #[test]
    fn test_observe_file_path_with_root_file_returns_false() {
        let initial = PathBuf::from("/home/user/project");
        let mut tracker = EffectiveCwdTracker::with_initial_cwd(initial);

        // A file at the root has "/" as parent
        let file_path = PathBuf::from("/root_file.txt");
        let start = Instant::now();

        // First observation
        let changed = tracker.observe_file_path_at(&file_path, start);
        assert!(!changed);

        // After threshold - should change to root
        let after_threshold = start + Duration::from_millis(600);
        let changed = tracker.observe_file_path_at(&file_path, after_threshold);
        assert!(changed);

        let expected_dir = PathBuf::from("/");
        assert_eq!(tracker.effective_cwd(), Some(&expected_dir));
    }

    #[test]
    fn test_observe_file_path_without_parent_returns_false() {
        let initial = PathBuf::from("/home/user/project");
        let mut tracker = EffectiveCwdTracker::with_initial_cwd(initial.clone());

        // A bare filename has no parent
        let file_path = PathBuf::from("file.txt");
        let changed = tracker.observe_file_path(&file_path);

        // Should return false and not change effective CWD
        assert!(!changed);
        assert_eq!(tracker.effective_cwd(), Some(&initial));
    }

    #[test]
    fn test_observe_file_path_and_directory_interleaved() {
        let initial = PathBuf::from("/home/user/project");
        let mut tracker = EffectiveCwdTracker::with_initial_cwd(initial);

        let file_path = PathBuf::from("/home/user/worktree/src/main.rs");
        let dir_path = PathBuf::from("/home/user/worktree/src");
        let start = Instant::now();

        // Observe via file path
        let changed = tracker.observe_file_path_at(&file_path, start);
        assert!(!changed);

        // Observe via directory directly (same directory as file's parent)
        // This should count toward the debounce threshold
        let mid = start + Duration::from_millis(300);
        let changed = tracker.observe_directory_at(dir_path.clone(), mid);
        assert!(!changed);

        // After threshold - should change
        let after_threshold = start + Duration::from_millis(600);
        let changed = tracker.observe_directory_at(dir_path.clone(), after_threshold);
        assert!(changed);
        assert_eq!(tracker.effective_cwd(), Some(&dir_path));
    }

    #[test]
    fn test_observe_empty_directory_returns_false() {
        // Empty directory paths should be rejected to prevent git commands from failing.
        // This can happen when ACP mode sends ExecCommandBegin events with PathBuf::new().
        let initial = PathBuf::from("/home/user/project");
        let mut tracker = EffectiveCwdTracker::with_initial_cwd(initial.clone());

        // Empty PathBuf should be rejected
        let empty_dir = PathBuf::new();
        let changed = tracker.observe_directory(empty_dir);

        // Should return false and not change effective CWD
        assert!(!changed);
        assert_eq!(tracker.effective_cwd(), Some(&initial));

        // Also verify it doesn't become a candidate (test by observing again after threshold)
        let start = Instant::now();
        let empty_dir = PathBuf::new();
        tracker.observe_directory_at(empty_dir.clone(), start);

        let after_threshold = start + Duration::from_millis(600);
        let changed = tracker.observe_directory_at(empty_dir, after_threshold);
        assert!(!changed);
        assert_eq!(tracker.effective_cwd(), Some(&initial));
    }

    #[test]
    fn test_observe_empty_directory_does_not_clear_effective_cwd() {
        // Verify that observing an empty directory doesn't clear an existing effective CWD
        let initial = PathBuf::from("/home/user/project");
        let mut tracker = EffectiveCwdTracker::with_initial_cwd(initial.clone());

        // Observe empty directory multiple times
        for _ in 0..10 {
            let changed = tracker.observe_directory(PathBuf::new());
            assert!(!changed);
        }

        // Effective CWD should still be the initial value
        assert_eq!(tracker.effective_cwd(), Some(&initial));
    }
}
