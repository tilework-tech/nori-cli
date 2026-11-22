//! The host as a standard ACP v1 agent: handshake, sessions, turns,
//! permission requests, closing, deleting, sign-in errors, and discovery.

use super::acp_client::Acp;
use super::acp_client::is_update_mentioning;
use super::acp_client::mentions;
use super::*;
use nori_harness::remote_agent::ManagedClient;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

fn mock_agent_binary() -> String {
    std::env::var("MOCK_ACP_AGENT_BIN").unwrap_or_else(|_| {
        std::path::Path::new(env!("CARGO_BIN_EXE_nori"))
            .with_file_name("mock_acp_agent")
            .display()
            .to_string()
    })
}

/// A running host whose default agent is the mock, with `extra` appended to
/// its configuration and `env` passed to its agents.
fn host_with(extra: &str, env: &[(&str, &str)]) -> Host {
    let host = Host::new();
    std::fs::write(
        host.0.path().join("config.toml"),
        format!(
            "agent = \"mock-model\"\napproval_policy = \"on-request\"\n{extra}\n\n[[agents]]\nname = \"Second Mock\"\nslug = \"second-mock\"\n\n[agents.distribution.local]\ncommand = \"{}\"\n",
            mock_agent_binary()
        ),
    )
    .unwrap();
    let mut command = host.command(&["host", "--port", "0"]);
    command.envs(env.iter().copied());
    success(bounded_output(&mut command));
    host
}

/// Connect without initializing, so a test can send its own `initialize`.
fn connect(host: &Host) -> ManagedClient {
    let state: Value = serde_json::from_str(
        &std::fs::read_to_string(host.0.path().join("host/state.json")).unwrap(),
    )
    .unwrap();
    let token = std::fs::read_to_string(host.0.path().join("host/token")).unwrap();
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            ManagedClient::connect(state["endpoint"].as_str().unwrap(), token.trim())
                .await
                .unwrap()
        })
    })
}

fn controller(host: &Host) -> Acp {
    Acp::new(tokio::task::block_in_place(|| host.client()))
}

fn watcher(host: &Host) -> Acp {
    Acp::new(tokio::task::block_in_place(|| host.observer()))
}

async fn new_session(acp: &mut Acp, folder: &std::path::Path) -> (String, Value) {
    let result = acp
        .call(
            "session/new",
            json!({"cwd": canonical(folder), "mcpServers": []}),
        )
        .await
        .unwrap();
    (result["sessionId"].as_str().unwrap().to_owned(), result)
}

fn text(text: &str) -> Value {
    json!([{"type": "text", "text": text}])
}

fn is_permission_request(frame: &Value) -> bool {
    frame["method"] == "session/request_permission" && frame.get("id").is_some()
}

fn listed<'a>(listing: &'a Value, id: &str) -> Option<&'a Value> {
    listing["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["sessionId"] == id)
}

