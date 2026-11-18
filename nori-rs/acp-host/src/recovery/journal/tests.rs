use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::tempdir;

const BINDING: &str = "example-binding";
const LIMIT: usize = 1024 * 1024;

#[test]
fn lost_ack_replays_original_source_and_raw_payload() -> Result<()> {
    let root = tempdir()?;
    let mut journal = Journal::open(root.path(), LIMIT)?;
    journal.register(BINDING, 4)?;
    let update = json!({"method":"session/update","params":{"futureField":{"nested":[null,0]}}});
    let first = journal.append(BINDING, 4, "main", &update)?;
    let second = journal.append(
        BINDING,
        4,
        "main",
        &json!({"result":{"stopReason":"end_turn"}}),
    )?;
    drop(journal);

    let mut restarted = Journal::open(root.path(), LIMIT)?;
    assert_ne!(restarted.incarnation(), first.source.host_incarnation);
    assert_eq!(
        restarted.replay(BINDING, 4)?,
        vec![first.clone(), second.clone()]
    );
    let after = restarted.append(BINDING, 4, "main", &update)?;
    assert_ne!(after.source.host_incarnation, first.source.host_incarnation);
    let mut bad = second.source.clone();
    bad.sequence += 1;
    assert!(restarted.ack(BINDING, 4, &bad).is_err());
    restarted.ack(BINDING, 4, &second.source)?;
    restarted.ack(BINDING, 4, &first.source)?;
    assert_eq!(restarted.replay(BINDING, 4)?, vec![after]);
    assert!(
        restarted
            .export(BINDING, 4)?
            .watermarks()
            .contains(&second.source)
    );
    Ok(())
}

#[test]
fn retries_reconcile_without_dispatch_across_restart_and_import() -> Result<()> {
    let root = tempdir()?;
    let request = json!({"method":"session/prompt","sessionId":"main","text":"sample"});
    let outcome = json!({"stopReason":"end_turn","vendorField":true});
    let mut journal = Journal::open(root.path(), LIMIT)?;
    journal.register(BINDING, 1)?;
    assert_eq!(
        journal.admit(BINDING, 1, "pending", &request)?,
        Admission::Dispatch
    );
    assert_eq!(
        journal.admit(BINDING, 1, "pending", &request)?,
        Admission::InFlight
    );
    assert_eq!(
        journal.admit(BINDING, 1, "done", &request)?,
        Admission::Dispatch
    );
    journal.complete(BINDING, 1, "done", &outcome)?;
    drop(journal);

    let mut journal = Journal::open(root.path(), LIMIT)?;
    assert_eq!(
        journal.admit(BINDING, 1, "pending", &request)?,
        Admission::Interrupted
    );
    assert!(journal.complete(BINDING, 1, "pending", &outcome).is_err());
    assert!(
        journal
            .admit(BINDING, 1, "done", &json!({"method":"session/new"}))
            .is_err()
    );
    let record = journal.append(BINDING, 1, "main", &json!({"method":"session/update"}))?;
    let snapshot = journal.export(BINDING, 1)?;
    let fresh = tempdir()?;
    let mut restored = Journal::open(fresh.path(), LIMIT)?;
    assert!(restored.import("other", 2, &snapshot).is_err());
    assert!(restored.import(BINDING, 1, &snapshot).is_err());
    restored.import(BINDING, 2, &snapshot)?;
    assert!(restored.admit(BINDING, 1, "late", &request).is_err());
    assert!(restored.ack(BINDING, 1, &record.source).is_err());
    assert_eq!(restored.replay(BINDING, 2)?, vec![record.clone()]);
    restored.ack(BINDING, 2, &record.source)?;
    assert_eq!(
        restored.admit(BINDING, 2, "pending", &request)?,
        Admission::Interrupted
    );
    assert_eq!(
        restored.admit(BINDING, 2, "done", &request)?,
        Admission::Completed(outcome)
    );
    Ok(())
}

#[test]
fn release_tombstone_survives_restart_and_is_scoped() -> Result<()> {
    let root = tempdir()?;
    let request = json!({"method":"release"});
    let result = json!({"released":true});
    let mut journal = Journal::open(root.path(), LIMIT)?;
    journal.register(BINDING, 2)?;
    journal.register("sibling", 2)?;
    assert_eq!(
        journal.admit_release(BINDING, 2, "release", &request)?,
        Admission::Dispatch
    );
    assert!(journal.admit(BINDING, 2, "late", &json!("prompt")).is_err());
    journal.complete_release(BINDING, 2, "release", &result)?;
    drop(journal);

    let mut journal = Journal::open(root.path(), LIMIT)?;
    journal.register(BINDING, 2)?;
    assert_eq!(
        journal.admit_release(BINDING, 2, "release", &request)?,
        Admission::Completed(result)
    );
    assert!(journal.admit(BINDING, 2, "late", &json!("prompt")).is_err());
    assert!(journal.register(BINDING, 3).is_err());
    assert_eq!(
        journal.admit("sibling", 2, "work", &json!("prompt"))?,
        Admission::Dispatch
    );
    Ok(())
}

