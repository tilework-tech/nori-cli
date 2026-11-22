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
    let mut repeated = launch.clone();
    repeated["spec"]["envRemove"] = json!([]);
    assert_eq!(a.request(ENSURE, repeated.clone()).await.unwrap(), first);
    repeated["spec"]["envRemove"] = json!(["SAMPLE_REMOVED"]);
    assert!(
        a.request(ENSURE, repeated)
            .await
            .unwrap_err()
            .to_string()
            .contains("conflict")
    );
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
    assert!(a.request("_nori/session/new", json!({})).await.is_err());
}

async fn refusal_code(client: &mut ManagedClient, params: Value) -> i64 {
    client
        .send(json!({"jsonrpc":"2.0","id":"refusal","method":ENSURE,"params":params}))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let reply = client.recv().await.unwrap();
            if reply["id"] == "refusal" {
                return reply["error"]["code"].as_i64().unwrap();
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prepared_runtimes_and_public_labels_follow_their_binding() {
    let host = start(2, true);
    let mut client = client(&host);
    let mut expected = Vec::new();
    for name in ["alpha", "beta"] {
        let mut launch = request(&host, name);
        let spec = &mut launch["spec"];
        let runtime = Path::new(spec["home"].as_str().unwrap()).join("prepared");
        let cwd = Path::new(spec["cwd"].as_str().unwrap()).to_owned();
        fs::create_dir(&runtime).unwrap();
        fs::write(runtime.join("marker"), name).unwrap();
        let label = format!("sessions:token-connection:{name}:v1");
        spec["runtimeDir"] = json!(runtime);
        spec["credentialRefs"] = json!([label, "opaque-legacy-reference"]);
        spec["env"]["API_KEY"] = json!("synthetic-secret");
        spec["adapter"]["path"] = json!("/bin/sh");
        spec["adapter"]["args"] = json!([
            "-c",
            r#"set -eu; test "$API_KEY" = synthetic-secret; /bin/cat "$NORI_AGENT_RUNTIME_DIR/marker" > result.tmp; /bin/mv result.tmp result"#
        ]);
        let receipt = client.request(ENSURE, launch.clone()).await.unwrap();
        assert_eq!(client.request(ENSURE, launch).await.unwrap(), receipt);
        wait_for_file(&cwd.join("result"));
        assert_eq!(fs::read_to_string(cwd.join("result")).unwrap(), name);
        expected.push(json!({"session_id": receipt["conversationId"], "credential_refs": [label]}));
    }
    let listing: Value =
        serde_json::from_str(&success(host.run(&["host", "ls", "--json"]))).unwrap();
    assert_eq!(listing, json!({"sessions": expected}));
    let wire = client
        .request("_nori/session/list", json!({}))
        .await
        .unwrap()
        .to_string();
    for private in [
        "synthetic-secret",
        "opaque-legacy-reference",
        "API_KEY",
        "marker",
    ] {
        assert!(!wire.contains(private));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removing_an_environment_key_in_one_binding_preserves_its_siblings_value() {
    let host = start(2, true);
    let mut control = client(&host);
    let state: Value =
        serde_json::from_slice(&fs::read(host.0.path().join("host/state.json")).unwrap()).unwrap();
    let endpoint = state["endpoint"]
        .as_str()
        .unwrap()
        .replace("/sessions", "/managed");
    let token = fs::read_to_string(host.0.path().join("host/token")).unwrap();
    let mut machine = ManagedClient::connect(&endpoint, token.trim())
        .await
        .unwrap();
    for (binding, set, removed) in [
        ("parent", "PARENT_MARK", "SIBLING_MARK"),
        ("sibling", "SIBLING_MARK", "PARENT_MARK"),
    ] {
        let mut launch = request(&host, binding);
        launch["spec"]["env"][set] = json!(binding);
        launch["spec"]["envRemove"] = json!([removed]);
        // Capture once at startup, and again when the host delivers ACP initialize.
        let observer = r#"observe() { printf '%s %s' "${PARENT_MARK-unset}" "${SIBLING_MARK-unset}"; }
observe > startup.tmp && /bin/mv startup.tmp startup
IFS= read -r request || exit 1
printf '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{},"_meta":{"observed":"%s"}}}\n' "$(observe)"
IFS= read -r request
"#;
        launch["spec"]["adapter"]["path"] = json!("/bin/sh");
        launch["spec"]["adapter"]["args"] = json!(["-c", observer]);
        control.request(ENSURE, launch.clone()).await.unwrap();
        let observed = Path::new(launch["spec"]["cwd"].as_str().unwrap()).join("startup");
        wait_for_file(&observed);
        assert_eq!(
            fs::read_to_string(observed).unwrap(),
            if binding == "parent" {
                "parent unset"
            } else {
                "unset sibling"
            }
        );
    }
    for (binding, expected) in [("sibling", "unset sibling"), ("parent", "parent unset")] {
        machine
            .send(json!({"bindingId":binding,"generation":1,"message":{
                "jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}
            }}))
            .await
            .unwrap();
        let frame = tokio::time::timeout(Duration::from_secs(5), machine.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(frame["bindingId"], binding);
        assert_eq!(frame["message"]["result"]["_meta"]["observed"], expected);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_runtime_or_token_label_refuses_the_launch_before_spawn() {
    let host = start(1, true);
    let mut client = client(&host);
    let mut launch = request(&host, "alpha");
    let home = Path::new(launch["spec"]["home"].as_str().unwrap()).to_owned();
    let foreign = request(&host, "beta");
    let other = Path::new(foreign["spec"]["home"].as_str().unwrap());
    fs::write(home.join("file"), "fixture").unwrap();
    std::os::unix::fs::symlink(other, home.join("redirect")).unwrap();
    for dir in [
        json!("relative"),
        json!(home.join("missing")),
        json!(home.join("file")),
        json!(other),
        json!(home.join("redirect")),
    ] {
        launch["spec"]["runtimeDir"] = dir;
        assert_eq!(refusal_code(&mut client, launch.clone()).await, -32602);
    }
    launch["spec"].as_object_mut().unwrap().remove("runtimeDir");
    for label in [
        "sessions:token-connection::v1",
        "sessions:token-connection:../beta:v1",
        "sessions:token-connection:alpha:v2",
    ] {
        launch["spec"]["credentialRefs"] = json!([label]);
        assert_eq!(refusal_code(&mut client, launch.clone()).await, -32602);
    }
    launch["spec"]["credentialRefs"] = json!(["sessions:codex-subscription-file:v1"]);
    let mut spoofed = launch.clone();
    spoofed["spec"]["env"]["nori_agent_runtime_dir"] = json!(other);
    assert_eq!(refusal_code(&mut client, spoofed).await, -32602);
    client.request(ENSURE, launch.clone()).await.unwrap();
    let listing: Value =
        serde_json::from_str(&success(host.run(&["host", "ls", "--json"]))).unwrap();
    assert_eq!(
        listing,
        json!({"sessions": [{"session_id": launch["conversationId"],
        "credential_refs": ["sessions:codex-subscription-file:v1"]}]})
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn managed_wrapper_requires_its_own_pin_and_is_part_of_the_binding_identity() {
    let host = start(1, true);
    let mut control = client(&host);
    let mut launch = request(&host, "wrapped");
    let home = Path::new(launch["spec"]["home"].as_str().unwrap());
    let runtime = home.join("prepared");
    let sibling = home.join("prepared-other");
    fs::create_dir_all(&runtime).unwrap();
    fs::create_dir_all(&sibling).unwrap();
    let wrapper = runtime.join("entry");
    fs::copy("/bin/sh", &wrapper).unwrap();
    let outside = sibling.join("entry");
    fs::copy("/bin/sh", &outside).unwrap();
    std::os::unix::fs::symlink(&outside, runtime.join("redirect")).unwrap();
    std::os::unix::fs::symlink(&sibling, runtime.join("redirect-dir")).unwrap();

    let pinned =
        json!({"path":wrapper, "version":"1", "args":["-c", "exec \"$@\"", "fixture wrapper"]});
    launch["spec"]["adapter"]["wrapper"] = pinned.clone();
    assert_eq!(refusal_code(&mut control, launch.clone()).await, -32602);

    launch["spec"]["runtimeDir"] = json!(runtime);
    for invalid in [
        json!({"path":"relative", "version":"1"}),
        json!({"path":runtime.join("missing"), "version":"1"}),
        json!({"path":outside, "version":"1"}),
        json!({"path":runtime.join("../prepared-other/entry"), "version":"1"}),
        json!({"path":runtime.join("redirect"), "version":"1"}),
        json!({"path":runtime.join("redirect-dir/entry"), "version":"1"}),
        json!({"path":wrapper}),
        json!({"path":wrapper, "version":"  "}),
        json!({"path":wrapper, "version":"1", "args":["bad\0arg"]}),
        json!({"path":wrapper, "version":"1", "wrapper":{"path":wrapper,"version":"1"}}),
    ] {
        launch["spec"]["adapter"]["wrapper"] = invalid;
        assert_eq!(refusal_code(&mut control, launch.clone()).await, -32602);
    }
    std::os::unix::fs::symlink("entry", runtime.join("current")).unwrap();
    launch["spec"]["adapter"]["wrapper"] = pinned;
    launch["spec"]["adapter"]["wrapper"]["path"] = json!(runtime.join("current"));
    let receipt = control.request(ENSURE, launch.clone()).await.unwrap();
    assert_eq!(
        control.request(ENSURE, launch.clone()).await.unwrap(),
        receipt
    );
    launch["spec"]["adapter"]["wrapper"]["version"] = json!("2");
    assert!(
        control
            .request(ENSURE, launch)
            .await
            .unwrap_err()
            .to_string()
            .contains("conflict")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_host_refuses_new_binding_with_capacity_code() {
    let host = start(1, true);
    let mut client = client(&host);
    client
        .request(ENSURE, request(&host, "alpha"))
        .await
        .unwrap();
    assert_eq!(
        refusal_code(&mut client, request(&host, "beta")).await,
        -32017
    );
    assert!(
        client
            .request(STATUS, json!({"bindingId":"beta","generation":1}))
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_ensure_returns_invalid_params_even_on_a_full_host() {
    let host = start(1, true);
    let mut client = client(&host);
    client
        .request(ENSURE, request(&host, "alpha"))
        .await
        .unwrap();
    let mut invalid = request(&host, "beta");
    invalid["spec"]["envRemove"] = json!([]);
    for (path, value) in [
        ("/spec/cwd", json!("relative")),
        ("/spec/cwd", Value::Null),
        ("/spec/adapter/args", json!(["arg\0value"])),
        ("/spec/env", json!({"LABEL":"env\0value"})),
        ("/spec/envRemove", json!([""])),
        ("/spec/envRemove", json!(["bad=name"])),
        ("/spec/envRemove", json!(["bad\0name"])),
        ("/spec/envRemove", json!(["home"])),
        ("/spec/envRemove", json!(["LABEL"])),
    ] {
        let mut invalid = invalid.clone();
        *invalid.pointer_mut(path).unwrap() = value;
        assert_eq!(refusal_code(&mut client, invalid).await, -32602, "{path}");
    }
    assert!(
        client
            .request(STATUS, json!({"bindingId":"beta","generation":1}))
            .await
            .is_err()
    );
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
        assert_eq!(
            refusal_code(&mut client, invalid).await,
            -32602,
            "{pointer}"
        );
    }
    for component in [".codex", ".nori", ".nori/cli"] {
        let path = root.join("home").join(component);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(outside.path().join("dangling"), &path).unwrap();
        assert_eq!(
            refusal_code(&mut client, launch.clone()).await,
            -32602,
            "{component}"
        );
        fs::remove_file(path).unwrap();
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dedicated_machine_matches_only_its_existing_roots_and_binding() {
    let host = start(1, true);
    success(host.run(&["host", "stop"]));
    let home = host.0.path().join("existing-home");
    let cwd = home.join("org/workspace");
    fs::create_dir_all(&cwd).unwrap();
    configure_dedicated(&host, "single", &home, &cwd);
    success(host.run(&["host", "--port", "0"]));
    let mut client = client(&host);
    let mut launch = request(&host, "single");
    assert!(client.request(ENSURE, launch.clone()).await.is_err());
    launch["spec"]["home"] = json!(home);
    launch["spec"]["cwd"] = json!(cwd);
    let mut foreign = launch.clone();
    foreign["bindingId"] = json!("other");
    assert!(client.request(ENSURE, foreign).await.is_err());
    client.request(ENSURE, launch).await.unwrap();
    wait_for_file(&cwd.join("observed"));
}

fn configure_dedicated(host: &Host, binding: &str, home: &Path, cwd: &Path) {
    fs::write(host.0.path().join("config.toml"), format!(
        "[host]\nmanaged=true\nmax_sessions=1\n[host.dedicated_roots]\nbinding_id='{binding}'\nhome='{}'\ncwd='{}'\n",
        home.display(),cwd.display())).unwrap();
}

#[test]
fn unusable_dedicated_layout_fails_before_discovery() {
    let host = Host::new();
    let home = host.0.path().join("home");
    let work = home.join("workspace");
    fs::create_dir_all(&work).unwrap();
    let alias = host.0.path().join("alias");
    std::os::unix::fs::symlink(&home, &alias).unwrap();
    for (binding, cwd) in [("single", &alias), ("bad/id", &work)] {
        configure_dedicated(&host, binding, &home, cwd);
        failure(host.run(&["host", "--port", "0"]));
        assert!(!host.0.path().join("host/state.json").exists());
    }
    fs::write(home.join(".codex"), "file").unwrap();
    configure_dedicated(&host, "single", &home, &work);
    failure(host.run(&["host", "--port", "0"]));
    assert!(!host.0.path().join("host/state.json").exists());
}
