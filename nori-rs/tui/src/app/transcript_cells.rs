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
    /// Where each tail replacement since the last other edit began, oldest
    /// first. Cells before the lowest of them are unchanged.
    tail_edits: Vec<usize>,
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
        self.tail_edits.clear();
        &mut self.cells
    }

    /// The tail replacements made so far in this generation.
    pub(crate) fn tail_edits(&self) -> i64 {
        i64::try_from(self.tail_edits.len()).unwrap_or(i64::MAX)
    }

    /// The first cell any tail replacement after the first `seen` changed.
    pub(crate) fn changed_since(&self, seen: i64) -> Option<usize> {
        let seen = usize::try_from(seen).unwrap_or_default();
        self.tail_edits.get(seen..)?.iter().copied().min()
    }

    /// Replace the cells in `range`, which runs to the end, with `cell`.
    /// Cells before `range` are untouched, so renderers keep their rows.
    pub(crate) fn replace_tail(
        &mut self,
        range: std::ops::Range<usize>,
        cell: Arc<dyn HistoryCell>,
    ) {
        self.tail_edits.push(range.start);
        self.cells.splice(range, std::iter::once(cell));
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
            tail_edits: Vec::new(),
        }
    }
}

impl std::ops::Deref for TranscriptCells {
    type Target = Vec<Arc<dyn HistoryCell>>;

    fn deref(&self) -> &Self::Target {
        &self.cells
    }
}
