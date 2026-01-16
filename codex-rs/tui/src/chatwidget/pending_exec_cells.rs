//! Tracks incomplete ExecCells that were flushed before completion.
//!
//! When agent text streams during an ACP tool call execution, the incomplete
//! ExecCell gets flushed from `active_cell`. This tracker saves those cells
//! by `call_id` so they can be retrieved and completed when `ExecCommandEnd`
//! arrives, preventing duplicate entries in history.
//!
//! Cells are stored with timestamps and can be timed out after a configurable
//! duration. Timed-out cells are discarded with detailed tracing warnings to
//! aid debugging.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

use crate::exec_cell::ExecCell;
use crate::history_cell::HistoryCell;

/// Entry storing a pending cell along with metadata.
struct PendingCellEntry {
    cell: Box<dyn HistoryCell>,
    saved_at: Instant,
    /// All call_ids associated with this cell (for logging).
    call_ids: Vec<String>,
}

/// Manages incomplete ExecCells that were flushed before their tool calls completed.
///
/// This prevents duplicate history entries when streaming text causes an incomplete
/// ExecCell to be flushed, and then a new one would be created when the tool call ends.
///
/// Supports multi-call cells by allowing storage under multiple call_ids that map to
/// the same cell. This is essential for exploring cells that group multiple Read/Search
/// operations, which can have completion events arrive out-of-order.
///
/// Cells are timestamped when saved and can be timed out via `check_timeouts()` or
/// `discard_timed_out()`. At task completion, remaining cells should be discarded
/// via `discard_all_with_warning()` rather than being drained to history.
#[derive(Default)]
pub(crate) struct PendingExecCellTracker {
    /// Maps call_id to primary_key for multi-key lookup.
    call_id_to_primary: HashMap<String, String>,
    /// Stores the actual cells keyed by primary_key.
    cells: HashMap<String, PendingCellEntry>,
}

impl PendingExecCellTracker {
    /// Creates a new empty tracker.
    pub(crate) fn new() -> Self {
        Self {
            call_id_to_primary: HashMap::new(),
            cells: HashMap::new(),
        }
    }

    /// Saves a pending cell by all its call_ids.
    ///
    /// Called when an incomplete ExecCell is flushed from `active_cell` during streaming.
    /// For multi-call exploring cells, this registers all pending call_ids so the cell
    /// can be retrieved when any of them completes.
    ///
    /// The current time is recorded so the cell can be timed out later if it's never
    /// completed.
    ///
    /// # Arguments
    /// * `call_ids` - All pending call_ids for this cell. The first is used as the primary key.
    /// * `cell` - The incomplete cell to save.
    pub(crate) fn save_pending(&mut self, call_ids: Vec<String>, cell: Box<dyn HistoryCell>) {
        if call_ids.is_empty() {
            debug!(
                target: "pending_exec_cells",
                "save_pending called with empty call_ids, ignoring"
            );
            return;
        }

        // Use the first call_id as the primary key
        let primary_key = call_ids[0].clone();

        debug!(
            target: "pending_exec_cells",
            call_ids = ?call_ids,
            primary_key = %primary_key,
            total_pending_before = self.cells.len(),
            "save_pending: storing cell with {} call_ids",
            call_ids.len()
        );

        // Map all call_ids to this primary key
        for id in &call_ids {
            self.call_id_to_primary
                .insert(id.clone(), primary_key.clone());
        }

        // Store the cell under the primary key with timestamp
        let entry = PendingCellEntry {
            cell,
            saved_at: Instant::now(),
            call_ids,
        };
        self.cells.insert(primary_key.clone(), entry);

        debug!(
            target: "pending_exec_cells",
            primary_key = %primary_key,
            total_pending_after = self.cells.len(),
            "save_pending: cell stored successfully"
        );
    }

