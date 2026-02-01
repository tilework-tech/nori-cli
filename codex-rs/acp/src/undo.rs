//! Undo support for the ACP backend via git ghost snapshots.
//!
//! This module provides [`GhostSnapshotStack`] for storing snapshots and
//! [`handle_undo`] for restoring the most recent snapshot.

use std::path::Path;

use codex_git::GhostCommit;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::UndoCompletedEvent;
use codex_protocol::protocol::UndoStartedEvent;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tracing::error;
use tracing::info;
use tracing::warn;

/// Thread-safe stack of ghost commit snapshots for undo support.
pub struct GhostSnapshotStack {
    snapshots: Mutex<Vec<GhostCommit>>,
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

    pub async fn push(&self, snapshot: GhostCommit) {
        self.snapshots.lock().await.push(snapshot);
    }

    pub async fn pop(&self) -> Option<GhostCommit> {
        self.snapshots.lock().await.pop()
    }

    pub async fn is_empty(&self) -> bool {
        self.snapshots.lock().await.is_empty()
    }
}

/// Execute the undo operation: pop the most recent ghost snapshot and restore it.
///
/// Emits `UndoStarted` and `UndoCompleted` events on the provided channel.
pub async fn handle_undo(
    event_tx: &mpsc::Sender<Event>,
    id: &str,
    cwd: &Path,
    snapshots: &GhostSnapshotStack,
) {
    let _ = event_tx
        .send(Event {
            id: id.to_string(),
            msg: EventMsg::UndoStarted(UndoStartedEvent {
                message: Some("Undo in progress...".to_string()),
            }),
        })
        .await;

    let snapshot = snapshots.pop().await;

    let completed = match snapshot {
        None => {
            warn!("Undo requested but no snapshots available");
            UndoCompletedEvent {
                success: false,
                message: Some("No snapshot available to undo.".to_string()),
            }
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
                    UndoCompletedEvent {
                        success: true,
                        message: Some(format!("Undo restored snapshot {short_id}.")),
                    }
                }
                Ok(Err(err)) => {
                    let message = format!("Failed to restore snapshot {commit_id}: {err}");
                    warn!("{message}");
                    UndoCompletedEvent {
                        success: false,
                        message: Some(message),
                    }
                }
                Err(err) => {
                    let message = format!("Failed to restore snapshot {commit_id}: {err}");
                    error!("{message}");
                    UndoCompletedEvent {
                        success: false,
                        message: Some(message),
                    }
                }
            }
        }
    };

    let _ = event_tx
        .send(Event {
            id: id.to_string(),
            msg: EventMsg::UndoCompleted(completed),
        })
        .await;
}
