use super::*;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

async fn assert_refused(wire: &mut Wire, binding: &Binding, session: &str) {
    let executed = binding.side_effects();
    wire.send_prompt(binding, 1, session, "refused", "must-not-run")
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
#[tokio::test]
async fn failed_recorder_barrier_refuses_the_next_prompt_before_admission() {
    let home = TempDir::new().unwrap();
    let b = Binding::new(home.path());
    let (host, fault) = fault_injecting_host(&home, "FAIL_SYNC_JSONL").await;
    let mut wire = host.connect().await;
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
    wire.initialize(&b, 1).await;
    let session = wire.new_session(&b, 1).await;
    wire.send_prompt(&b, 1, &session, "accepted", "runs-once")
        .await;
    wire.expect_reply().await;
    let checkpoint = wire.checkpoint(&b, 1).await;

    fs::write(&fault, "").unwrap();
    let failed = wire
        .client
        .request(
            "_nori/host/checkpoint",
            json!({"bindingId":b.id,"generation":1}),
        )
        .await
        .unwrap_err()
        .to_string();
    assert_refused(&mut wire, &b, &session).await;
    assert!(failed.contains("-32020"), "{failed}");
    assert_eq!(b.side_effects(), ["runs-once"]);

    let restored = TempDir::new().unwrap();
    let new_binding = b.relocated(restored.path());
    let new_host = Host::start(restored.path(), true).await;
    let mut next = new_host.connect().await;
    next.import(&new_binding, 2, checkpoint).await;
    next.initialize(&new_binding, 2).await;
    next.load(&new_binding, 2, &session).await;
    next.send_prompt(&new_binding, 2, &session, "refused", "now-healthy")
        .await;
    next.expect_reply().await;
    assert_eq!(new_binding.side_effects(), ["now-healthy"]);
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
