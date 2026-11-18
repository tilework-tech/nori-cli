use super::*;
use nori_harness::remote_agent::ManagedClient;
use serde_json::Value;
use serde_json::json;
use std::fs;
use std::path::Path;

const ENSURE: &str = "_nori/session/ensure";
const STATUS: &str = "_nori/session/status";
const CAP: &str = "/agentCapabilities/_meta/nori.dev~1managedHost";

fn executable(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn start(limit: i32, on: bool) -> Host {
    let host = Host::new();
    fs::write(host.0.path().join(".env"), "HOST_DOTENV=host-only\n").unwrap();
    fs::create_dir(host.0.path().join("hosted-sessions")).unwrap();
    let helper = host.0.path().join("ambient-helper.sh");
    executable(
        &helper,
        &format!(
            "#!/bin/sh\necho invoked >> '{}'\n",
            host.0.path().join("ambient-invoked").display()
        ),
    );
    fs::write(host.0.path().join("config.toml"), format!(
        "agent = 'mock-model'\n[host]\nmanaged = {on}\nmax_sessions = {limit}\n[hooks]\nsession_start = ['{helper}']\n[mcp_servers.fixture]\ncommand = '{helper}'\n", helper=helper.display()
    )).unwrap();
    executable(
        &host.0.path().join("fixture"),
        r#"#!/bin/sh
echo started >> starts
/bin/sh -c 'printf "%s\n" "$PWD" "$HOME" "$CODEX_HOME" "$NORI_HOME" "$LABEL" "${HOST_DOTENV-unset}" "${HOST_EXPORTED-unset}"' > observed.tmp
/bin/mv observed.tmp observed
while IFS= read -r request; do
    printf '%s\n' "$request" >> input
    printf '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{}}}\n'
done
"#,
    );
    success(bounded_output(
        host.command(&["host", "--port", "0"])
            .env("HOST_EXPORTED", "host-only")
            .env("CODEX_HOME", host.0.path()),
    ));
    host
}

fn request(host: &Host, binding: &str) -> Value {
    let dir = host.0.path().join("hosted-sessions").join(binding);
    let cwd = dir.join("workspace");
    let home = dir.join("home");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&home).unwrap();
    json!({
        "commandId":format!("launch-{binding}"),
        "conversationId":uuid::Uuid::new_v4(),
        "bindingId":binding,"generation":1,
        "spec":{
            "projectId":"fixture-project","runId":format!("run-{binding}"),
            "agent":"fixture","model":"fixture-model",
            "cwd":cwd,"home":home,"env":{"LABEL":binding},
            "adapter":{"path":host.0.path().join("fixture"),"version":"1.2.3","args":[]},
            "additions":{"promptEcho":false,"statusEvents":false,"recording":false,"clientMcp":false}
        }
    })
}

