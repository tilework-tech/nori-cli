#![cfg(not(target_os = "windows"))]

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use codex_git::CreateGhostCommitOptions;
use codex_git::create_ghost_commit;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::UndoCompletedEvent;
use nori_acp::undo::GhostSnapshotStack;
use pretty_assertions::assert_eq;
use std::fs;
use std::path::Path;
use std::process::Command;
use tokio::sync::mpsc;
use tokio::time;

fn git(path: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(path)
        .status()
        .with_context(|| format!("failed to run git {args:?}"))?;
    if status.success() {
        return Ok(());
    }
    bail!("git {args:?} exited with {status}");
}

fn init_git_repo(path: &Path) -> Result<()> {
    git(path, &["init", "--initial-branch=main"])?;
    git(path, &["config", "core.autocrlf", "false"])?;
    git(path, &["config", "user.name", "Test"])?;
    git(path, &["config", "user.email", "test@example.com"])?;
    let readme = path.join("README.txt");
    fs::write(&readme, "init\n")?;
    git(path, &["add", "README.txt"])?;
    git(path, &["commit", "-m", "init"])?;
    Ok(())
}

fn create_snapshot(path: &Path) -> Result<codex_git::GhostCommit> {
    let options = CreateGhostCommitOptions::new(path);
    Ok(create_ghost_commit(&options)?)
}

