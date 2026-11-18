use super::*;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

const REFUSED_PROMPT: &str = "retry-after-barrier";

async fn assert_refused(wire: &mut Wire, binding: &Binding, session: &str) {
    let executed = binding.side_effects();
    wire.send_prompt(binding, 1, session, "refused", REFUSED_PROMPT)
        .await;
    let response = timeout(DEADLINE, wire.acp.as_mut().unwrap().recv())
        .await
        .unwrap()
        .unwrap();
    let lookup = wire
        .client
        .request(
            "_nori/host/command",
            json!({
                "bindingId": binding.id, "generation": 1, "commandId": "refused"
            }),
        )
        .await;
    assert!(
        lookup.is_err_and(|err| err.to_string().contains("unknown command")),
        "refused command entered the ledger"
    );
    assert_eq!(response["message"]["id"], wire.next_id, "{response}");
    assert_eq!(response["message"]["error"]["code"], -32020, "{response}");
    assert_eq!(executed, binding.side_effects());
}

#[cfg(target_os = "linux")]
async fn fault_injecting_host(home: &TempDir, marker_name: &str) -> (Host, PathBuf) {
    let library = home.path().join("fault.so");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/recovery/fail_storage_sync.c");
    assert!(
        Command::new("cc")
            .args(["-shared", "-fPIC", "-o"])
            .arg(&library)
            .arg(fixture)
            .status()
            .unwrap()
            .success()
    );
    let fault = home.path().join("fault-enabled");
    let host = Host::with_env(
        home.path(),
        true,
        &[("LD_PRELOAD", &library), (marker_name, &fault)],
    )
    .await;
    (host, fault)
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn uncertain_admission_sync_requires_lookup_in_the_reopened_journal() {
    let home = TempDir::new().unwrap();
    let binding = Binding::new(home.path());
    let (mut host, fault) = fault_injecting_host(&home, "FAIL_SYNC_WAL").await;
    let mut wire = host.connect().await;
    let session = wire.start(&binding, 1).await;
    fs::write(fault, "").unwrap();
    wire.send_prompt(&binding, 1, &session, "uncertain", "first")
        .await;
    let first = wire.prompt_reply().await;
    wire.send_prompt(&binding, 1, &session, "later", "second")
        .await;
    let later = wire.prompt_reply().await;
    assert!(binding.side_effects().is_empty());
    host.crash();

    let host = Host::start(home.path(), true).await;
    let mut wire = host.connect().await;
    let lookup = |command| json!({"bindingId":binding.id, "generation":1, "commandId":command});
    let recovered = wire
        .client
        .request("_nori/host/command", lookup("uncertain"))
        .await
        .unwrap();
    assert_eq!(recovered["status"], "interrupted");
    let absent = wire
        .client
        .request("_nori/host/command", lookup("later"))
        .await
        .unwrap_err();
    assert!(absent.to_string().contains("unknown command"));
    assert_eq!(later["error"]["code"], -32020, "{later}");
    assert_eq!(first["error"]["code"], -32021, "{first}");
}

#[cfg(target_os = "linux")]
async fn start_recording(wire: &mut Wire, b: &Binding) -> String {
    let mut spec = b.spec();
    spec["additions"]["recording"] = json!(true);
    wire.client
        .request(
            "_nori/session/ensure",
            json!({
                "bindingId": b.id, "conversationId": b.conversation_id, "commandId":"start",
                "generation":1, "recoveryVersion":1, "spec":spec
            }),
        )
        .await
        .unwrap();
    wire.initialize(b, 1).await;
    wire.new_session(b, 1).await
}

#[cfg(target_os = "linux")]
async fn assert_checkpoint_refused(wire: &mut Wire, b: &Binding) {
    let failure = wire
        .client
        .request(
            "_nori/host/checkpoint",
            json!({"bindingId":b.id,"generation":1}),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(failure.contains("-32020"), "{failure}");
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn failed_recorder_barrier_refuses_the_next_prompt_before_admission() {
    let home = TempDir::new().unwrap();
    let b = Binding::new(home.path());
    let (host, fault) = fault_injecting_host(&home, "FAIL_SYNC_JSONL").await;
    let mut wire = host.connect().await;
    let session = start_recording(&mut wire, &b).await;
    wire.send_prompt(&b, 1, &session, "accepted", "runs-once")
        .await;
    wire.expect_reply().await;
    let checkpoint = wire.checkpoint(&b, 1).await;

    fs::write(&fault, "").unwrap();
    assert_checkpoint_refused(&mut wire, &b).await;
    assert_refused(&mut wire, &b, &session).await;
    assert_eq!(b.side_effects(), ["runs-once"]);

    let restored = TempDir::new().unwrap();
    let new_binding = b.relocated(restored.path());
    let new_host = Host::start(restored.path(), true).await;
    let mut next = new_host.connect().await;
    next.import(&new_binding, 2, checkpoint).await;
    next.initialize(&new_binding, 2).await;
    next.load(&new_binding, 2, &session).await;
    next.send_prompt(&new_binding, 2, &session, "refused", REFUSED_PROMPT)
        .await;
    next.expect_reply().await;
    assert_eq!(new_binding.side_effects(), [REFUSED_PROMPT]);
}

#[cfg(target_os = "linux")]
async fn fail_barrier_after_new_records(
    wire: &mut Wire,
    b: &Binding,
    fault: &Path,
) -> (String, PathBuf) {
    use nori_harness::transcript::TranscriptLoader;

    let session = start_recording(wire, b).await;
    wire.send_prompt(b, 1, &session, "prefix", "durable-prefix")
        .await;
    wire.expect_reply().await;
    wire.checkpoint(b, 1).await;
    wire.send_prompt(b, 1, &session, "suffix", "unsynced-suffix")
        .await;
    wire.expect_reply().await;
    fs::write(fault, "").unwrap();
    assert_checkpoint_refused(wire, b).await;

    let loader = TranscriptLoader::new(b.roots.join("home/.nori/cli"));
    let recordings = loader
        .find_sessions_for_cwd(&b.roots.join("workspace"))
        .await
        .unwrap();
    let [recording] = recordings.as_slice() else {
        panic!("expected one private fixture transcript");
    };
    let path = loader.session_path(&recording.project_id, &recording.session_id);
    (session, path)
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn recorder_recovers_in_the_same_generation_before_admitting_a_refused_id_once() {
    let home = TempDir::new().unwrap();
    let b = Binding::new(home.path());
    let (host, fault) = fault_injecting_host(&home, "FAIL_SYNC_JSONL").await;
    let mut wire = host.connect().await;
    let (session, path) = fail_barrier_after_new_records(&mut wire, &b, &fault).await;
    let expected = fs::read_to_string(&path).unwrap();
    assert!(expected.contains("durable-prefix") && expected.contains("unsynced-suffix"));
    let retained = wire.replay(&b, 1).await;
    assert!(retained.iter().any(|r| {
        r.pointer("/message/params/update/content/text") == Some(&json!("unsynced-suffix"))
    }));

    assert_refused(&mut wire, &b, &session).await;
    assert_checkpoint_refused(&mut wire, &b).await;
    fs::remove_file(fault).unwrap();
    assert_refused(&mut wire, &b, &session).await;
    wire.checkpoint(&b, 1).await;
    assert_eq!(fs::read_to_string(&path).unwrap(), expected);
    assert_eq!(wire.replay(&b, 1).await, retained);
    for _ in 0..2 {
        wire.send_prompt(&b, 1, &session, "refused", REFUSED_PROMPT)
            .await;
        assert_eq!(wire.expect_reply().await["stopReason"], "end_turn");
    }
    assert_eq!(
        b.side_effects(),
        ["durable-prefix", "unsynced-suffix", REFUSED_PROMPT]
    );
    wire.checkpoint(&b, 1).await;
    let persisted = fs::read_to_string(path).unwrap();
    let appended = persisted.strip_prefix(&expected).unwrap();
    let appended_turns: Vec<_> = appended
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|line| line["type"] == "user")
        .map(|line| line["content"].clone())
        .collect();
    assert_eq!(appended_turns, [json!(REFUSED_PROMPT)]);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn recorder_keeps_admission_closed_if_unsynced_bytes_changed() {
    use std::os::unix::fs::FileExt;

    let home = TempDir::new().unwrap();
    let b = Binding::new(home.path());
    let (host, fault) = fault_injecting_host(&home, "FAIL_SYNC_JSONL").await;
    let mut wire = host.connect().await;
    let (session, path) = fail_barrier_after_new_records(&mut wire, &b, &fault).await;
    let original = fs::read_to_string(&path).unwrap();
    let offset = original.find("unsynced-suffix").unwrap();
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.write_all_at(b"X", offset as u64).unwrap();
    file.sync_data().unwrap();
    fs::remove_file(fault).unwrap();
    for _ in 0..2 {
        assert_checkpoint_refused(&mut wire, &b).await;
        assert_refused(&mut wire, &b, &session).await;
    }
    assert_eq!(b.side_effects(), ["durable-prefix", "unsynced-suffix"]);
}

#[tokio::test]
async fn a_full_journal_refuses_new_prompt_ids_with_a_storage_error() {
    let home = TempDir::new().unwrap();
    let b = Binding::new(home.path());
    let host = Host::start(home.path(), true).await;
    let mut wire = host.connect().await;
    let session = wire.start(&b, 1).await;
    wire.send_prompt(&b, 1, &session, "fill", "fill-journal")
        .await;
    timeout(DEADLINE, async {
        while wire.acp.as_mut().unwrap().recv().await.unwrap()["closed"] != true {}
    })
    .await
    .expect("journal saturation should retire the producer");
    assert_eq!(b.side_effects(), ["fill-journal"]);
    assert_refused(&mut wire, &b, &session).await;
}
