//! Tracks incomplete ExecCells that were flushed before completion.
//!
//! When agent text streams during an ACP tool call execution, the incomplete
//! ExecCell gets flushed from `active_cell`. This tracker saves those cells
//! by `call_id` so they can be retrieved and completed when `ExecCommandEnd`
//! arrives, preventing duplicate entries in history.

use std::collections::HashMap;
use tracing::debug;

use crate::exec_cell::ExecCell;
use crate::history_cell::HistoryCell;

/// Manages incomplete ExecCells that were flushed before their tool calls completed.
///
/// This prevents duplicate history entries when streaming text causes an incomplete
/// ExecCell to be flushed, and then a new one would be created when the tool call ends.
///
/// Supports multi-call cells by allowing storage under multiple call_ids that map to
/// the same cell. This is essential for exploring cells that group multiple Read/Search
/// operations, which can have completion events arrive out-of-order.
#[derive(Default)]
pub(crate) struct PendingExecCellTracker {
    /// Maps call_id to primary_key for multi-key lookup.
    call_id_to_primary: HashMap<String, String>,
    /// Stores the actual cells keyed by primary_key.
    cells: HashMap<String, Box<dyn HistoryCell>>,
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

        // Store the cell under the primary key
        self.cells.insert(primary_key.clone(), cell);

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

        // Remove and return the cell
        let cell = self.cells.remove(&primary_key);

        debug!(
            target: "pending_exec_cells",
            call_id = %call_id,
            primary_key = %primary_key,
            found = cell.is_some(),
            total_pending_after = self.cells.len(),
            "retrieve: completed"
        );

        cell
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
            .map(|(key, mut cell)| {
                debug!(
                    target: "pending_exec_cells",
                    primary_key = %key,
                    "drain_failed: marking cell as failed"
                );
                if let Some(exec) = cell.as_any_mut().downcast_mut::<ExecCell>() {
                    exec.mark_failed();
                }
                cell
            })
            .collect();

        debug!(
            target: "pending_exec_cells",
            drained_count = cells.len(),
            "drain_failed: completed"
        );

        cells
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec_cell::new_active_exec_command;
    use codex_core::protocol::ExecCommandSource;

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
}