async fn collect_undo_completed(rx: &mut mpsc::Receiver<Event>) -> Result<UndoCompletedEvent> {
    let mut found_started = false;
    loop {
        let event = rx
            .recv()
            .await
            .context("event channel closed unexpectedly")?;
        match event.msg {
            EventMsg::UndoStarted(_) => {
                found_started = true;
            }
            EventMsg::UndoCompleted(completed) => {
                assert!(found_started, "UndoCompleted received before UndoStarted");
                return Ok(completed);
            }
            other => bail!("unexpected event: {other:?}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn undo_with_no_snapshots_reports_failure() -> Result<()> {
    let (event_tx, mut event_rx) = mpsc::channel(32);
    let snapshots = GhostSnapshotStack::new();
    let tmp = tempfile::tempdir()?;

    nori_acp::undo::handle_undo(&event_tx, "test-1", tmp.path(), &snapshots).await;

    let completed = collect_undo_completed(&mut event_rx).await?;
    assert!(!completed.success);
    assert_eq!(
        completed.message.as_deref(),
        Some("No snapshot available to undo.")
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn undo_restores_file_after_modification() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    init_git_repo(tmp.path())?;

    let tracked = tmp.path().join("file.txt");
    fs::write(&tracked, "before\n")?;
    git(tmp.path(), &["add", "file.txt"])?;
    git(tmp.path(), &["commit", "-m", "add file"])?;

    // Create snapshot before modification
    let snapshot = create_snapshot(tmp.path())?;
    let snapshots = GhostSnapshotStack::new();
    snapshots.push(snapshot, "modify file".to_string()).await;

    // Simulate agent modifying the file
    fs::write(&tracked, "after\n")?;
    assert_eq!(fs::read_to_string(&tracked)?, "after\n");

    // Undo
    let (event_tx, mut event_rx) = mpsc::channel(32);
    nori_acp::undo::handle_undo(&event_tx, "test-2", tmp.path(), &snapshots).await;

    let completed = collect_undo_completed(&mut event_rx).await?;
    assert!(completed.success, "undo failed: {:?}", completed.message);
    assert_eq!(fs::read_to_string(&tracked)?, "before\n");
    assert!(snapshots.is_empty().await);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sequential_undos_consume_snapshots() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    init_git_repo(tmp.path())?;

    let file = tmp.path().join("story.txt");
    fs::write(&file, "v1\n")?;
    git(tmp.path(), &["add", "story.txt"])?;
    git(tmp.path(), &["commit", "-m", "v1"])?;

    // Snapshot before turn 1
    let snap1 = create_snapshot(tmp.path())?;
    fs::write(&file, "v2\n")?;
    git(tmp.path(), &["add", "story.txt"])?;
    git(tmp.path(), &["commit", "-m", "v2"])?;

    // Snapshot before turn 2
    let snap2 = create_snapshot(tmp.path())?;
    fs::write(&file, "v3\n")?;
    git(tmp.path(), &["add", "story.txt"])?;
    git(tmp.path(), &["commit", "-m", "v3"])?;

    // Snapshot before turn 3
    let snap3 = create_snapshot(tmp.path())?;
    fs::write(&file, "v4\n")?;

    let snapshots = GhostSnapshotStack::new();
    snapshots.push(snap1, "turn 1".to_string()).await;
    snapshots.push(snap2, "turn 2".to_string()).await;
    snapshots.push(snap3, "turn 3".to_string()).await;

    let (event_tx, mut event_rx) = mpsc::channel(32);

    // Undo turn 3 -> back to v3
    nori_acp::undo::handle_undo(&event_tx, "u1", tmp.path(), &snapshots).await;
    let c1 = collect_undo_completed(&mut event_rx).await?;
    assert!(c1.success, "undo 1 failed: {:?}", c1.message);
    assert_eq!(fs::read_to_string(&file)?, "v3\n");

    // Undo turn 2 -> back to v2
    nori_acp::undo::handle_undo(&event_tx, "u2", tmp.path(), &snapshots).await;
    let c2 = collect_undo_completed(&mut event_rx).await?;
    assert!(c2.success, "undo 2 failed: {:?}", c2.message);
    assert_eq!(fs::read_to_string(&file)?, "v2\n");

    // Undo turn 1 -> back to v1
    nori_acp::undo::handle_undo(&event_tx, "u3", tmp.path(), &snapshots).await;
    let c3 = collect_undo_completed(&mut event_rx).await?;
    assert!(c3.success, "undo 3 failed: {:?}", c3.message);
    assert_eq!(fs::read_to_string(&file)?, "v1\n");

    // No more snapshots -> failure
    nori_acp::undo::handle_undo(&event_tx, "u4", tmp.path(), &snapshots).await;
    let c4 = collect_undo_completed(&mut event_rx).await?;
    assert!(!c4.success);
    assert_eq!(
        c4.message.as_deref(),
        Some("No snapshot available to undo.")
    );

    Ok(())
}

// ============================================================================
// Tests for list() and restore_to_index()
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_returns_snapshots_in_reverse_chronological_order() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    init_git_repo(tmp.path())?;

    let file = tmp.path().join("data.txt");

    fs::write(&file, "v1\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v1"])?;
    let snap1 = create_snapshot(tmp.path())?;

    // Small delay so timestamps differ
    time::sleep(time::Duration::from_millis(10)).await;

    fs::write(&file, "v2\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v2"])?;
    let snap2 = create_snapshot(tmp.path())?;

    time::sleep(time::Duration::from_millis(10)).await;

    fs::write(&file, "v3\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v3"])?;
    let snap3 = create_snapshot(tmp.path())?;

    let snapshots = GhostSnapshotStack::new();
    snapshots.push(snap1, "fix login bug".to_string()).await;
    snapshots.push(snap2, "add tests".to_string()).await;
    snapshots.push(snap3, "refactor auth".to_string()).await;

    let list = snapshots.list().await;
    assert_eq!(list.len(), 3);

    // Most recent first
    assert_eq!(list[0].label, "refactor auth");
    assert_eq!(list[1].label, "add tests");
    assert_eq!(list[2].label, "fix login bug");

    // Indices should be 0, 1, 2 (display order)
    assert_eq!(list[0].index, 0);
    assert_eq!(list[1].index, 1);
    assert_eq!(list[2].index, 2);

    // Each should have a non-empty short_id
    for info in &list {
        assert!(!info.short_id.is_empty());
        assert!(info.short_id.len() <= 7);
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_empty_stack_returns_empty_vec() -> Result<()> {
    let snapshots = GhostSnapshotStack::new();
    let list = snapshots.list().await;
    assert!(list.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restore_to_index_restores_correct_snapshot_and_truncates() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    init_git_repo(tmp.path())?;

    let file = tmp.path().join("data.txt");

    // v1 state
    fs::write(&file, "v1\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v1"])?;
    let snap1 = create_snapshot(tmp.path())?;

    // v2 state
    fs::write(&file, "v2\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v2"])?;
    let snap2 = create_snapshot(tmp.path())?;

    // v3 state
    fs::write(&file, "v3\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v3"])?;
    let snap3 = create_snapshot(tmp.path())?;

    // Current state: v4
    fs::write(&file, "v4\n")?;

    let snapshots = GhostSnapshotStack::new();
    snapshots.push(snap1, "turn 1".to_string()).await;
    snapshots.push(snap2, "turn 2".to_string()).await;
    snapshots.push(snap3, "turn 3".to_string()).await;

    // list() returns [snap3(idx=0), snap2(idx=1), snap1(idx=2)]
    // Selecting index 1 means "restore to snap2" and discard snap3, snap2
    let commit = snapshots.restore_to_index(1).await;
    assert!(commit.is_ok(), "restore_to_index should return a commit");

    // After restoring index 1, only snap1 should remain
    let remaining = snapshots.list().await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].label, "turn 1");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restore_to_index_zero_restores_most_recent() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    init_git_repo(tmp.path())?;

    let file = tmp.path().join("data.txt");

    fs::write(&file, "v1\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v1"])?;
    let snap1 = create_snapshot(tmp.path())?;

    fs::write(&file, "v2\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v2"])?;
    let snap2 = create_snapshot(tmp.path())?;

    let snapshots = GhostSnapshotStack::new();
    snapshots.push(snap1, "turn 1".to_string()).await;
    snapshots.push(snap2, "turn 2".to_string()).await;

    // Index 0 = most recent = snap2; restoring it removes only snap2
    let commit = snapshots.restore_to_index(0).await;
    assert!(commit.is_ok());

    let remaining = snapshots.list().await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].label, "turn 1");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restore_to_last_index_empties_stack() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    init_git_repo(tmp.path())?;

    let file = tmp.path().join("data.txt");

    fs::write(&file, "v1\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v1"])?;
    let snap1 = create_snapshot(tmp.path())?;

    fs::write(&file, "v2\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v2"])?;
    let snap2 = create_snapshot(tmp.path())?;

    let snapshots = GhostSnapshotStack::new();
    snapshots.push(snap1, "turn 1".to_string()).await;
    snapshots.push(snap2, "turn 2".to_string()).await;

    // Index 1 = oldest = snap1; restoring it removes both
    let commit = snapshots.restore_to_index(1).await;
    assert!(commit.is_ok());
    assert!(snapshots.is_empty().await);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restore_to_out_of_bounds_index_returns_none() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    init_git_repo(tmp.path())?;

    let file = tmp.path().join("data.txt");
    fs::write(&file, "v1\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v1"])?;
    let snap1 = create_snapshot(tmp.path())?;

    let snapshots = GhostSnapshotStack::new();
    snapshots.push(snap1, "turn 1".to_string()).await;

    // Index 5 is out of bounds (only 1 entry)
    let commit = snapshots.restore_to_index(5).await;
    assert!(commit.is_err());

    // Stack should be unchanged
    let remaining = snapshots.list().await;
    assert_eq!(remaining.len(), 1);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restore_to_negative_index_returns_error() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    init_git_repo(tmp.path())?;

    let file = tmp.path().join("data.txt");
    fs::write(&file, "v1\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v1"])?;
    let snap1 = create_snapshot(tmp.path())?;

    let snapshots = GhostSnapshotStack::new();
    snapshots.push(snap1, "turn 1".to_string()).await;

    let commit = snapshots.restore_to_index(-1).await;
    assert!(commit.is_err());

    // Stack should be unchanged
    let remaining = snapshots.list().await;
    assert_eq!(remaining.len(), 1);

    Ok(())
}