    /// Retrieves and removes a pending cell by call_id.
    ///
    /// Called when `ExecCommandEnd` arrives to check if there's an incomplete cell
    /// that should be completed instead of creating a new one.
    ///
    /// This works for any call_id associated with the cell, not just the primary key.
    /// When retrieved, all call_ids for this cell are invalidated.
    pub(crate) fn retrieve(&mut self, call_id: &str) -> Option<Box<dyn HistoryCell>> {
        self.retrieve_with_timestamp(call_id).map(|(cell, _)| cell)
    }

    /// Retrieves and removes a pending cell by call_id, returning the timestamp.
    ///
    /// Same as `retrieve()` but also returns when the cell was saved. Useful for
    /// debugging and testing.
    pub(crate) fn retrieve_with_timestamp(
        &mut self,
        call_id: &str,
    ) -> Option<(Box<dyn HistoryCell>, Instant)> {
        debug!(
            target: "pending_exec_cells",
            call_id = %call_id,
            total_pending = self.cells.len(),
            "retrieve: looking up cell"
        );

        // Look up the primary key for this call_id
        let primary_key = match self.call_id_to_primary.remove(call_id) {
            Some(pk) => pk,
            None => {
                debug!(
                    target: "pending_exec_cells",
                    call_id = %call_id,
                    "retrieve: no mapping found for call_id"
                );
                return None;
            }
        };

        debug!(
            target: "pending_exec_cells",
            call_id = %call_id,
            primary_key = %primary_key,
            "retrieve: found primary key, removing all mappings"
        );

        // Remove all other mappings to this primary key
        self.call_id_to_primary.retain(|_, pk| pk != &primary_key);

        // Remove and return the cell with its timestamp
        let entry = self.cells.remove(&primary_key);

        debug!(
            target: "pending_exec_cells",
            call_id = %call_id,
            primary_key = %primary_key,
            found = entry.is_some(),
            total_pending_after = self.cells.len(),
            "retrieve: completed"
        );

        entry.map(|e| (e.cell, e.saved_at))
    }

    /// Returns the number of cells currently pending.
    #[allow(dead_code)]
    pub(crate) fn len(&self) -> usize {
        self.cells.len()
    }

    /// Drains all pending cells, marking them as failed.
    ///
    /// Called on task completion to clean up any cells that weren't completed
    /// (e.g., due to interruption). Returns the cells for insertion into history.
    ///
    /// NOTE: Prefer `discard_all_with_warning()` instead to avoid dumping cells
    /// out of order at the end of the transcript.
    #[allow(dead_code)]
    pub(crate) fn drain_failed(&mut self) -> Vec<Box<dyn HistoryCell>> {
        let count = self.cells.len();
        debug!(
            target: "pending_exec_cells",
            count = count,
            "drain_failed: draining all pending cells"
        );

        // Clear the call_id mappings
        self.call_id_to_primary.clear();

        // Drain and mark all cells as failed
        let cells: Vec<_> = self
            .cells
            .drain()
            .map(|(key, mut entry)| {
                debug!(
                    target: "pending_exec_cells",
                    primary_key = %key,
                    "drain_failed: marking cell as failed"
                );
                if let Some(exec) = entry.cell.as_any_mut().downcast_mut::<ExecCell>() {
                    exec.mark_failed();
                }
                entry.cell
            })
            .collect();

        debug!(
            target: "pending_exec_cells",
            drained_count = cells.len(),
            "drain_failed: completed"
        );

        cells
    }