fn client(host: &Host) -> ManagedClient {
    tokio::task::block_in_place(|| host.client())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_ensure_uses_one_slot_and_preserves_the_pinned_process() {
    let host = start(1, true);
    let mut a = client(&host);
    let mut b = client(&host);
    let init = a.request("initialize", json!({})).await.unwrap();
    assert_eq!(init.pointer(CAP).unwrap()["version"], 1);
    let launch = request(&host, "alpha");
    let (first, retry) = tokio::join!(
        a.request(ENSURE, launch.clone()),
        b.request(ENSURE, launch.clone())
    );
    let first = first.unwrap();
    assert_eq!(retry.unwrap(), first);
    assert_eq!(first["conversationId"], launch["conversationId"]);
    assert_eq!(first["state"], "spawned");
    let cwd = Path::new(launch["spec"]["cwd"].as_str().unwrap());
    wait_for_file(&cwd.join("observed"));
    assert_eq!(fs::read_to_string(cwd.join("starts")).unwrap(), "started\n");
    assert!(!cwd.join("input").exists());
    let mut conflict = launch.clone();
    conflict["spec"]["adapter"]["version"] = json!("2");
    assert!(
        a.request(ENSURE, conflict)
            .await
            .unwrap_err()
            .to_string()
            .contains("conflict")
    );
    let overflow = request(&host, "beta");
    assert!(
        a.request(ENSURE, overflow)
            .await
            .unwrap_err()
            .to_string()
            .contains("capacity")
    );
    assert!(a.request("_nori/session/new", json!({})).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn descendants_see_only_their_binding_environment_and_roots() {
    let host = start(2, true);
    let mut a = client(&host);
    let mut first_id = None;
    for binding in ["alpha", "beta"] {
        let launch = request(&host, binding);
        if let Some(id) = first_id.take() {
            let mut collision = launch.clone();
            collision["conversationId"] = id;
            assert!(
                a.request(ENSURE, collision)
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("conflict")
            );
        }
        a.request(ENSURE, launch.clone()).await.unwrap();
        first_id = Some(launch["conversationId"].clone());
        let mut collision = launch.clone();
        collision["conversationId"] = json!(uuid::Uuid::new_v4());
        assert!(
            a.request(ENSURE, collision)
                .await
                .unwrap_err()
                .to_string()
                .contains("conflict")
        );
        let home = canonical(Path::new(launch["spec"]["home"].as_str().unwrap()));
        let cwd = canonical(Path::new(launch["spec"]["cwd"].as_str().unwrap()));
        let observed = Path::new(&cwd).join("observed");
        wait_for_file(&observed);
        assert_eq!(
            fs::read_to_string(observed).unwrap(),
            format!("{cwd}\n{home}\n{home}/.codex\n{home}/.nori/cli\n{binding}\nunset\nunset\n")
        );
    }
    assert!(!host.0.path().join("ambient-invoked").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn managed_roots_and_pins_fail_closed() {
    let host = start(1, true);
    let mut client = client(&host);
    let launch = request(&host, "alpha");
    let outside = TempDir::new().unwrap();
    let root = host.0.path().join("hosted-sessions/alpha");
    let link = root.join("escape");
    std::os::unix::fs::symlink(outside.path(), &link).unwrap();
    std::os::unix::fs::symlink(&root, host.0.path().join("hosted-sessions/alias")).unwrap();
    for (pointer, value) in [
        ("/bindingId", json!("alias")),
        ("/spec/cwd", json!("relative")),
        ("/spec/cwd", json!(link)),
        ("/spec/home", json!(outside.path())),
        ("/spec/home", json!(root.join("workspace"))),
        ("/bindingId", json!("../host")),
        ("/spec/adapter/path", json!("fixture")),
        ("/spec/env", json!({"HOME":"/wrong"})),
        ("/generation", json!(0)),
    ] {
        let mut invalid = launch.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        assert!(client.request(ENSURE, invalid).await.is_err(), "{pointer}");
    }
    assert!(!root.join("workspace/starts").exists());
    client.request(ENSURE, launch).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exited_and_never_spawned_remain_distinct_on_retry() {
    let host = start(1, true);
    let mut client = client(&host);
    let mut failed = request(&host, "failed");
    let broken = host.0.path().join("broken");
    executable(&broken, "#!/missing-fixture-interpreter\n");
    failed["spec"]["adapter"]["path"] = json!(broken);
    let scope = |r: &Value| json!({"bindingId":r["bindingId"], "generation":r["generation"]});
    assert!(client.request(ENSURE, failed.clone()).await.is_err());
    assert_eq!(
        client.request(STATUS, scope(&failed)).await.unwrap()["state"],
        "spawnFailed"
    );
    executable(&broken, "#!/bin/sh\nexit 0\n");
    assert!(client.request(ENSURE, failed.clone()).await.is_err());
    let mut conflict = failed;
    conflict["spec"]["model"] = json!("changed");
    assert!(
        client
            .request(ENSURE, conflict)
            .await
            .unwrap_err()
            .to_string()
            .contains("conflict")
    );

    let mut exits = request(&host, "exits");
    let binary = host.0.path().join("exits");
    executable(&binary, "#!/bin/sh\necho started >> starts\nexit 17\n");
    exits["spec"]["adapter"]["path"] = json!(binary);
    let ack = client.request(ENSURE, exits.clone()).await.unwrap();
    assert_eq!(ack["state"], "spawned");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let status = client.request(STATUS, scope(&exits)).await.unwrap();
        if status["state"] == "exited" {
            assert_eq!(status["exitCode"], 17);
            break;
        }
        assert!(Instant::now() < deadline, "{status}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(client.request(ENSURE, exits.clone()).await.unwrap(), ack);
    assert!(
        client
            .request(STATUS, json!({"bindingId":"exits","generation":2}))
            .await
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(Path::new(exits["spec"]["cwd"].as_str().unwrap()).join("starts"))
            .unwrap(),
        "started\n"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_off_keeps_local_session_creation_and_omits_managed_discovery() {
    let host = start(1, false);
    let mut client = client(&host);
    assert!(
        client
            .request("initialize", json!({}))
            .await
            .unwrap()
            .pointer(CAP)
            .is_none()
    );
    assert!(client.request(ENSURE, request(&host, "off")).await.is_err());
    assert!(
        client
            .request("_nori/session/new", json!({}))
            .await
            .unwrap()["session_id"]
            .is_string()
    );
}
