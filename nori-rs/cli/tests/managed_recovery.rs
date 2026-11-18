#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod recovery;

use pretty_assertions::assert_eq;
use recovery::Binding;
use recovery::Host;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn restart_replays_the_original_event_until_its_source_cursor_is_acknowledged() {
    let home = TempDir::new().unwrap();
    let binding = Binding::new(home.path());
    let mut host = Host::start(home.path(), true).await;
    let mut wire = host.connect().await;
    let session = wire.start(&binding, 1).await;
    wire.send_prompt(&binding, 1, &session, "turn-a", "record")
        .await;
    let delivered = wire.text_event("record").await;
    let cursor = &delivered["source"];
    assert_eq!(cursor["bindingId"], binding.id);
    assert_eq!(cursor["generation"], 1);
    assert!(
        cursor["hostIncarnation"]
            .as_str()
            .is_some_and(|id| !id.is_empty()),
        "{cursor}"
    );
    assert!(
        cursor["sequence"].as_i64().is_some_and(|seq| seq > 0),
        "{cursor}"
    );

    host.crash();
    let host = Host::start(home.path(), true).await;
    let mut wire = host.connect().await;
    let replay = wire.replay(&binding, 1).await;
    assert!(replay.contains(&delivered), "{replay:?}");
    wire.ack(&binding, 1, &delivered).await;
    drop(host);
    let host = Host::start(home.path(), true).await;
    let mut wire = host.connect().await;
    let replay = wire.replay(&binding, 1).await;
    assert!(!replay.contains(&delivered), "{replay:?}");
}

#[tokio::test]
async fn an_interrupted_prompt_never_repeats_its_side_effect() {
    let home = TempDir::new().unwrap();
    let binding = Binding::new(home.path());
    let mut host = Host::start(home.path(), true).await;
    let mut wire = host.connect().await;
    let session = wire.start(&binding, 1).await;
    wire.send_prompt(&binding, 1, &session, "turn-b", "hold")
        .await;
    wire.text_event("hold").await;
    assert_eq!(binding.side_effects(), ["hold"]);
    host.crash();

    let host = Host::start(home.path(), true).await;
    let mut wire = host.connect().await;
    let command = wire
        .client
        .request(
            "_nori/host/command",
            json!({
                "bindingId": binding.id, "generation": 1, "commandId": "turn-b",
            }),
        )
        .await
        .unwrap();
    assert_eq!(command["status"], "interrupted");
    wire.send_prompt(&binding, 1, &session, "turn-b", "hold")
        .await;
    let retry = wire.prompt_reply().await;
    assert!(
        retry["error"]["message"]
            .as_str()
            .unwrap()
            .contains("interrupted"),
        "{retry}"
    );
    assert_eq!(binding.side_effects(), ["hold"]);
}

#[tokio::test]
async fn a_checkpoint_imports_the_stable_identity_into_a_fresh_host_and_fences_old_commands() {
    cold_import(true).await;
}

#[tokio::test]
async fn cold_import_reports_limited_continuation_for_an_agent_without_load() {
    cold_import(false).await;
}

async fn cold_import(supports_load: bool) {
    let home = TempDir::new().unwrap();
    let binding = Binding::new(home.path());
    let host = Host::start(home.path(), true).await;
    if !supports_load {
        binding.disable_load();
    }
    let mut source = host.connect().await;
    let session = source.start(&binding, 1).await;
    source
        .send_prompt(&binding, 1, &session, "turn-c", "before")
        .await;
    let prior_event = source.text_event("before").await;
    assert_eq!(
        source.prompt_reply().await["result"]["stopReason"],
        "end_turn"
    );
    let checkpoint = source.checkpoint(&binding, 1).await;
    drop(host);

    let other_home = TempDir::new().unwrap();
    let destination = binding.relocated(other_home.path());
    drop(binding);
    if !supports_load {
        destination.disable_load();
    }
    let other_host = Host::start(other_home.path(), true).await;
    let mut restored = other_host.connect().await;
    let imported = restored.import(&destination, 2, checkpoint).await;
    assert_eq!(imported["conversationId"], destination.conversation_id);
    let mode = if supports_load { "load" } else { "fresh" };
    assert_eq!(imported["continuation"], mode);
    let stale_ack = json!({
        "bindingId": destination.id, "generation": 1, "source": prior_event["source"],
    });
    assert!(
        restored
            .client
            .request("_nori/host/ack", stale_ack)
            .await
            .is_err()
    );
    let backlog = restored.replay(&destination, 2).await;
    assert!(backlog.contains(&prior_event), "{backlog:?}");
    let capabilities = restored.initialize(&destination, 2).await;
    assert_eq!(
        capabilities["agentCapabilities"]["loadSession"],
        supports_load
    );
    let resume_id = if supports_load {
        restored.load(&destination, 2, &session).await;
        session.clone()
    } else {
        let fresh = restored.new_session(&destination, 2).await;
        assert_ne!(fresh, session);
        fresh
    };
    restored
        .send_prompt(&destination, 1, &resume_id, "old", "stale")
        .await;
    assert!(restored.prompt_reply().await["error"].is_object());
    restored
        .send_prompt(&destination, 2, &resume_id, "fresh", "after")
        .await;
    restored.text_event("after").await;
    assert_eq!(
        restored.prompt_reply().await["result"]["stopReason"],
        "end_turn"
    );
    assert_eq!(destination.side_effects(), ["after"]);
}

