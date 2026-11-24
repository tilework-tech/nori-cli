use std::time::Duration;

use nori_config::NoriConfig;
use nori_config::NoriConfigOverrides;
use nori_protocol::SessionStatus;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

use super::launch;

/// A home whose default agent is the mock, which the registry finds beside
/// the test binary.
fn config(home: &TempDir) -> (NoriConfig, NoriConfigOverrides) {
    let path = home.path().join("config.toml");
    std::fs::write(
        &path,
        "agent = \"mock-model\"\napproval_policy = \"on-request\"\n",
    )
    .unwrap();
    let overrides = NoriConfigOverrides {
        cwd: Some(home.path().to_owned()),
        ..Default::default()
    };
    (
        NoriConfig::load_from_path_with_overrides(&path, overrides.clone()).unwrap(),
        overrides,
    )
}

async fn call(host: &mut super::OwnedHost, id: i64, method: &str, params: Value) -> Value {
    host.to_host
        .send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let line = host.from_host.recv().await.expect("host stopped answering");
            let message: Value = serde_json::from_str(&line).unwrap();
            if message["id"] == id && message.get("method").is_none() {
                break message;
            }
        }
    })
    .await
    .expect("host reply")
}

#[tokio::test]
async fn the_pipe_client_and_the_tui_share_one_host() {
    let home = TempDir::new().unwrap();
    let (config, overrides) = config(&home);
    let mut host = launch(&config, &overrides).unwrap();

    let initialized = call(&mut host, 1, "initialize", json!({"protocolVersion":1})).await;
    assert_eq!(initialized["result"]["control"]["role"], "controller");
    let created = call(
        &mut host,
        2,
        "session/new",
        json!({"cwd": home.path(), "mcpServers": [], "_meta":{"nori/agent":"mock-model"}}),
    )
    .await;
    let session_id = created["result"]["sessionId"]
        .as_str()
        .unwrap_or_else(|| panic!("no session: {created}"))
        .to_owned();

    let shared: Vec<String> = host
        .manager
        .list()
        .into_iter()
        .map(|record| record.conversation_id.to_string())
        .collect();
    assert_eq!(shared, vec![session_id]);
}

#[tokio::test]
async fn stopping_the_host_ends_its_sessions() {
    let home = TempDir::new().unwrap();
    let (config, overrides) = config(&home);
    let mut host = launch(&config, &overrides).unwrap();
    call(&mut host, 1, "initialize", json!({"protocolVersion":1})).await;
    call(
        &mut host,
        2,
        "session/new",
        json!({"cwd": home.path(), "mcpServers": [], "_meta":{"nori/agent":"mock-model"}}),
    )
    .await;
    let manager = host.manager.clone();
    let mut from_host = host.from_host;
    let draining = tokio::spawn(async move { while from_host.recv().await.is_some() {} });

    (host.stop)().await.unwrap();

    let live = |manager: &nori_harness::session_manager::SessionManager| {
        manager
            .list()
            .into_iter()
            .filter(|record| {
                matches!(
                    record.status,
                    SessionStatus::Starting
                        | SessionStatus::Working
                        | SessionStatus::WaitingOnYou
                        | SessionStatus::Ready
                )
            })
            .count()
    };
    let settled = tokio::time::timeout(Duration::from_secs(5), async {
        while live(&manager) > 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(
        settled.is_ok(),
        "sessions still running after stop: {:?}",
        manager.list()
    );
    draining.await.unwrap();
}

#[tokio::test]
async fn permission_on_the_foreground_pipe_is_answered_once_as_the_user() {
    use nori_protocol::AcpEvent;
    use nori_protocol::NoriEvent;
    use nori_protocol::SessionEvent;

    let home = TempDir::new().unwrap();
    let (config, overrides) = config(&home);
    let mut host = launch(&config, &overrides).unwrap();
    let mut events = host.manager.subscribe(256);
    call(&mut host, 1, "initialize", json!({})).await;
    let created = call(
        &mut host,
        2,
        "_nori/session/new",
        json!({"agent":"mock-model"}),
    )
    .await;
    let id = &created["result"]["session_id"];
    let prompted = call(
        &mut host,
        3,
        "_nori/session/prompt",
        json!({
            "session_id":id, "prompt":[{"type":"text","text":"mock:request-permission"}]
        }),
    )
    .await;
    assert!(prompted.get("error").is_none(), "{prompted}");
    let request = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let SessionEvent::Acp(AcpEvent::Request {
                request_id,
                request: nori_protocol::acp::v1::AgentRequest::RequestPermissionRequest(_),
            }) = events.recv().await.unwrap().event
            {
                break request_id;
            }
        }
    })
    .await
    .unwrap();
    let answer = json!({"session_id":id, "request_id":request, "responder":"remote_control",
        "response":{"outcome":{"outcome":"selected","optionId":"allow"}}});
    let accepted = call(&mut host, 4, "_nori/session/respond", answer.clone()).await;
    assert_eq!(accepted["result"]["answered"], true, "{accepted}");
    let duplicate = call(&mut host, 5, "_nori/session/respond", answer).await;
    assert_eq!(duplicate["error"]["code"], -32600);
    let resolved = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let SessionEvent::Nori(NoriEvent::RequestResolved(resolved)) =
                events.recv().await.unwrap().event
            {
                break resolved;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(resolved.request_id, request);
    assert_eq!(resolved.answered_by, "client");
    (host.stop)().await.unwrap();
}

