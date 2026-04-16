//! Tracks incomplete ExecCells that were flushed before completion.
//!
//! When agent text streams during an ACP tool call execution, the incomplete
//! ExecCell gets flushed from `active_cell`. This tracker saves those cells
//! by `call_id` so they can be retrieved and completed when `ExecCommandEnd`
//! arrives, preventing duplicate entries in history.

use std::collections::HashMap;
use std::time::SystemTime;
use tracing::debug;
use tracing::warn;

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
    cells: HashMap<String, PendingCellEntry>,
}

#[derive(Debug)]
struct PendingCellEntry {
    cell: Box<dyn HistoryCell>,
    initial_pending_at: SystemTime,
    last_update_at: SystemTime,
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
    #[allow(dead_code)]
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
        let now = SystemTime::now();

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
        match self.cells.entry(primary_key.clone()) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let e = entry.get_mut();
                e.cell = cell;
                e.last_update_at = now;
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(PendingCellEntry {
                    cell,
                    initial_pending_at: now,
                    last_update_at: now,
                });
            }
        }

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
        let cell = self.cells.remove(&primary_key).map(|entry| entry.cell);

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

    /// Drains all pending cells, logging and discarding them.
    ///
    /// Called on task completion to clean up any cells that weren't completed
    /// (e.g., due to interruption). Returns the number of discarded cells.
    pub(crate) fn drain_failed(&mut self) -> usize {
        let count = self.cells.len();
        if count == 0 {
            debug!(
                target: "pending_exec_cells",
                "drain_failed: no pending cells to drain"
            );
            return 0;
        }

        let drain_time = SystemTime::now();
        let mut call_ids_by_primary: HashMap<String, Vec<String>> = HashMap::new();
        for (call_id, primary_key) in &self.call_id_to_primary {
            call_ids_by_primary
                .entry(primary_key.clone())
                .or_default()
                .push(call_id.clone());
        }

        // Clear the call_id mappings
        self.call_id_to_primary.clear();

        for (primary_key, mut entry) in self.cells.drain() {
            let call_ids = call_ids_by_primary.remove(&primary_key).unwrap_or_default();
            let pending_duration = drain_time.duration_since(entry.initial_pending_at).ok();
            let since_update = drain_time.duration_since(entry.last_update_at).ok();
            let (pending_call_ids, total_calls) =
                if let Some(exec) = entry.cell.as_any_mut().downcast_mut::<ExecCell>() {
                    (exec.pending_call_ids(), exec.iter_calls().count())
                } else {
                    (Vec::new(), 0)
                };

            warn!(
                target: "pending_exec_cells",
                primary_key = %primary_key,
                call_ids = ?call_ids,
                pending_call_ids = ?pending_call_ids,
                total_calls = total_calls,
                initial_pending_at = ?entry.initial_pending_at,
                last_update_at = ?entry.last_update_at,
                drain_time = ?drain_time,
                pending_duration = ?pending_duration,
                since_last_update = ?since_update,
                "drain_failed: discarding pending cell after incomplete execution"
            );
        }

        warn!(
            target: "pending_exec_cells",
            drained_count = count,
            "drain_failed: discarded all pending cells"
        );

        count
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
    fn drain_failed_discards_all_cells_and_empties_tracker() {
        let mut tracker = PendingExecCellTracker::new();

        tracker.save_pending(vec!["call-1".to_string()], make_test_exec_cell("call-1"));
        tracker.save_pending(vec!["call-2".to_string()], make_test_exec_cell("call-2"));

        let drained_count = tracker.drain_failed();
        assert_eq!(drained_count, 2, "Should drain all pending cells");

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
