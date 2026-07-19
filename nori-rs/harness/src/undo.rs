//! Undo support for the ACP backend via git ghost snapshots.
//!
//! This module provides [`GhostSnapshotStack`] for storing snapshots and
//! [`handle_undo`] for restoring the most recent snapshot.

use std::path::Path;

use codex_git::GhostCommit;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use nori_protocol::UndoEvent;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::backend::BackendEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoSnapshot {
    pub index: i64,
    pub short_id: String,
    pub label: String,
}

/// A ghost commit paired with a user-facing label (the user's message at that turn).
pub struct SnapshotEntry {
    pub commit: GhostCommit,
    pub label: String,
}

/// Error returned by [`GhostSnapshotStack::restore_to_index`] when the
/// requested index cannot be fulfilled.
pub enum RestoreError {
    /// The snapshot stack is empty.
    Empty,
    /// The index is out of bounds for the current stack length.
    OutOfBounds,
}

/// Thread-safe stack of ghost commit snapshots for undo support.
pub struct GhostSnapshotStack {
    snapshots: Mutex<Vec<SnapshotEntry>>,
}

impl Default for GhostSnapshotStack {
    fn default() -> Self {
        Self::new()
    }
}

impl GhostSnapshotStack {
    pub fn new() -> Self {
        Self {
            snapshots: Mutex::new(Vec::new()),
        }
    }

    pub async fn push(&self, snapshot: GhostCommit, label: String) {
        self.snapshots.lock().await.push(SnapshotEntry {
            commit: snapshot,
            label,
        });
    }

    pub async fn pop(&self) -> Option<GhostCommit> {
        self.snapshots.lock().await.pop().map(|entry| entry.commit)
    }

    pub async fn is_empty(&self) -> bool {
        self.snapshots.lock().await.is_empty()
    }

    /// Return snapshot metadata in reverse chronological order (most recent first).
    pub async fn list(&self) -> Vec<UndoSnapshot> {
        let snapshots = self.snapshots.lock().await;
        snapshots
            .iter()
            .rev()
            .enumerate()
            .map(|(i, entry)| {
                let short_id: String = entry.commit.id().chars().take(7).collect();
                UndoSnapshot {
                    index: i as i64,
                    short_id,
                    label: entry.label.clone(),
                }
            })
            .collect()
    }

    /// Remove and return the commit at the given display index (0 = most recent),
    /// along with all newer entries. Returns a [`RestoreError`] if the stack is
    /// empty or the index is out of bounds.
    pub async fn restore_to_index(&self, index: i64) -> Result<GhostCommit, RestoreError> {
        let mut snapshots = self.snapshots.lock().await;
        let len = snapshots.len() as i64;
        if len == 0 {
            return Err(RestoreError::Empty);
        }
        if index < 0 || index >= len {
            return Err(RestoreError::OutOfBounds);
        }
        // Display index 0 = last element in vec, index 1 = second-to-last, etc.
        let vec_index = len - 1 - index;
        let entry = snapshots.remove(vec_index as usize);
        // Truncate everything after vec_index (newer entries)
        snapshots.truncate(vec_index as usize);
        Ok(entry.commit)
    }
}

/// Execute the undo operation: pop the most recent ghost snapshot and restore it.
///
/// Emits `UndoStarted` and `UndoCompleted` events on the provided channel.
pub async fn handle_undo(
    event_tx: &mpsc::Sender<BackendEvent>,
    id: &str,
    cwd: &Path,
    snapshots: &GhostSnapshotStack,
) {
    let _ = event_tx
        .send(BackendEvent::Public(SessionEvent::Nori(NoriEvent::Undo(
            UndoEvent::Started {
                operation_id: id.to_string(),
                message: Some("Undo in progress...".to_string()),
            },
        ))))
        .await;

    let snapshot = snapshots.pop().await;

    let completed = match snapshot {
        None => {
            warn!("Undo requested but no snapshots available");
            (false, Some("No snapshot available to undo.".to_string()))
        }
        Some(ghost_commit) => {
            let commit_id = ghost_commit.id().to_string();
            let repo_path = cwd.to_path_buf();
            let restore_result = tokio::task::spawn_blocking(move || {
                codex_git::restore_ghost_commit(&repo_path, &ghost_commit)
            })
            .await;

            match restore_result {
                Ok(Ok(())) => {
                    let short_id: String = commit_id.chars().take(7).collect();
                    info!(commit_id, "Undo restored ghost snapshot");
                    (
                        true,
                        Some(format!(
                            "Undo restored snapshot {short_id}. Note: the agent is not aware that files have changed."
                        )),
                    )
                }
                Ok(Err(err)) => {
                    let message = format!("Failed to restore snapshot {commit_id}: {err}");
                    warn!("{message}");
                    (false, Some(message))
                }
                Err(err) => {
                    let message = format!("Failed to restore snapshot {commit_id}: {err}");
                    error!("{message}");
                    (false, Some(message))
                }
            }
        }
    };

    let _ = event_tx
        .send(BackendEvent::Public(SessionEvent::Nori(NoriEvent::Undo(
            UndoEvent::Completed {
                operation_id: id.to_string(),
                success: completed.0,
                message: completed.1,
            },
        ))))
        .await;
}

/// Undo to a specific snapshot identified by display index (0 = most recent).
///
/// Emits `UndoStarted` and `UndoCompleted` events on the provided channel.
/// The completion message includes a warning that the agent is unaware of the change.
pub async fn handle_undo_to(
    event_tx: &mpsc::Sender<BackendEvent>,
    id: &str,
    cwd: &Path,
    snapshots: &GhostSnapshotStack,
    index: i64,
) {
    let _ = event_tx
        .send(BackendEvent::Public(SessionEvent::Nori(NoriEvent::Undo(
            UndoEvent::Started {
                operation_id: id.to_string(),
                message: Some("Undo in progress...".to_string()),
            },
        ))))
        .await;

    let completed = match snapshots.restore_to_index(index).await {
        Err(RestoreError::Empty) => {
            warn!("Undo requested but no snapshots available");
            (false, Some("No snapshot available to undo.".to_string()))
        }
        Err(RestoreError::OutOfBounds) => {
            warn!("Undo requested with out-of-bounds index {index}");
            (false, Some(format!("Invalid snapshot index: {index}")))
        }
        Ok(ghost_commit) => {
            let commit_id = ghost_commit.id().to_string();
            let repo_path = cwd.to_path_buf();
            let restore_result = tokio::task::spawn_blocking(move || {
                codex_git::restore_ghost_commit(&repo_path, &ghost_commit)
            })
            .await;

            match restore_result {
                Ok(Ok(())) => {
                    let short_id: String = commit_id.chars().take(7).collect();
                    info!(commit_id, "Undo restored ghost snapshot via undo_to");
                    (
                        true,
                        Some(format!(
                            "Restored snapshot {short_id}. Note: the agent is not aware that files have changed."
                        )),
                    )
                }
                Ok(Err(err)) => {
                    let message = format!("Failed to restore snapshot {commit_id}: {err}");
                    warn!("{message}");
                    (false, Some(message))
                }
                Err(err) => {
                    let message = format!("Failed to restore snapshot {commit_id}: {err}");
                    error!("{message}");
                    (false, Some(message))
                }
            }
        }
    };

    let _ = event_tx
        .send(BackendEvent::Public(SessionEvent::Nori(NoriEvent::Undo(
            UndoEvent::Completed {
                operation_id: id.to_string(),
                success: completed.0,
                message: completed.1,
            },
        ))))
        .await;
}