#[tokio::test]
async fn queued_text_from_the_foreground_pipe_can_be_taken_back_by_its_composer() {
    let home = TempDir::new().unwrap();
    let (config, overrides) = config(&home);
    let mut host = launch(&config, &overrides).unwrap();
    call(&mut host, 1, "initialize", json!({})).await;
    let created = call(
        &mut host,
        2,
        "_nori/session/new",
        json!({"agent":"mock-model"}),
    )
    .await;
    let id = created["result"]["session_id"].as_str().unwrap();
    let turn = call(
        &mut host,
        3,
        "_nori/session/prompt",
        json!({
            "session_id":id,"prompt":[{"type":"text","text":"mock:request-permission"}]
        }),
    )
    .await;
    let queued = call(&mut host, 4, "_nori/session/follow_up", json!({
        "session_id":id,"expected_turn":turn["result"]["request_id"],"message_id":"sample-queued",
        "intent":"queue","prompt":[{"type":"text","text":"revise this line"}]
    })).await;
    assert_eq!(queued["result"]["status"], "queued", "{queued}");
    let handle = host
        .manager
        .handle(&nori_harness::ConversationId::from_string(id).unwrap())
        .unwrap();
    let recalled = handle
        .reclaim_queued_prompt("sample-queued".into())
        .await
        .unwrap();
    assert_eq!(recalled.as_deref(), Some("revise this line"));
    (host.stop)().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn a_pending_foreground_prompt_keeps_the_pipe_responsive() {
    let home = TempDir::new().unwrap();
    let (mut config, overrides) = config(&home);
    let gate = home.path().join("waiting.sh");
    std::fs::write(
        &gate,
        r#"cd "$(dirname "$0")"
touch entered
while [ ! -e release ]; do sleep 0.02; done
"#,
    )
    .unwrap();
    config.pre_user_prompt_hooks.push(gate);
    let mut host = launch(&config, &overrides).unwrap();
    call(&mut host, 1, "initialize", json!({})).await;
    let created = call(
        &mut host,
        2,
        "_nori/session/new",
        json!({"agent":"mock-model"}),
    )
    .await;
    host.to_host.send(json!({"jsonrpc":"2.0","id":3,"method":"_nori/session/prompt",
        "params":{"session_id":created["result"]["session_id"],"prompt":[{"type":"text","text":"sample"}]}}).to_string()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !home.path().join("entered").exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("prompt reached its hook");
    let listed = tokio::time::timeout(
        Duration::from_secs(2),
        call(&mut host, 4, "_nori/session/list", json!({})),
    )
    .await;
    std::fs::write(home.path().join("release"), "").unwrap();
    assert_eq!(
        listed.expect("host must answer while a prompt hook is waiting")["result"]["sessions"][0]["session_id"],
        created["result"]["session_id"]
    );
    (host.stop)().await.unwrap();
}