    /// Checks for cells that have exceeded the timeout and returns them.
    ///
    /// Cells that have been pending longer than `timeout` are removed from the
    /// tracker and returned. The caller is responsible for handling them (e.g.,
    /// discarding with a warning).
    ///
    /// Returns a vector of (cell, saved_at, call_ids) tuples for timed-out cells.
    pub(crate) fn check_timeouts(
        &mut self,
        timeout: Duration,
    ) -> Vec<(Box<dyn HistoryCell>, Instant, Vec<String>)> {
        let now = Instant::now();

        // Find all entries that have timed out
        let timed_out_keys: Vec<String> = self
            .cells
            .iter()
            .filter(|(_, entry)| now.duration_since(entry.saved_at) > timeout)
            .map(|(key, _)| key.clone())
            .collect();

        if timed_out_keys.is_empty() {
            return Vec::new();
        }

        debug!(
            target: "pending_exec_cells",
            count = timed_out_keys.len(),
            timeout_ms = timeout.as_millis(),
            "check_timeouts: found timed out cells"
        );

        // Remove timed-out entries and collect them
        let mut result = Vec::new();
        for key in timed_out_keys {
            if let Some(entry) = self.cells.remove(&key) {
                // Clean up call_id mappings for this entry
                for call_id in &entry.call_ids {
                    self.call_id_to_primary.remove(call_id);
                }
                result.push((entry.cell, entry.saved_at, entry.call_ids));
            }
        }

        result
    }

    /// Discards cells that have exceeded the timeout, logging detailed warnings.
    ///
    /// This is the preferred way to handle timed-out cells. Each discarded cell
    /// is logged with detailed information for debugging:
    /// - All associated call_ids
    /// - How long the cell was pending
    /// - Full debug representation of the cell
    /// - ExecCell-specific details if applicable
    ///
    /// Returns the number of cells discarded.
    pub(crate) fn discard_timed_out(&mut self, timeout: Duration) -> usize {
        let timed_out = self.check_timeouts(timeout);
        let count = timed_out.len();

        for (cell, saved_at, call_ids) in timed_out {
            let elapsed = saved_at.elapsed();
            log_discarded_cell(&*cell, &call_ids, elapsed, "timed out");
        }

        count
    }

    /// Discards all remaining pending cells with detailed warnings.
    ///
    /// Called at task completion to clean up any cells that weren't completed.
    /// Unlike `drain_failed()`, this does NOT return cells for insertion into
    /// history - they are simply discarded with logging. This prevents cells
    /// from appearing out of order at the end of the transcript.
    ///
    /// Returns the number of cells discarded.
    pub(crate) fn discard_all_with_warning(&mut self) -> usize {
        let count = self.cells.len();

        if count == 0 {
            return 0;
        }

        debug!(
            target: "pending_exec_cells",
            count = count,
            "discard_all_with_warning: discarding all remaining pending cells"
        );

        // Clear call_id mappings
        self.call_id_to_primary.clear();

        // Drain and log each cell
        for (_, entry) in self.cells.drain() {
            let elapsed = entry.saved_at.elapsed();
            log_discarded_cell(&*entry.cell, &entry.call_ids, elapsed, "task completed");
        }

        count
    }
}