// ============================================================================
// Tests for handle_undo_to (full flow with filesystem verification)
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handle_undo_to_restores_selected_snapshot() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    init_git_repo(tmp.path())?;

    let file = tmp.path().join("data.txt");

    // v1 state
    fs::write(&file, "v1\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v1"])?;
    let snap1 = create_snapshot(tmp.path())?;

    // v2 state
    fs::write(&file, "v2\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v2"])?;
    let snap2 = create_snapshot(tmp.path())?;

    // v3 state
    fs::write(&file, "v3\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v3"])?;
    let snap3 = create_snapshot(tmp.path())?;

    // Current state: v4
    fs::write(&file, "v4\n")?;

    let snapshots = GhostSnapshotStack::new();
    snapshots.push(snap1, "write v1".to_string()).await;
    snapshots.push(snap2, "write v2".to_string()).await;
    snapshots.push(snap3, "write v3".to_string()).await;

    let (event_tx, mut event_rx) = mpsc::channel(32);

    // Undo to index 1 (snap2) — should restore to v2 state
    nori_acp::undo::handle_undo_to(&event_tx, "ut1", tmp.path(), &snapshots, 1).await;

    let completed = collect_undo_completed(&mut event_rx).await?;
    assert!(completed.success, "undo_to failed: {:?}", completed.message);

    // File should be at v2
    assert_eq!(fs::read_to_string(&file)?, "v2\n");

    // Only snap1 should remain
    let remaining = snapshots.list().await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].label, "write v1");

    // Completion message should contain agent warning
    let msg = completed.message.unwrap();
    assert!(
        msg.contains("agent"),
        "completion message should warn about agent unawareness: {msg}"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handle_undo_to_out_of_bounds_reports_failure() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    init_git_repo(tmp.path())?;

    let file = tmp.path().join("data.txt");
    fs::write(&file, "v1\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v1"])?;
    let snap1 = create_snapshot(tmp.path())?;

    let snapshots = GhostSnapshotStack::new();
    snapshots.push(snap1, "turn 1".to_string()).await;

    let (event_tx, mut event_rx) = mpsc::channel(32);
    nori_acp::undo::handle_undo_to(&event_tx, "ut2", tmp.path(), &snapshots, 10).await;

    let completed = collect_undo_completed(&mut event_rx).await?;
    assert!(!completed.success);

    // Stack should be unchanged
    let remaining = snapshots.list().await;
    assert_eq!(remaining.len(), 1);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handle_undo_to_empty_stack_reports_failure() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let snapshots = GhostSnapshotStack::new();

    let (event_tx, mut event_rx) = mpsc::channel(32);
    nori_acp::undo::handle_undo_to(&event_tx, "ut3", tmp.path(), &snapshots, 0).await;

    let completed = collect_undo_completed(&mut event_rx).await?;
    assert!(!completed.success);
    assert_eq!(
        completed.message.as_deref(),
        Some("No snapshot available to undo.")
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handle_undo_to_negative_index_reports_failure() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    init_git_repo(tmp.path())?;

    let file = tmp.path().join("data.txt");
    fs::write(&file, "v1\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v1"])?;
    let snap1 = create_snapshot(tmp.path())?;

    let snapshots = GhostSnapshotStack::new();
    snapshots.push(snap1, "turn 1".to_string()).await;

    let (event_tx, mut event_rx) = mpsc::channel(32);
    nori_acp::undo::handle_undo_to(&event_tx, "ut-neg", tmp.path(), &snapshots, -1).await;

    let completed = collect_undo_completed(&mut event_rx).await?;
    assert!(!completed.success);

    // Stack should be unchanged
    let remaining = snapshots.list().await;
    assert_eq!(remaining.len(), 1);

    Ok(())
}