#[test]
fn backpressure_blocks_new_turns_until_ack_without_losing_events() -> Result<()> {
    let root = tempdir()?;
    let limit = 4096;
    let mut journal = Journal::open(root.path(), limit)?;
    journal.register(BINDING, 1)?;
    let message = json!({"text":"x".repeat(2200)});
    let first = journal.append(BINDING, 1, "main", &message)?;
    assert!(journal.append(BINDING, 1, "main", &message).is_err());
    assert!(journal.admit(BINDING, 1, "work", &json!("prompt")).is_err());
    drop(journal);
    let mut journal = Journal::open(root.path(), limit)?;
    assert!(journal.admit(BINDING, 1, "work", &json!("prompt")).is_err());
    assert_eq!(journal.replay(BINDING, 1)?, vec![first.clone()]);
    journal.ack(BINDING, 1, &first.source)?;
    assert_eq!(
        journal.admit(BINDING, 1, "work", &json!("prompt"))?,
        Admission::Dispatch
    );
    Ok(())
}

#[test]
fn cold_import_keeps_identity_and_never_rewinds_completed_work_on_retry() -> Result<()> {
    let root = tempdir()?;
    let mut journal = Journal::open(root.path(), LIMIT)?;
    journal.register_identity(
        BINDING,
        3,
        "example-conversation",
        Some("example-transcript"),
    )?;
    assert!(
        journal
            .register_identity(BINDING, 3, "other", Some("example-transcript"))
            .is_err()
    );
    assert!(
        journal
            .register_identity(BINDING, 3, "example-conversation", None)
            .is_err()
    );
    let snapshot = journal.export(BINDING, 3)?;
    assert_eq!(snapshot.conversation_id(), Some("example-conversation"));
    assert_eq!(snapshot.transcript_id(), Some("example-transcript"));
    let destination = tempdir()?;
    let mut restored = Journal::open(destination.path(), LIMIT)?;
    restored.import(BINDING, 4, &snapshot)?;
    let prompt = json!({"method":"session/prompt","sessionId":"main"});
    let result = json!({"id":8,"result":{"stopReason":"end_turn"}});
    restored.admit(BINDING, 4, "turn", &prompt)?;
    let record = restored.finish_prompt(BINDING, 4, "turn", "main", &result)?;
    restored.import(BINDING, 4, &snapshot)?;
    assert_eq!(
        restored.command(BINDING, 4, "turn")?,
        Admission::Completed(result)
    );
    assert_eq!(restored.replay(BINDING, 4)?, vec![record]);
    journal.admit(BINDING, 3, "different", &prompt)?;
    assert!(
        restored
            .import(BINDING, 4, &journal.export(BINDING, 3)?)
            .is_err()
    );
    Ok(())
}

#[test]
fn only_one_host_owns_the_journal_directory() -> Result<()> {
    let root = tempdir()?;
    let first = Journal::open(root.path(), LIMIT)?;
    assert!(Journal::open(root.path(), LIMIT).is_err());
    drop(first);
    assert!(Journal::open(root.path(), LIMIT).is_ok());
    Ok(())
}

#[test]
fn oversized_final_response_neither_completes_command_nor_advances_output() -> Result<()> {
    let root = tempdir()?;
    let mut journal = Journal::open(root.path(), 4096)?;
    journal.register(BINDING, 1)?;
    journal.admit(BINDING, 1, "turn", &json!("prompt"))?;
    let result = json!({"result":{"oversize":"a".repeat(5000)}});
    assert!(
        journal
            .finish_prompt(BINDING, 1, "turn", "main", &result)
            .is_err()
    );
    assert!(journal.replay(BINDING, 1)?.is_empty());
    drop(journal);
    let journal = Journal::open(root.path(), 4096)?;
    assert_eq!(journal.command(BINDING, 1, "turn")?, Admission::Interrupted);
    Ok(())
}