/// Logs detailed information about a discarded cell.
fn log_discarded_cell(
    cell: &dyn HistoryCell,
    call_ids: &[String],
    elapsed: Duration,
    reason: &str,
) {
    // Extract ExecCell-specific details if available
    let (exec_commands, exec_pending_ids) =
        if let Some(exec) = cell.as_any().downcast_ref::<ExecCell>() {
            let commands: Vec<Vec<String>> = exec.iter_calls().map(|c| c.command.clone()).collect();
            let pending: Vec<String> = exec.pending_call_ids();
            (Some(commands), Some(pending))
        } else {
            (None, None)
        };

    warn!(
        target: "pending_exec_cells",
        call_ids = ?call_ids,
        elapsed_ms = elapsed.as_millis(),
        reason = %reason,
        cell_debug = %format!("{:#?}", cell),
        exec_commands = ?exec_commands,
        exec_pending_ids = ?exec_pending_ids,
        "discarding pending cell without resolution - cell data logged for debugging"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec_cell::new_active_exec_command;
    use codex_core::protocol::ExecCommandSource;
    use std::time::{Duration, Instant};

    fn make_test_exec_cell(call_id: &str) -> Box<dyn HistoryCell> {
        Box::new(new_active_exec_command(
            call_id.to_string(),
            vec!["echo".to_string(), "test".to_string()],
            vec![],
            ExecCommandSource::Agent,
            None,
            false, // animations disabled
        ))
    }

    #[test]
    fn save_and_retrieve_returns_cell() {
        let mut tracker = PendingExecCellTracker::new();
        let call_id = "test-call-001";

        tracker.save_pending(vec![call_id.to_string()], make_test_exec_cell(call_id));

        let retrieved = tracker.retrieve(call_id);
        assert!(retrieved.is_some(), "Should retrieve the saved cell");

        // Second retrieve should return None (cell was removed)
        let second = tracker.retrieve(call_id);
        assert!(second.is_none(), "Cell should be removed after retrieval");
    }

    #[test]
    fn retrieve_nonexistent_returns_none() {
        let mut tracker = PendingExecCellTracker::new();

        let result = tracker.retrieve("nonexistent-call");
        assert!(result.is_none(), "Should return None for unknown call_id");
    }

    #[test]
    fn drain_failed_returns_all_cells_and_empties_tracker() {
        let mut tracker = PendingExecCellTracker::new();

        tracker.save_pending(vec!["call-1".to_string()], make_test_exec_cell("call-1"));
        tracker.save_pending(vec!["call-2".to_string()], make_test_exec_cell("call-2"));

        let drained = tracker.drain_failed();
        assert_eq!(drained.len(), 2, "Should drain all pending cells");

        // Tracker should be empty now
        assert!(
            tracker.retrieve("call-1").is_none(),
            "Tracker should be empty after drain"
        );
        assert!(
            tracker.retrieve("call-2").is_none(),
            "Tracker should be empty after drain"
        );
    }

    #[test]
    fn drain_failed_marks_exec_cells_as_failed() {
        let mut tracker = PendingExecCellTracker::new();
        tracker.save_pending(vec!["call-1".to_string()], make_test_exec_cell("call-1"));

        let drained = tracker.drain_failed();
        assert_eq!(drained.len(), 1);

        // The cell should no longer be active (mark_failed sets output on all calls)
        let cell = &drained[0];
        if let Some(exec) = cell.as_any().downcast_ref::<ExecCell>() {
            assert!(
                !exec.is_active(),
                "ExecCell should be marked as failed (not active)"
            );
        } else {
            panic!("Expected ExecCell");
        }
    }

    /// Test that a multi-call ExecCell can be retrieved by any of its pending call_ids.
    ///
    /// This tests the scenario where an exploring cell groups multiple Read operations,
    /// gets flushed while incomplete, and then completion events arrive out-of-order.
    /// The cell should be retrievable by ANY of the pending call_ids, not just the first.
    #[test]
    fn multi_call_cell_retrievable_by_any_pending_id() {
        use codex_protocol::parse_command::ParsedCommand;
        use std::path::PathBuf;

        let mut tracker = PendingExecCellTracker::new();

        // Create an ExecCell with 3 exploring calls (Read operations)
        let mut exec_cell = new_active_exec_command(
            "call-1".to_string(),
            vec!["Read".to_string(), "file1.rs".to_string()],
            vec![ParsedCommand::Read {
                cmd: "Read".to_string(),
                name: "file1.rs".to_string(),
                path: PathBuf::from("src/file1.rs"),
            }],
            ExecCommandSource::Agent,
            None,
            false,
        );

        // Add second call to the cell
        if let Some(new_cell) = exec_cell.with_added_call(
            "call-2".to_string(),
            vec!["Read".to_string(), "file2.rs".to_string()],
            vec![ParsedCommand::Read {
                cmd: "Read".to_string(),
                name: "file2.rs".to_string(),
                path: PathBuf::from("src/file2.rs"),
            }],
            ExecCommandSource::Agent,
            None,
        ) {
            exec_cell = new_cell;
        }

        // Add third call to the cell
        if let Some(new_cell) = exec_cell.with_added_call(
            "call-3".to_string(),
            vec!["Read".to_string(), "file3.rs".to_string()],
            vec![ParsedCommand::Read {
                cmd: "Read".to_string(),
                name: "file3.rs".to_string(),
                path: PathBuf::from("src/file3.rs"),
            }],
            ExecCommandSource::Agent,
            None,
        ) {
            exec_cell = new_cell;
        }

        // Verify the cell has 3 pending calls
        assert_eq!(
            exec_cell.pending_call_ids().len(),
            3,
            "Cell should have 3 pending calls"
        );

        // Get pending IDs and convert to Box<dyn HistoryCell>
        let pending_ids = exec_cell.pending_call_ids();
        let cell: Box<dyn HistoryCell> = Box::new(exec_cell);

        tracker.save_pending(pending_ids, cell);

        // Should be able to retrieve by call-2 (not the first call_id)
        let retrieved = tracker.retrieve("call-2");
        assert!(
            retrieved.is_some(),
            "Should be able to retrieve cell by second pending call_id"
        );

        // After retrieval, other call_ids should also be invalidated
        assert!(
            tracker.retrieve("call-1").is_none(),
            "First call_id should be invalidated after retrieval"
        );
        assert!(
            tracker.retrieve("call-3").is_none(),
            "Third call_id should be invalidated after retrieval"
        );
    }

    /// Test that retrieving by one call_id invalidates all other call_ids for the same cell.
    #[test]
    fn retrieve_invalidates_all_call_ids_for_same_cell() {
        use codex_protocol::parse_command::ParsedCommand;

        let mut tracker = PendingExecCellTracker::new();

        // Create a multi-call cell
        let mut exec_cell = new_active_exec_command(
            "call-a".to_string(),
            vec!["Search".to_string()],
            vec![ParsedCommand::Search {
                cmd: "Search".to_string(),
                query: Some("TODO".to_string()),
                path: None,
            }],
            ExecCommandSource::Agent,
            None,
            false,
        );

        if let Some(new_cell) = exec_cell.with_added_call(
            "call-b".to_string(),
            vec!["Search".to_string()],
            vec![ParsedCommand::Search {
                cmd: "Search".to_string(),
                query: Some("FIXME".to_string()),
                path: None,
            }],
            ExecCommandSource::Agent,
            None,
        ) {
            exec_cell = new_cell;
        }

        let pending_ids = exec_cell.pending_call_ids();
        let cell: Box<dyn HistoryCell> = Box::new(exec_cell);

        tracker.save_pending(pending_ids, cell);

        // Retrieve by call-b
        let retrieved = tracker.retrieve("call-b");
        assert!(retrieved.is_some(), "Should retrieve cell");

        // call-a should now be invalid
        assert!(
            tracker.retrieve("call-a").is_none(),
            "Other call_ids should be invalidated"
        );
    }

    // =========================================================================
    // Timestamp and timeout tests (for inline pending events feature)
    // =========================================================================

    #[test]
    fn save_pending_records_timestamp() {
        let mut tracker = PendingExecCellTracker::new();
        let before = Instant::now();
        tracker.save_pending(vec!["call-1".to_string()], make_test_exec_cell("call-1"));
        let after = Instant::now();

        // Retrieve with timestamp to verify it was recorded
        let (_, saved_at) = tracker.retrieve_with_timestamp("call-1").unwrap();
        assert!(
            saved_at >= before && saved_at <= after,
            "Timestamp should be recorded at save time"
        );
    }

    #[test]
    fn check_timeouts_detects_old_cells() {
        let mut tracker = PendingExecCellTracker::new();
        tracker.save_pending(vec!["call-1".to_string()], make_test_exec_cell("call-1"));

        // Use a very short timeout to simulate time passage
        let timed_out = tracker.check_timeouts(Duration::from_nanos(1));
        assert_eq!(timed_out.len(), 1, "Should detect cell that timed out");
    }

    #[test]
    fn check_timeouts_ignores_recent_cells() {
        let mut tracker = PendingExecCellTracker::new();
        tracker.save_pending(vec!["call-1".to_string()], make_test_exec_cell("call-1"));

        // Very long timeout - cell should not be considered timed out
        let timed_out = tracker.check_timeouts(Duration::from_secs(3600));
        assert!(
            timed_out.is_empty(),
            "Should not detect cells within timeout window"
        );
    }

    #[test]
    fn check_timeouts_removes_timed_out_cells() {
        let mut tracker = PendingExecCellTracker::new();
        tracker.save_pending(vec!["call-1".to_string()], make_test_exec_cell("call-1"));
        tracker.save_pending(vec!["call-2".to_string()], make_test_exec_cell("call-2"));

        // Timeout call-1 (short timeout)
        let _ = tracker.check_timeouts(Duration::from_nanos(1));

        // Both cells should be removed (since both timed out)
        assert!(
            tracker.retrieve("call-1").is_none(),
            "Timed out cell should be removed"
        );
        assert!(
            tracker.retrieve("call-2").is_none(),
            "Both cells should be removed"
        );
    }

    #[test]
    fn check_timeouts_cleans_up_call_id_mappings() {
        use codex_protocol::parse_command::ParsedCommand;
        use std::path::PathBuf;

        let mut tracker = PendingExecCellTracker::new();

        // Create a multi-call cell
        let mut exec_cell = new_active_exec_command(
            "call-a".to_string(),
            vec!["Read".to_string(), "file1.rs".to_string()],
            vec![ParsedCommand::Read {
                cmd: "Read".to_string(),
                name: "file1.rs".to_string(),
                path: PathBuf::from("src/file1.rs"),
            }],
            ExecCommandSource::Agent,
            None,
            false,
        );

        if let Some(new_cell) = exec_cell.with_added_call(
            "call-b".to_string(),
            vec!["Read".to_string(), "file2.rs".to_string()],
            vec![ParsedCommand::Read {
                cmd: "Read".to_string(),
                name: "file2.rs".to_string(),
                path: PathBuf::from("src/file2.rs"),
            }],
            ExecCommandSource::Agent,
            None,
        ) {
            exec_cell = new_cell;
        }

        let pending_ids = exec_cell.pending_call_ids();
        let cell: Box<dyn HistoryCell> = Box::new(exec_cell);
        tracker.save_pending(pending_ids, cell);

        // Timeout the cell
        let _ = tracker.check_timeouts(Duration::from_nanos(1));

        // Both call_ids should be cleaned up
        assert!(
            tracker.retrieve("call-a").is_none(),
            "call-a mapping should be cleaned up"
        );
        assert!(
            tracker.retrieve("call-b").is_none(),
            "call-b mapping should be cleaned up"
        );
    }

    #[test]
    fn discard_timed_out_removes_and_returns_count() {
        let mut tracker = PendingExecCellTracker::new();
        tracker.save_pending(vec!["call-1".to_string()], make_test_exec_cell("call-1"));
        tracker.save_pending(vec!["call-2".to_string()], make_test_exec_cell("call-2"));

        // Discard with very short timeout
        let discarded_count = tracker.discard_timed_out(Duration::from_nanos(1));

        assert_eq!(discarded_count, 2, "Should discard 2 cells");
        assert!(
            tracker.retrieve("call-1").is_none(),
            "call-1 should be gone"
        );
        assert!(
            tracker.retrieve("call-2").is_none(),
            "call-2 should be gone"
        );
    }

    #[test]
    fn discard_all_with_warning_empties_tracker() {
        let mut tracker = PendingExecCellTracker::new();
        tracker.save_pending(vec!["call-1".to_string()], make_test_exec_cell("call-1"));
        tracker.save_pending(vec!["call-2".to_string()], make_test_exec_cell("call-2"));

        let discarded_count = tracker.discard_all_with_warning();

        assert_eq!(discarded_count, 2, "Should discard all cells");
        assert_eq!(tracker.len(), 0, "Tracker should be empty");
    }

    #[test]
    fn discard_all_with_warning_on_empty_tracker() {
        let mut tracker = PendingExecCellTracker::new();

        let discarded_count = tracker.discard_all_with_warning();

        assert_eq!(discarded_count, 0, "Should discard 0 cells");
    }
}
