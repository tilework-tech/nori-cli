//! Tracks incomplete ExecCells that were flushed before completion.
//!
//! When agent text streams during an ACP tool call execution, the incomplete
//! ExecCell gets flushed from `active_cell`. This tracker saves those cells
//! by `call_id` so they can be retrieved and completed when `ExecCommandEnd`
//! arrives, preventing duplicate entries in history.

use std::collections::HashMap;

use crate::exec_cell::ExecCell;
use crate::history_cell::HistoryCell;

/// Manages incomplete ExecCells that were flushed before their tool calls completed.
///
/// This prevents duplicate history entries when streaming text causes an incomplete
/// ExecCell to be flushed, and then a new one would be created when the tool call ends.
#[derive(Default)]
pub(crate) struct PendingExecCellTracker {
    /// Incomplete cells keyed by call_id for later retrieval.
    pending: HashMap<String, Box<dyn HistoryCell>>,
}

impl PendingExecCellTracker {
    /// Creates a new empty tracker.
    pub(crate) fn new() -> Self {
        Self {
            pending: HashMap::new(),
        }
    }

    /// Saves a pending cell by its call_id.
    ///
    /// Called when an incomplete ExecCell is flushed from `active_cell` during streaming.
    pub(crate) fn save_pending(&mut self, call_id: String, cell: Box<dyn HistoryCell>) {
        self.pending.insert(call_id, cell);
    }

    /// Retrieves and removes a pending cell by call_id.
    ///
    /// Called when `ExecCommandEnd` arrives to check if there's an incomplete cell
    /// that should be completed instead of creating a new one.
    pub(crate) fn retrieve(&mut self, call_id: &str) -> Option<Box<dyn HistoryCell>> {
        self.pending.remove(call_id)
    }

    /// Drains all pending cells, marking them as failed.
    ///
    /// Called on task completion to clean up any cells that weren't completed
    /// (e.g., due to interruption). Returns the cells for insertion into history.
    pub(crate) fn drain_failed(&mut self) -> Vec<Box<dyn HistoryCell>> {
        self.pending
            .drain()
            .map(|(_, mut cell)| {
                if let Some(exec) = cell.as_any_mut().downcast_mut::<ExecCell>() {
                    exec.mark_failed();
                }
                cell
            })
            .collect()
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

        tracker.save_pending(call_id.to_string(), make_test_exec_cell(call_id));

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

        tracker.save_pending("call-1".to_string(), make_test_exec_cell("call-1"));
        tracker.save_pending("call-2".to_string(), make_test_exec_cell("call-2"));

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
        tracker.save_pending("call-1".to_string(), make_test_exec_cell("call-1"));

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
}