#[tokio::test]
async fn an_additional_acp_session_stays_out_of_the_bindings_recovery_replay() {
    let home = TempDir::new().unwrap();
    let binding = Binding::new(home.path());
    let host = Host::start(home.path(), true).await;
    let mut wire = host.connect().await;
    let main = wire.start(&binding, 1).await;
    let side = wire.new_session(&binding, 1).await;
    wire.send_prompt(&binding, 1, &side, "ephemeral", "side reply")
        .await;
    let side_output = wire.text_event("side reply").await;
    assert!(side_output["source"].is_null());
    assert_eq!(
        wire.prompt_reply().await["result"]["stopReason"],
        "end_turn"
    );
    wire.send_prompt(&binding, 1, &main, "recorded", "main reply")
        .await;
    let main_output = wire.text_event("main reply").await;
    assert_eq!(
        wire.prompt_reply().await["result"]["stopReason"],
        "end_turn"
    );
    let records = wire.replay(&binding, 1).await;
    assert!(records.contains(&main_output));
    assert!(records.iter().all(|r| r["sessionId"] != side));
}

#[tokio::test]
async fn a_checkpoint_cannot_report_a_completed_barrier_while_its_prompt_is_blocked() {
    let home = TempDir::new().unwrap();
    let binding = Binding::new(home.path());
    let host = Host::start(home.path(), true).await;
    let mut wire = host.connect().await;
    let session = wire.start(&binding, 1).await;
    wire.send_prompt(&binding, 1, &session, "working", "hold")
        .await;
    let event = wire.text_event("hold").await;
    let params = json!({"bindingId":binding.id,"generation":1});
    {
        let request = wire.client.request("_nori/host/checkpoint", params.clone());
        tokio::pin!(request);
        let early = tokio::time::timeout(std::time::Duration::from_millis(250), &mut request).await;
        assert!(
            !matches!(early, Ok(Ok(_))),
            "checkpoint returned success during an active prompt: {early:?}"
        );
        binding.unblock();
        if early.is_err() {
            let _ = request.await;
        }
    }
    assert_eq!(
        wire.prompt_reply().await["result"]["stopReason"],
        "end_turn"
    );
    let export = wire
        .client
        .request("_nori/host/checkpoint", params)
        .await
        .unwrap();
    let watermark = export["watermarks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| {
            source["generation"] == 1
                && source["hostIncarnation"] == event["source"]["hostIncarnation"]
        })
        .expect("missing producing stream");
    assert!(
        watermark["sequence"].as_i64().unwrap() >= event["source"]["sequence"].as_i64().unwrap()
    );
}

#[tokio::test]
async fn releasing_one_binding_twice_leaves_a_siblings_in_flight_prompt_alive() {
    let home = TempDir::new().unwrap();
    let a = Binding::new(home.path());
    let b = Binding::new(home.path());
    let host = Host::start(home.path(), true).await;
    let mut wire = host.connect().await;
    let a_session = wire.start(&a, 1).await;
    let b_session = wire.start(&b, 1).await;
    wire.send_prompt(&b, 1, &b_session, "turn-d", "hold").await;
    wire.text_event("hold").await;
    let first = wire.release(&a, 1, "release-a").await;
    assert_eq!(first, wire.release(&a, 1, "release-a").await);
    b.unblock();
    assert_eq!(
        wire.prompt_reply().await["result"]["stopReason"],
        "end_turn"
    );
    wire.send_prompt(&a, 1, &a_session, "closed", "late").await;
    assert!(wire.prompt_reply().await["error"].is_object());
    assert_eq!(b.side_effects(), ["hold"]);
    assert!(a.side_effects().is_empty());
}