#[cfg(unix)]
#[test]
fn failed_disk_write_closes_admission_and_preserves_last_durable_output() -> Result<()> {
    let root = tempdir()?;
    let dir = root.path().join("journal");
    let detached = root.path().join("detached");
    let mut journal = Journal::open(&dir, LIMIT)?;
    journal.register(BINDING, 1)?;
    let first = journal.append(BINDING, 1, "main", &json!("saved"))?;
    std::fs::rename(&dir, &detached)?;
    let write = journal.append(BINDING, 1, "main", &json!("unsaved"));
    std::fs::rename(detached, &dir)?;
    assert!(write.is_err());
    assert_eq!(
        journal.saved_through(BINDING, 1)?,
        vec![first.source.clone()]
    );
    assert!(journal.admit(BINDING, 1, "late", &json!("prompt")).is_err());
    drop(journal);
    let recovered = Journal::open(&dir, LIMIT)?;
    assert_eq!(recovered.replay(BINDING, 1)?, vec![first]);
    Ok(())
}

#[test]
fn only_one_launch_can_dispatch_per_generation() -> Result<()> {
    let root = tempdir()?;
    let mut journal = Journal::open(root.path(), LIMIT)?;
    journal.register(BINDING, 1)?;
    let launch = json!({"method":"ensure","digest":"example"});
    assert_eq!(
        journal.admit_launch(BINDING, 1, "a", &launch)?,
        Admission::Dispatch
    );
    drop(journal);
    let mut journal = Journal::open(root.path(), LIMIT)?;
    assert_eq!(
        journal.admit_launch(BINDING, 1, "a", &launch)?,
        Admission::Interrupted
    );
    assert!(journal.admit_launch(BINDING, 1, "b", &launch).is_err());
    let previous = journal.export(BINDING, 1)?;
    journal.import(BINDING, 2, &previous)?;
    assert_eq!(
        journal.admit_launch(BINDING, 2, "b", &launch)?,
        Admission::Dispatch
    );
    Ok(())
}

#[test]
fn repeated_acknowledged_output_stays_within_disk_limit() -> Result<()> {
    let root = tempdir()?;
    let cap = 4096;
    let mut journal = Journal::open(root.path(), cap)?;
    journal.register(BINDING, 1)?;
    for step in 1..=80 {
        let event = journal.append(BINDING, 1, "main", &json!({"step":step,"text":"example"}))?;
        journal.ack(BINDING, 1, &event.source)?;
    }
    drop(journal);
    let recovered = Journal::open(root.path(), cap)?;
    assert_eq!(recovered.saved_through(BINDING, 1)?[0].sequence, 80);
    assert!(recovered.replay(BINDING, 1)?.is_empty());
    let bytes: u64 = std::fs::read_dir(root.path())?
        .map(|f| f.and_then(|f| Ok(f.metadata()?.len())))
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .sum();
    assert!(bytes <= u64::try_from(cap)?);
    Ok(())
}

#[test]
fn incomplete_final_frame_is_discarded_but_checksum_failure_stops_recovery() -> Result<()> {
    use std::io::Write;

    let root = tempdir()?;
    let mut journal = Journal::open(root.path(), LIMIT)?;
    journal.register(BINDING, 1)?;
    let committed = journal.append(BINDING, 1, "main", &json!("committed"))?;
    drop(journal);
    let path = std::fs::read_dir(root.path())?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| path.extension().is_some_and(|extension| extension == "wal"))
        .context("missing test journal")?;
    let original = std::fs::read(&path)?;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)?
        .write_all(&[40, 0, 0])?;
    let recovered = Journal::open(root.path(), LIMIT)?;
    assert_eq!(recovered.replay(BINDING, 1)?, vec![committed]);
    assert_eq!(std::fs::read(&path)?, original);
    drop(recovered);
    let mut damaged = original;
    let last = damaged.last_mut().context("missing committed frame")?;
    *last ^= 1;
    std::fs::write(path, damaged)?;
    let recovered = Journal::open(root.path(), LIMIT)?;
    assert!(recovered.contains(BINDING).is_err());
    Ok(())
}

#[test]
fn a_lost_producer_interrupts_prompts_without_interrupting_other_commands() -> Result<()> {
    let root = tempdir()?;
    let mut journal = Journal::open(root.path(), LIMIT)?;
    journal.register(BINDING, 1)?;
    journal.admit_launch(BINDING, 1, "launch", &json!("ensure"))?;
    journal.admit(BINDING, 1, "turn", &json!("prompt"))?;
    journal.interrupt_prompts(BINDING, 1)?;
    assert_eq!(journal.command(BINDING, 1, "turn")?, Admission::Interrupted);
    assert_eq!(journal.command(BINDING, 1, "launch")?, Admission::InFlight);
    Ok(())
}