#[tokio::test(flavor = "multi_thread")]
async fn initialize_negotiates_version_one_and_advertises_the_host_contract() {
    let host = host_with("", &[]);
    let mut client = connect(&host);

    let result = client
        .request(
            "initialize",
            json!({"protocolVersion": 99, "clientCapabilities": {}}),
        )
        .await
        .unwrap();

    assert_eq!(result["protocolVersion"], 1);
    assert_eq!(result["agentInfo"]["name"], "nori-host");
    assert_eq!(result["agentInfo"]["version"], env!("CARGO_PKG_VERSION"));
    let capabilities = &result["agentCapabilities"];
    assert_eq!(capabilities["loadSession"], true);
    for capability in ["list", "resume", "close", "delete"] {
        assert!(
            capabilities["sessionCapabilities"][capability].is_object(),
            "{capability} is advertised: {capabilities}"
        );
    }
    assert_eq!(
        capabilities.get("_meta"),
        None,
        "the handshake carries no Nori _meta: {capabilities}"
    );
    assert_eq!(result["authMethods"], json!([]));
    assert_eq!(
        result["capabilities"]["nori_sessions"]["version"], 1,
        "the transition release keeps the old keys: {result}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_session_runs_a_turn_in_its_folder() {
    let host = host_with("", &[("MOCK_AGENT_WRITE_FILE", "marker.txt")]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);

    let (id, created) = new_session(&mut acp, folder.path()).await;
    let finished = acp
        .call(
            "session/prompt",
            json!({"sessionId": id, "prompt": text("write a marker")}),
        )
        .await
        .unwrap();

    assert_eq!(created["modes"]["currentModeId"], "agent");
    assert!(
        created["configOptions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|option| option["id"] == "model"),
        "{created}"
    );
    assert_eq!(finished["stopReason"], "end_turn");
    assert!(
        mentions(&acp.inbox, &id, "agent_message_chunk"),
        "the turn's updates precede its result: {:?}",
        acp.inbox
    );
    assert!(folder.path().join("marker.txt").exists());
    let listing = acp
        .call("session/list", json!({"cwd": canonical(folder.path())}))
        .await
        .unwrap();
    assert!(listed(&listing, &id).is_some(), "{listing}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_clients_mcp_servers_reach_the_agent_it_switches_to() {
    let folder = TempDir::new().unwrap();
    let log = folder.path().join("mcp.log");
    // Only the second agent logs, so the log proves which agent started.
    let host = host_with("", &[]);
    success(host.run(&["host", "stop"]));
    let config = std::fs::read_to_string(host.0.path().join("config.toml")).unwrap();
    std::fs::write(
        host.0.path().join("config.toml"),
        format!(
            "{config}\n[agents.distribution.local.env]\nMOCK_AGENT_MCP_LOG = \"{}\"\n",
            log.display()
        ),
    )
    .unwrap();
    success(host.run(&["host", "--port", "0"]));
    let mut acp = controller(&host);

    let created = acp
        .call(
            "session/new",
            json!({
                "cwd": canonical(folder.path()),
                "mcpServers": [{"name": "probe", "command": "/bin/true", "args": [], "env": []}],
            }),
        )
        .await
        .unwrap();
    let id = created["sessionId"].as_str().unwrap().to_owned();
    let switched = acp
        .call(
            "session/set_config_option",
            json!({"sessionId": id, "configId": "agent", "value": "second-mock"}),
        )
        .await
        .unwrap();

    let agent = switched["configOptions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["id"] == "agent")
        .unwrap();
    assert_eq!(agent["currentValue"], "second-mock");
    let seen = std::fs::read_to_string(&log).unwrap();
    assert!(seen.lines().any(|name| name == "probe"), "{seen}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_session_needs_an_existing_absolute_folder() {
    let host = host_with("", &[]);
    let mut acp = controller(&host);

    let relative = acp
        .call("session/new", json!({"cwd": "relative", "mcpServers": []}))
        .await
        .unwrap_err();
    let missing = acp
        .call(
            "session/new",
            json!({"cwd": "/nonexistent/nori-test-folder", "mcpServers": []}),
        )
        .await
        .unwrap_err();

    assert_eq!(relative.code, -32602, "{relative:?}");
    assert_eq!(missing.code, -32602, "{missing:?}");
    let listing = acp.call("session/list", json!({})).await.unwrap();
    assert_eq!(listing["sessions"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_hosts_default_mode_applies_to_new_sessions() {
    let host = host_with("\n[host]\ndefault_mode = \"approve-all\"\n", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);

    let (_, created) = new_session(&mut acp, folder.path()).await;

    assert_eq!(created["modes"]["currentModeId"], "approve-all");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_prompt_during_a_turn_is_busy_and_cancel_ends_the_turn() {
    let host = host_with("", &[("MOCK_AGENT_STREAM_UNTIL_CANCEL", "1")]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;

    acp.send(json!({"jsonrpc":"2.0","id":"first","method":"session/prompt","params":{"sessionId":id,"prompt":text("mock:stream-until-cancel")}}))
        .await;
    acp.until(is_update_mentioning(&id, "agent_message_chunk"))
        .await;
    let busy = acp
        .call(
            "session/prompt",
            json!({"sessionId": id, "prompt": text("another")}),
        )
        .await
        .unwrap_err();
    acp.send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":id}}))
        .await;
    let ended = acp.until(|frame| frame["id"] == "first").await;

    assert_eq!(busy.code, -32015, "{busy:?}");
    assert_eq!(ended["result"]["stopReason"], "cancelled", "{ended}");
}

#[tokio::test(flavor = "multi_thread")]
async fn permission_requests_go_to_the_controller_only() {
    let host = host_with("", &[("MOCK_AGENT_REQUEST_PERMISSION", "1")]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    let mut watching = watcher(&host);
    watching
        .call(
            "session/resume",
            json!({"sessionId": id, "cwd": canonical(folder.path())}),
        )
        .await
        .unwrap();

    acp.send(json!({"jsonrpc":"2.0","id":"turn","method":"session/prompt","params":{"sessionId":id,"prompt":text("run the command")}}))
        .await;
    let request = acp.until(is_permission_request).await;
    assert_eq!(request["params"]["sessionId"], id);
    acp.send(json!({"jsonrpc":"2.0","id":request["id"],"result":{"outcome":{"outcome":"selected","optionId":"allow"}}}))
        .await;
    let ended = acp.until(|frame| frame["id"] == "turn").await;
    watching
        .until(is_update_mentioning(&id, "Permission granted"))
        .await;

    assert_eq!(ended["result"]["stopReason"], "end_turn", "{ended}");
    assert!(
        !watching.log.iter().any(is_permission_request),
        "watchers are not asked: {:?}",
        watching.log
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pending_permission_request_is_asked_again_after_reconnecting() {
    let host = host_with("", &[("MOCK_AGENT_REQUEST_PERMISSION", "1")]);
    let folder = TempDir::new().unwrap();
    let mut first = controller(&host);
    let (id, _) = new_session(&mut first, folder.path()).await;
    first
        .send(json!({"jsonrpc":"2.0","id":"turn","method":"session/prompt","params":{"sessionId":id,"prompt":text("run the command")}}))
        .await;
    first.until(is_permission_request).await;
    drop(first);

    let mut second = controller(&host);
    second.call("_nori/control/take", json!({})).await.unwrap();
    second
        .call(
            "session/resume",
            json!({"sessionId": id, "cwd": canonical(folder.path())}),
        )
        .await
        .unwrap();
    let request = second.until(is_permission_request).await;
    second
        .send(json!({"jsonrpc":"2.0","id":request["id"],"result":{"outcome":{"outcome":"selected","optionId":"allow"}}}))
        .await;

    assert_eq!(request["params"]["sessionId"], id);
    assert!(request["params"]["toolCall"].is_object(), "{request}");
    second
        .until(is_update_mentioning(&id, "Permission granted"))
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn loading_a_session_does_not_ask_about_permissions_already_answered() {
    let host = host_with("", &[("MOCK_AGENT_REQUEST_PERMISSION", "1")]);
    let folder = TempDir::new().unwrap();
    let mut first = controller(&host);
    let (id, _) = new_session(&mut first, folder.path()).await;
    first
        .send(json!({"jsonrpc":"2.0","id":"turn","method":"session/prompt","params":{"sessionId":id,"prompt":text("run the command")}}))
        .await;
    let request = first.until(is_permission_request).await;
    first
        .send(json!({"jsonrpc":"2.0","id":request["id"],"result":{"outcome":{"outcome":"selected","optionId":"allow"}}}))
        .await;
    first.until(|frame| frame["id"] == "turn").await;
    drop(first);

    let mut second = controller(&host);
    second.call("_nori/control/take", json!({})).await.unwrap();
    second
        .call(
            "session/load",
            json!({"sessionId": id, "cwd": canonical(folder.path()), "mcpServers": []}),
        )
        .await
        .unwrap();
    second.collect_for(Duration::from_millis(500)).await;

    assert!(
        mentions(&second.log, &id, "Permission granted"),
        "the history is replayed: {:?}",
        second.log
    );
    assert!(
        !second.log.iter().any(is_permission_request),
        "an answered request is not asked again: {:?}",
        second.log
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn watchers_cannot_prompt_close_or_delete() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    let mut watching = watcher(&host);

    for (method, params) in [
        (
            "session/prompt",
            json!({"sessionId": id, "prompt": text("hi")}),
        ),
        ("session/close", json!({"sessionId": id})),
        ("session/delete", json!({"sessionId": id})),
    ] {
        let refused = watching.call(method, params).await.unwrap_err();
        assert_eq!(refused.code, -32016, "{method}: {refused:?}");
    }
    let frames = acp.collect_for(Duration::from_millis(300)).await;
    assert!(!mentions(&frames, &id, "\"hi\""), "{frames:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_closed_session_stays_listed_and_loads_again() {
    let host = host_with("", &[("MOCK_AGENT_WRITE_FILE", "marker.txt")]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    acp.call(
        "session/prompt",
        json!({"sessionId": id, "prompt": text("write a marker")}),
    )
    .await
    .unwrap();
    std::fs::remove_file(folder.path().join("marker.txt")).unwrap();

    acp.call("session/close", json!({"sessionId": id}))
        .await
        .unwrap();
    let listing = acp.call("session/list", json!({})).await.unwrap();
    assert!(listed(&listing, &id).is_some(), "{listing}");
    acp.inbox.clear();
    acp.call(
        "session/load",
        json!({"sessionId": id, "cwd": canonical(folder.path()), "mcpServers": []}),
    )
    .await
    .unwrap();
    assert!(
        mentions(&acp.inbox, &id, "write a marker"),
        "{:?}",
        acp.inbox
    );
    let finished = acp
        .call(
            "session/prompt",
            json!({"sessionId": id, "prompt": text("write again")}),
        )
        .await
        .unwrap();

    assert_eq!(finished["stopReason"], "end_turn");
    assert!(folder.path().join("marker.txt").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deleted_session_is_gone_from_the_list() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    acp.call(
        "session/prompt",
        json!({"sessionId": id, "prompt": text("remember me")}),
    )
    .await
    .unwrap();

    acp.call("session/delete", json!({"sessionId": id}))
        .await
        .unwrap();

    let listing = acp.call("session/list", json!({})).await.unwrap();
    assert!(listed(&listing, &id).is_none(), "{listing}");
    let gone = acp
        .call(
            "session/load",
            json!({"sessionId": id, "cwd": canonical(folder.path()), "mcpServers": []}),
        )
        .await
        .unwrap_err();
    assert_eq!(gone.code, -32002, "{gone:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_that_needs_sign_in_fails_with_auth_required() {
    let host = host_with("", &[("MOCK_AGENT_REQUIRE_AUTH", "1")]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);

    let refused = acp
        .call(
            "session/new",
            json!({"cwd": canonical(folder.path()), "mcpServers": []}),
        )
        .await
        .unwrap_err();

    assert_eq!(refused.code, -32000, "{refused:?}");
    assert!(
        refused.message.contains("Authentication required"),
        "{refused:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_prompt_waiting_on_an_agent_that_needs_sign_in_reports_why_it_failed() {
    let host = host_with(
        "",
        &[
            ("MOCK_AGENT_REQUIRE_AUTH", "1"),
            ("MOCK_AGENT_STARTUP_DELAY_MS", "500"),
        ],
    );
    let folder = TempDir::new().unwrap();
    let mut client = tokio::task::block_in_place(|| host.client());
    let started = client
        .request(
            "_nori/session/new",
            json!({"cwd": canonical(folder.path())}),
        )
        .await
        .unwrap();
    let id = started["session_id"].as_str().unwrap();

    let refused = client
        .request(
            "_nori/session/prompt",
            json!({"session_id": id, "prompt": text("hello")}),
        )
        .await
        .unwrap_err()
        .to_string();

    assert!(refused.starts_with("host error -32000:"), "{refused}");
    assert!(refused.contains("Authentication required"), "{refused}");
}

#[test]
fn the_host_publishes_host_json_beside_the_legacy_discovery_files() {
    use std::os::unix::fs::PermissionsExt;
    let host = host_with("", &[]);
    let folder = host.0.path().join("host");

    let published: Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join("host.json")).unwrap()).unwrap();

    let state: Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join("state.json")).unwrap()).unwrap();
    let token = std::fs::read_to_string(folder.join("token")).unwrap();
    assert_eq!(published["endpoint"], state["endpoint"]);
    assert_eq!(published["pid"], state["pid"]);
    assert_eq!(published["token"], token.trim());
    assert_eq!(published["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(published["contract"], json!({"acp": 1, "nori": 1}));
    assert_eq!(published["startedBy"], "nori-cli");
    assert!(
        uuid::Uuid::parse_str(published["instanceId"].as_str().unwrap()).is_ok(),
        "{published}"
    );
    let mode = std::fs::metadata(folder.join("host.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[tokio::test(flavor = "multi_thread")]
async fn list_rows_say_which_agent_runs_a_session() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let created = acp
        .call(
            "session/new",
            json!({
                "cwd": canonical(folder.path()),
                "mcpServers": [],
                "_meta": {"nori/agent": "second-mock"},
            }),
        )
        .await
        .unwrap();
    let live = created["sessionId"].as_str().unwrap().to_owned();
    let (closed, _) = new_session(&mut acp, folder.path()).await;
    acp.call("session/close", json!({"sessionId": closed}))
        .await
        .unwrap();

    let listing = acp.call("session/list", json!({})).await.unwrap();

    assert_eq!(
        listed(&listing, &live).unwrap()["_meta"],
        json!({"nori/agent": "second-mock"})
    );
    assert_eq!(
        listed(&listing, &closed).unwrap()["_meta"],
        json!({"nori/agent": "mock-model"})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_session_refuses_an_unknown_agent() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);

    let refused = acp
        .call(
            "session/new",
            json!({
                "cwd": canonical(folder.path()),
                "mcpServers": [],
                "_meta": {"nori/agent": "no-such-agent"},
            }),
        )
        .await
        .unwrap_err();

    assert_eq!(refused.code, -32602, "{refused:?}");
}