// ============================================================================
// Tests for handle_list_snapshots
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handle_list_snapshots_sends_event_with_entries() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    init_git_repo(tmp.path())?;

    let file = tmp.path().join("data.txt");

    fs::write(&file, "v1\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v1"])?;
    let snap1 = create_snapshot(tmp.path())?;

    fs::write(&file, "v2\n")?;
    git(tmp.path(), &["add", "data.txt"])?;
    git(tmp.path(), &["commit", "-m", "v2"])?;
    let snap2 = create_snapshot(tmp.path())?;

    let snapshots = GhostSnapshotStack::new();
    snapshots.push(snap1, "fix bug".to_string()).await;
    snapshots.push(snap2, "add feature".to_string()).await;

    let (event_tx, mut event_rx) = mpsc::channel(32);
    nori_acp::undo::handle_list_snapshots(&event_tx, "ls1", &snapshots).await;

    let event = event_rx.recv().await.context("no event received")?;
    match event.msg {
        EventMsg::UndoListResult(result) => {
            assert_eq!(result.snapshots.len(), 2);
            assert_eq!(result.snapshots[0].label, "add feature");
            assert_eq!(result.snapshots[1].label, "fix bug");
            assert_eq!(result.snapshots[0].index, 0);
            assert_eq!(result.snapshots[1].index, 1);
        }
        other => bail!("expected UndoListResult, got: {other:?}"),
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handle_list_snapshots_empty_sends_empty_list() -> Result<()> {
    let snapshots = GhostSnapshotStack::new();

    let (event_tx, mut event_rx) = mpsc::channel(32);
    nori_acp::undo::handle_list_snapshots(&event_tx, "ls2", &snapshots).await;

    let event = event_rx.recv().await.context("no event received")?;
    match event.msg {
        EventMsg::UndoListResult(result) => {
            assert!(result.snapshots.is_empty());
        }
        other => bail!("expected UndoListResult, got: {other:?}"),
    }

    Ok(())
}
