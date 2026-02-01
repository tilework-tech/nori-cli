#![cfg(not(target_os = "windows"))]

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use codex_acp::undo::GhostSnapshotStack;
use codex_git::CreateGhostCommitOptions;
use codex_git::create_ghost_commit;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::UndoCompletedEvent;
use pretty_assertions::assert_eq;
use std::fs;
use std::path::Path;
use std::process::Command;
use tokio::sync::mpsc;

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

    codex_acp::undo::handle_undo(&event_tx, "test-1", tmp.path(), &snapshots).await;

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
    snapshots.push(snapshot).await;

    // Simulate agent modifying the file
    fs::write(&tracked, "after\n")?;
    assert_eq!(fs::read_to_string(&tracked)?, "after\n");

    // Undo
    let (event_tx, mut event_rx) = mpsc::channel(32);
    codex_acp::undo::handle_undo(&event_tx, "test-2", tmp.path(), &snapshots).await;

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
    snapshots.push(snap1).await;
    snapshots.push(snap2).await;
    snapshots.push(snap3).await;

    let (event_tx, mut event_rx) = mpsc::channel(32);

    // Undo turn 3 -> back to v3
    codex_acp::undo::handle_undo(&event_tx, "u1", tmp.path(), &snapshots).await;
    let c1 = collect_undo_completed(&mut event_rx).await?;
    assert!(c1.success, "undo 1 failed: {:?}", c1.message);
    assert_eq!(fs::read_to_string(&file)?, "v3\n");

    // Undo turn 2 -> back to v2
    codex_acp::undo::handle_undo(&event_tx, "u2", tmp.path(), &snapshots).await;
    let c2 = collect_undo_completed(&mut event_rx).await?;
    assert!(c2.success, "undo 2 failed: {:?}", c2.message);
    assert_eq!(fs::read_to_string(&file)?, "v2\n");

    // Undo turn 1 -> back to v1
    codex_acp::undo::handle_undo(&event_tx, "u3", tmp.path(), &snapshots).await;
    let c3 = collect_undo_completed(&mut event_rx).await?;
    assert!(c3.success, "undo 3 failed: {:?}", c3.message);
    assert_eq!(fs::read_to_string(&file)?, "v1\n");

    // No more snapshots -> failure
    codex_acp::undo::handle_undo(&event_tx, "u4", tmp.path(), &snapshots).await;
    let c4 = collect_undo_completed(&mut event_rx).await?;
    assert!(!c4.success);
    assert_eq!(
        c4.message.as_deref(),
        Some("No snapshot available to undo.")
    );

    Ok(())
}
