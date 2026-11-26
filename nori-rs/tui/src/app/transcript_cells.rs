//! The active session's transcript cells, with a count of the edits that
//! were not plain appends.
//!
//! Anything that renders the transcript and caches the result (the
//! full-screen view) can render only the newly appended cells while the
//! generation is unchanged, and must start over once it moves.

use std::sync::Arc;

use crate::history_cell::HistoryCell;

#[derive(Debug, Default)]
pub(crate) struct TranscriptCells {
    cells: Vec<Arc<dyn HistoryCell>>,
    generation: i64,
}

impl TranscriptCells {
    /// Moves on every edit other than `push`.
    pub(crate) fn generation(&self) -> i64 {
        self.generation
    }

    pub(crate) fn push(&mut self, cell: Arc<dyn HistoryCell>) {
        self.cells.push(cell);
    }

    /// Change the cells in any way other than appending.
    pub(crate) fn edit(&mut self) -> &mut Vec<Arc<dyn HistoryCell>> {
        self.generation += 1;
        &mut self.cells
    }

    /// Swap in another transcript and return the previous one.
    pub(crate) fn replace(
        &mut self,
        cells: Vec<Arc<dyn HistoryCell>>,
    ) -> Vec<Arc<dyn HistoryCell>> {
        std::mem::replace(self.edit(), cells)
    }
}

impl From<Vec<Arc<dyn HistoryCell>>> for TranscriptCells {
    fn from(cells: Vec<Arc<dyn HistoryCell>>) -> Self {
        Self {
            cells,
            generation: 0,
        }
    }
}

impl std::ops::Deref for TranscriptCells {
    type Target = Vec<Arc<dyn HistoryCell>>;

    fn deref(&self) -> &Self::Target {
        &self.cells
    }
}