#[tokio::test]
async fn local_mode_keeps_its_session_list_and_rejects_managed_checkpoint_requests() {
    let home = TempDir::new().unwrap();
    let host = Host::start(home.path(), false).await;
    let mut wire = host.connect().await;
    let listing = wire
        .client
        .request("_nori/session/list", json!({}))
        .await
        .unwrap();
    assert_eq!(listing["sessions"], json!([]));
    assert!(
        wire.client
            .request(
                "_nori/host/checkpoint",
                json!({"bindingId": "fixture", "generation": 1})
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn disconnected_machine_output_is_saved_and_its_live_lane_can_reattach() {
    let home = TempDir::new().unwrap();
    let binding = Binding::new(home.path());
    let host = Host::start(home.path(), true).await;
    let mut first = host.connect().await;
    let session = first.start(&binding, 1).await;
    first
        .send_prompt(&binding, 1, &session, "while-detached", "hold")
        .await;
    first.text_event("hold").await;
    drop(first);
    binding.unblock();
    let mut reconnected = host.connect().await;
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if reconnected
                .replay(&binding, 1)
                .await
                .iter()
                .any(|r| r.pointer("/message/result/stopReason") == Some(&json!("end_turn")))
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("adapter reply journaled with its machine socket gone");
    reconnected
        .send_prompt(&binding, 1, &session, "while-detached", "hold")
        .await;
    assert_eq!(
        reconnected.prompt_reply().await["result"]["stopReason"],
        "end_turn"
    );
    reconnected
        .send_prompt(&binding, 1, &session, "next", "continue")
        .await;
    reconnected.text_event("continue").await;
    assert_eq!(
        reconnected.prompt_reply().await["result"]["stopReason"],
        "end_turn"
    );
    assert_eq!(binding.side_effects(), ["hold", "continue"]);
}

#[tokio::test]
async fn rejected_import_leaves_the_resident_generation_usable() {
    let home = TempDir::new().unwrap();
    let binding = Binding::new(home.path());
    let host = Host::start(home.path(), true).await;
    let mut wire = host.connect().await;
    let session = wire.start(&binding, 1).await;
    let checkpoint = wire.checkpoint(&binding, 1).await;
    let rejected = wire.client.request("_nori/host/import", json!({
        "bindingId":binding.id,"generation":2,"checkpoint":checkpoint,"launchSpec":binding.spec()
    })).await;
    assert!(rejected.is_err());
    wire.send_prompt(&binding, 1, &session, "resident", "still-live")
        .await;
    wire.text_event("still-live").await;
    assert_eq!(
        wire.prompt_reply().await["result"]["stopReason"],
        "end_turn"
    );
}

#[tokio::test]
async fn a_drained_release_can_finish_after_a_crash_before_ledger_completion() {
    let home = TempDir::new().unwrap();
    let binding = Binding::new(home.path());
    let mut host = Host::start(home.path(), true).await;
    let mut wire = host.connect().await;
    wire.start(&binding, 1).await;
    let expected = wire.release(&binding, 1, "cleanup").await;
    host.crash();

    // Recreate the durable cut between the final checkpoint rename and the tombstone append.
    let wal = std::fs::read_dir(home.path().join("host/recovery"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|extension| extension == "wal"))
        .unwrap();
    let bytes = std::fs::read(&wal).unwrap();
    let mut offset = 0;
    let mut last = 0;
    while offset < bytes.len() {
        last = offset;
        let size = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
        offset += 40 + usize::try_from(size).unwrap();
    }
    assert_eq!(offset, bytes.len());
    std::fs::OpenOptions::new()
        .write(true)
        .open(wal)
        .unwrap()
        .set_len(u64::try_from(last).unwrap())
        .unwrap();

    let restarted = Host::start(home.path(), true).await;
    let mut wire = restarted.connect().await;
    assert_eq!(wire.release(&binding, 1, "cleanup").await, expected);
    assert_eq!(wire.release(&binding, 1, "cleanup").await, expected);
}

#[tokio::test]
async fn a_released_binding_restores_on_the_same_host_without_rolling_back_late_acks() {
    let home = TempDir::new().unwrap();
    let binding = Binding::new(home.path());
    let host = Host::start(home.path(), true).await;
    let mut wire = host.connect().await;
    let session = wire.start(&binding, 1).await;
    wire.send_prompt(&binding, 1, &session, "before-release", "example")
        .await;
    let delivered = wire.text_event("example").await;
    wire.prompt_reply().await;
    let receipt = wire.release(&binding, 1, "drain").await;
    wire.ack(&binding, 1, &delivered).await;
    let mut changed = receipt["checkpoint"].clone();
    changed["loadSession"] = json!(false);
    let params = json!({"bindingId":binding.id,"generation":2,"checkpoint":changed,"launchSpec":binding.spec()});
    assert!(
        wire.client
            .request("_nori/host/import", params)
            .await
            .is_err()
    );
    wire.import(&binding, 2, receipt["checkpoint"].clone())
        .await;
    assert!(!wire.replay(&binding, 2).await.contains(&delivered));
}

#[tokio::test]
async fn an_imported_command_receipt_cannot_select_another_bindings_private_checkpoint() {
    let source_home = TempDir::new().unwrap();
    let b = Binding::new(source_home.path());
    let source = Host::start(source_home.path(), true).await;
    let mut old = source.connect().await;
    old.start(&b, 1).await;
    let mut checkpoint = old.checkpoint(&b, 1).await;
    drop(source);

    let home = TempDir::new().unwrap();
    let a = Binding::new(home.path());
    let host = Host::start(home.path(), true).await;
    let mut client = host.connect().await;
    client.start(&a, 1).await;
    let output = client.release(&a, 1, "release-a").await;
    let mut command = output["checkpoint"]["journal"]["commands"]["release-a"].clone();
    let receipt = client
        .client
        .request(
            "_nori/host/command",
            json!({
                "bindingId":a.id,"generation":1,"commandId":"release-a"
            }),
        )
        .await
        .unwrap();
    let mut reference = receipt["response"].clone();
    reference["file"] = json!(format!("release-{}-1.json", a.conversation_id));
    command["outcome"] = json!({"completed":reference});
    checkpoint["journal"]["commands"]["release-a"] = command;

    let destination = b.relocated(home.path());
    client.import(&destination, 2, checkpoint).await;
    assert!(
        client
            .client
            .request(
                "_nori/host/release",
                json!({
                    "bindingId":destination.id,"generation":2,"commandId":"release-a"
                })
            )
            .await
            .is_err()
    );
    let own = client.release(&destination, 2, "release-b").await;
    assert_eq!(
        own["checkpoint"]["journal"]["identity"]["conversationId"],
        destination.conversation_id
    );
}

#[tokio::test]
async fn an_adapter_protocol_fault_after_a_completed_turn_fails_the_checkpoint_barrier() {
    let home = TempDir::new().unwrap();
    let binding = Binding::new(home.path());
    let host = Host::start(home.path(), true).await;
    let mut wire = host.connect().await;
    let session = wire.start(&binding, 1).await;
    wire.send_prompt(&binding, 1, &session, "bad-tail", "malformed-adapter-tail")
        .await;
    wire.text_event("malformed-adapter-tail").await;
    assert_eq!(
        wire.prompt_reply().await["result"]["stopReason"],
        "end_turn"
    );
    tokio::time::timeout(std::time::Duration::from_secs(4), async {
        loop {
            let checkpoint = wire
                .client
                .request(
                    "_nori/host/checkpoint",
                    json!({"bindingId":binding.id, "generation":1}),
                )
                .await;
            if checkpoint.is_err_and(|err| err.to_string().contains("lost durable output")) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("a corrupted tail must invalidate the source checkpoint");
}

#[tokio::test]
async fn releasing_a_restored_done_child_preserves_its_delegation_and_resident_parent() {
    let source_home = TempDir::new().unwrap();
    let child = Binding::new(source_home.path());
    let source = Host::start(source_home.path(), true).await;
    let mut old = source.connect().await;
    let child_session = old.start(&child, 1).await;
    let mut checkpoint = old.checkpoint(&child, 1).await;
    drop(source);

    let home = TempDir::new().unwrap();
    let parent = Binding::new(home.path());
    let host = Host::start(home.path(), true).await;
    let mut client = host.connect().await;
    let parent_session = client.start(&parent, 1).await;
    let terminal_history = json!({
        "conversationId": child.conversation_id, "acpSessionId": child_session,
        "parent": parent.conversation_id, "agent": "fixture", "source": "session", "status": "done",
        "delegations": [{
            "request_id": "fixture-delegation", "parent_id": parent.conversation_id,
            "child_id": child.conversation_id, "parent_agent": "fixture", "agent": "fixture",
            "task": "Inspect the sample", "result": "Sample inspected", "depth": 1,
            "status": "completed", "origin": "user"
        }]
    });
    checkpoint["history"] = terminal_history.clone();
    let destination = child.relocated(home.path());
    client.import(&destination, 2, checkpoint).await;
    let released = client.release(&destination, 2, "release-child").await;
    assert_eq!(released["checkpoint"]["history"], terminal_history);

    client
        .send_prompt(
            &parent,
            1,
            &parent_session,
            "after-child-release",
            "continue",
        )
        .await;
    client.text_event("continue").await;
    assert_eq!(
        client.prompt_reply().await["result"]["stopReason"],
        "end_turn"
    );
    assert_eq!(parent.side_effects(), ["continue"]);
}
