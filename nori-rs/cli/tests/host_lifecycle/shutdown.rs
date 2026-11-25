//! Exercise orderly exit and a hard crash through the public host commands.
use super::acp_v1::controller;
use super::acp_v1::new_session;
use super::acp_v1::text;
use super::*;
use serde_json::json;
use std::fs;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/standard_acp");

// Both adapters must see EOF before either finishes. One then stalls until
// the host escalates. No timing inference is needed to detect serial shutdown.
const COOPERATIVE_THEN_STALLED: &str = r#"
python3 "$1" graceful
touch eof
while [ ! -e "$(cat peer)/eof" ]; do sleep 0.02; done
touch flushed
if [ -e stall ]; then exec sleep 30; fi
"#;

fn home_with_adapter(command: &str, args: &[&str]) -> Host {
    let host = Host::new();
    let config = format!(
        "agent = 'lifecycle'\napproval_policy = 'on-request'\n[[agents]]\nname = 'Lifecycle'\nslug = 'lifecycle'\n[agents.distribution.local]\ncommand = {command:?}\nargs = {}\n",
        serde_json::to_string(args).unwrap()
    );
    fs::write(host.0.path().join("config.toml"), config).unwrap();
    host
}

fn expect_locked(lease: &fs::File) {
    assert!(matches!(
        lease.try_lock(),
        Err(fs::TryLockError::WouldBlock)
    ));
}

async fn expect_released(lease: &fs::File) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match lease.try_lock() {
                Ok(()) => return,
                Err(fs::TryLockError::WouldBlock) => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(error) => panic!("could not inspect process lease: {error}"),
            }
        }
    })
    .await
    .expect("a process still holds its lease after host termination");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_gives_adapters_eof_concurrently_and_sweeps_their_descendants() {
    let fixture = format!("{FIXTURES}/fixture.py");
    let host = home_with_adapter(
        "sh",
        &["-c", COOPERATIVE_THEN_STALLED, "shutdown", &fixture],
    );
    let fast = TempDir::new().unwrap();
    let stalled = TempDir::new().unwrap();
    fs::write(fast.path().join("peer"), canonical(stalled.path())).unwrap();
    fs::write(stalled.path().join("peer"), canonical(fast.path())).unwrap();
    fs::write(stalled.path().join("stall"), "").unwrap();
    success(host.run(&["host", "--port", "0"]));
    let mut client = controller(&host);
    new_session(&mut client, fast.path()).await;
    let (id, _) = new_session(&mut client, stalled.path()).await;
    fs::File::create(stalled.path().join("hold-lease-work")).unwrap();
    let descendant = fs::File::create(stalled.path().join("hold-lease-work-client")).unwrap();
    client
        .call(
            "session/prompt",
            json!({"sessionId":id, "prompt":text("hold-lease-work")}),
        )
        .await
        .unwrap();
    expect_locked(&descendant);

    let start = Instant::now();
    success(host.run(&["host", "stop"]));
    assert!(
        start.elapsed() < Duration::from_secs(15),
        "waited for the stalled adapter"
    );
    for folder in [&fast, &stalled] {
        assert!(
            folder.path().join("flushed").exists(),
            "adapter lost its EOF cleanup in {:?}",
            folder.path()
        );
    }
    expect_released(&descendant).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hard_host_death_disconnects_an_active_session_and_allows_a_fresh_instance() {
    let fixture = format!("{FIXTURES}/fork_fixture.py");
    let host = home_with_adapter("python3", &[&fixture, "shutdown"]);
    let folder = TempDir::new().unwrap();
    let mut process =
        tokio::process::Command::from(host.command(&["host", "--foreground", "--port", "0"]))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
    wait_for_file(&host.0.path().join("host/state.json"));
    let first = discovery(&host);
    let mut client = controller(&host);
    let (id, _) = new_session(&mut client, folder.path()).await;
    client
        .send(json!({"jsonrpc":"2.0","id":71,"method":"session/prompt",
        "params":{"sessionId":id,"prompt":text("stream-close")}}))
        .await;
    client
        .until(super::acp_client::is_update_mentioning(&id, "first"))
        .await;
    let adapter = fs::File::open(folder.path().join("adapter.lock")).unwrap();
    expect_locked(&adapter);
    let mut observer = tokio::task::block_in_place(|| host.observer());

    process.kill().await.unwrap();
    let disconnected = tokio::time::timeout(
        Duration::from_secs(3),
        observer.request("session/list", json!({})),
    )
    .await
    .expect("a connected client hung after the host died");
    assert!(disconnected.is_err());
    expect_released(&adapter).await;
    success(host.run(&["host", "--port", "0"]));
    assert_ne!(first["instanceId"], discovery(&host)["instanceId"]);
    let mut replacement = controller(&host);
    let running = replacement
        .call("_nori/session/list", json!({}))
        .await
        .unwrap();
    assert_eq!(
        running["sessions"],
        json!([]),
        "a hard crash resumed live work"
    );
    let fresh = TempDir::new().unwrap();
    let (new_id, _) = new_session(&mut replacement, fresh.path()).await;
    let completed = replacement
        .call(
            "session/prompt",
            json!({"sessionId":new_id,"prompt":text("hello")}),
        )
        .await
        .unwrap();
    assert_eq!(completed["stopReason"], "end_turn");
}
