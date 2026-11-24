//! Real WebSocket clients exercising the managed session protocol.

#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::SinkExt;
use futures::StreamExt;
use nori_config::NoriConfig;
use nori_harness::ConversationId;
use nori_harness::remote_agent::ManagedAgent;
use nori_harness::remote_agent::ManagedRemoteHost;
use nori_harness::remote_agent::RemoteAcpServer;
use nori_harness::runtime::AgentPrepareSpec;
use nori_harness::runtime::SessionStart;
use nori_harness::session_manager::SessionAgent;
use nori_harness::session_manager::SessionManager;
use nori_harness::session_manager::SessionManagerConfig;
use nori_harness::session_manager::SessionSource;
use nori_harness::session_manager::StartSession;
use nori_protocol::SessionEnvelope;
use nori_protocol::SessionStatus;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const WAIT: Duration = Duration::from_secs(15);
const TOKEN: &str = "managed-remote-test-token";
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

struct Fixture {
    manager: SessionManager,
    host: Arc<ManagedRemoteHost>,
    server: RemoteAcpServer,
    ids: [ConversationId; 2],
    _homes: [tempfile::TempDir; 2],
}

impl Fixture {
    async fn new(retained: i32) -> Self {
        let manager =
            SessionManager::with_replay_capacity(SessionManagerConfig::default(), retained);
        let homes = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
        let ids = std::array::from_fn(|index| {
            let session = manager
                .start(StartSession {
                    agent: SessionAgent::Prepare(AgentPrepareSpec {
                        config: Arc::new(NoriConfig {
                            active_agent: if index == 0 {
                                "mock-model"
                            } else {
                                "mock-model-alt"
                            }
                            .into(),
                            cwd: homes[index].path().into(),
                            nori_home: homes[index].path().into(),
                            ..Default::default()
                        }),
                        cli_version: "managed-remote-test".into(),
                        session_context: None,
                        initial_context: None,
                    }),
                    start: SessionStart::New,
                    parent: None,
                    source: SessionSource::User,
                })
                .unwrap();
            session.conversation_id
        });
        for id in ids {
            wait_status(&manager, id, SessionStatus::Ready).await;
        }
        let host = Arc::new(ManagedRemoteHost::authenticated_host(manager.clone()));
        host.set_selected_session(Some(ids[0]));
        let server = RemoteAcpServer::bind_host(
            "127.0.0.1:0".parse().unwrap(),
            host.clone(),
            TOKEN.to_string(),
        )
        .await
        .unwrap();
        Self {
            manager,
            host,
            server,
            ids,
            _homes: homes,
        }
    }

    /// Connect as the controller, once any previous controller has left.
    async fn connect(&self) -> Socket {
        tokio::time::timeout(WAIT, async {
            while self.server.controller_connected().await {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let mut socket = self.open().await;
        send(&mut socket, 0, "initialize", json!({})).await;
        let initialized = receive(&mut socket).await;
        assert!(
            initialized["result"]["capabilities"]["nori_sessions"].is_object(),
            "{initialized}"
        );
        assert_eq!(initialized["result"]["control"]["role"], "controller");
        socket
    }

    async fn open(&self) -> Socket {
        let mut request = format!("ws://{}/sessions", self.server.local_addr())
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {TOKEN}").parse().unwrap());
        let (socket, _) = connect_async(request).await.unwrap();
        socket
    }

    async fn stop(self) {
        self.server.shutdown().await;
        self.manager.shutdown(Duration::ZERO).await;
    }
}

async fn wait_status(manager: &SessionManager, id: ConversationId, status: SessionStatus) {
    tokio::time::timeout(WAIT, async {
        while manager.get(&id).unwrap().status != status {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

async fn send(socket: &mut Socket, id: i64, method: &str, params: Value) {
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
}

async fn receive(socket: &mut Socket) -> Value {
    tokio::time::timeout(WAIT, async {
        loop {
            match socket.next().await.unwrap().unwrap() {
                Message::Text(text) => return serde_json::from_str(&text).unwrap(),
                Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await.unwrap(),
                other => panic!("unexpected websocket frame: {other:?}"),
            }
        }
    })
    .await
    .unwrap()
}

fn event(value: &Value) -> Option<SessionEnvelope> {
    (value["method"] == "_nori/session/event")
        .then(|| serde_json::from_value(value["params"].clone()).unwrap())
}

fn prompt(id: ConversationId, text: &str) -> Value {
    json!({"session_id":id.to_string(), "prompt":[{"type":"text", "text":text}]})
}

#[tokio::test]
async fn websocket_two_sessions_reconnect_without_lost_or_duplicate_envelopes() {
    let fixture = Fixture::new(4096).await;
    let mut reference = fixture
        .manager
        .subscribe_from(None, &HashMap::new(), 4096)
        .unwrap();
    let mut expected = reference.replay;
    let mut socket = fixture.connect().await;
    send(&mut socket, 1, "_nori/session/list", json!({})).await;
    let listed = receive(&mut socket).await;
    assert_eq!(listed["result"]["sessions"].as_array().unwrap().len(), 2);
    for row in listed["result"]["sessions"].as_array().unwrap() {
        assert!(row["latest_seq"].as_i64().unwrap() > 0);
        assert!(row["first_available_seq"].as_i64().unwrap() > 0);
        assert!(row["age_seconds"].is_i64());
    }
    assert_eq!(
        listed["result"]["selected_session_id"],
        fixture.ids[0].to_string()
    );
    send(
        &mut socket,
        2,
        "_nori/session/subscribe",
        json!({"session_id":"*"}),
    )
    .await;
    let mut received = Vec::new();
    let mut cursors = HashMap::new();
    for (index, id) in fixture.ids.iter().enumerate() {
        send(
            &mut socket,
            10 + index as i64,
            "_nori/session/prompt",
            prompt(*id, "mock:stream-until-cancel"),
        )
        .await;
    }
    let mut streamed = [false; 2];
    while !streamed.iter().all(|seen| *seen) {
        let value = receive(&mut socket).await;
        assert!(value.get("error").is_none(), "{value}");
        if let Some(envelope) = event(&value) {
            if let nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
                nori_protocol::acp::v1::AgentNotification::SessionNotification(notification),
            )) = &envelope.event
                && let nori_protocol::acp::v1::SessionUpdate::AgentMessageChunk(chunk) =
                    &notification.update
                && let nori_protocol::acp::v1::ContentBlock::Text(text) = &chunk.content
                && text.text == "Streaming..."
            {
                for (index, id) in fixture.ids.iter().enumerate() {
                    if envelope.conversation_id == id.to_string() {
                        streamed[index] = true;
                    }
                }
            }
            cursors.insert(envelope.conversation_id.clone(), envelope.seq);
            received.push(value["params"].clone());
        }
    }
    drop(socket);
    // Both agent processes keep producing while no remote socket exists.
    tokio::time::sleep(Duration::from_millis(100)).await;
    for id in fixture.ids {
        fixture.manager.handle(&id).unwrap().cancel().await.unwrap();
        wait_status(&fixture.manager, id, SessionStatus::Ready).await;
    }
    while let Ok(envelope) = reference.events.try_recv() {
        expected.push(envelope);
    }
    let expected_len = expected.len();
    let mut socket = fixture.connect().await;
    send(
        &mut socket,
        3,
        "_nori/session/subscribe",
        json!({"session_id":"*", "cursors":cursors}),
    )
    .await;
    while received.len() < expected_len {
        let value = receive(&mut socket).await;
        assert!(value.get("error").is_none(), "{value}");
        if value["method"] == "_nori/session/event" {
            received.push(value["params"].clone());
        }
    }
    // Compare the raw wire payload: ACP response enums are untagged, so
    // deserializing a response without its method can select an empty variant.
    let expected = expected
        .iter()
        .map(|envelope| serde_json::to_value(envelope).unwrap())
        .collect::<Vec<_>>();
    // Cross-session interleaving is not a protocol guarantee; each complete
    // session stream must exactly equal the independent manager observer.
    for id in fixture.ids {
        let owned = |values: &[Value]| {
            values
                .iter()
                .filter(|e| e["conversation_id"] == id.to_string())
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(owned(&received), owned(&expected));
        let sequences = received
            .iter()
            .filter(|e| e["conversation_id"] == id.to_string())
            .map(|e| e["seq"].as_i64().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(sequences, (1..=sequences.len() as i64).collect::<Vec<_>>());
    }
    fixture.stop().await;
}

#[tokio::test]
async fn selection_filters_events_and_permissions_survive_disconnect() {
    let fixture = Fixture::new(4096).await;
    let mut socket = fixture.connect().await;
    send(
        &mut socket,
        1,
        "_nori/session/subscribe",
        json!({"session_id":"*"}),
    )
    .await;
    for (index, id) in fixture.ids.iter().enumerate() {
        send(
            &mut socket,
            2 + index as i64,
            "_nori/session/prompt",
            prompt(*id, "mock:request-permission"),
        )
        .await;
    }
    for id in fixture.ids {
        wait_status(&fixture.manager, id, SessionStatus::WaitingOnYou).await;
    }
    drop(socket);
    let requests = fixture
        .ids
        .map(|id| fixture.manager.get(&id).unwrap().pending_requests[0].clone());
    let mut socket = fixture.connect().await;
    send(
        &mut socket,
        10,
        "_nori/session/subscribe",
        json!({"session_id":fixture.ids[0].to_string()}),
    )
    .await;
    send(&mut socket, 11, "_nori/session/respond", json!({"session_id":fixture.ids[0].to_string(), "request_id":requests[0], "responder":"client", "response":{"outcome":{"outcome":"selected", "optionId":"allow"}}})).await;
    let mut seen = Vec::new();
    let mut answered = false;
    let mut idle = false;
    while !answered || !idle {
        let value = receive(&mut socket).await;
        assert!(value.get("error").is_none(), "{value}");
        if let Some(envelope) = event(&value) {
            assert_eq!(envelope.conversation_id, fixture.ids[0].to_string());
            idle |= answered
                && matches!(
                    &envelope.event,
                    nori_protocol::SessionEvent::Nori(
                        nori_protocol::NoriEvent::SessionPhaseChanged(
                            nori_protocol::SessionPhase::Idle
                        )
                    )
                );
            seen.push(envelope);
        }
        answered |= value["id"] == 11;
    }
    let responders: Vec<_> = seen
        .iter()
        .filter_map(|envelope| match &envelope.event {
            nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::RequestResolved(
                answer,
            )) if answer.request_id == requests[0] => Some(answer.answered_by.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(responders, ["remote_control"]);
    wait_status(&fixture.manager, fixture.ids[0], SessionStatus::Ready).await;
    assert_eq!(
        fixture.manager.get(&fixture.ids[1]).unwrap().status,
        SessionStatus::WaitingOnYou
    );
    assert!(
        !seen.is_empty(),
        "one-session subscription actually delivered events"
    );
    assert!(
        seen.iter()
            .all(|e| e.conversation_id == fixture.ids[0].to_string())
    );
    send(&mut socket, 12, "_nori/session/respond", json!({"session_id":fixture.ids[1].to_string(), "request_id":requests[1], "response":{"outcome":{"outcome":"selected", "optionId":"allow"}}})).await;
    loop {
        let value = receive(&mut socket).await;
        assert!(value.get("error").is_none(), "{value}");
        if let Some(envelope) = event(&value) {
            assert_eq!(envelope.conversation_id, fixture.ids[0].to_string());
        }
        if value["id"] == 12 {
            break;
        }
    }
    wait_status(&fixture.manager, fixture.ids[1], SessionStatus::Ready).await;
    fixture.stop().await;
}

#[tokio::test]
async fn default_target_follows_selection_and_explicit_target_wins() {
    let fixture = Fixture::new(4096).await;
    fixture.host.set_selected_session(Some(fixture.ids[0]));
    let reply = fixture.host.request(json!({"jsonrpc":"2.0","id":1,"method":"_nori/session/prompt","params":{"prompt":[{"type":"text","text":"mock:request-permission"}]}})).await;
    assert!(reply.response.get("error").is_none(), "{}", reply.response);
    wait_status(
        &fixture.manager,
        fixture.ids[0],
        SessionStatus::WaitingOnYou,
    )
    .await;
    assert_eq!(
        fixture.manager.get(&fixture.ids[1]).unwrap().status,
        SessionStatus::Ready
    );
    let explicit = fixture.host.request(json!({"jsonrpc":"2.0","id":3,"method":"_nori/session/prompt","params":prompt(fixture.ids[1], "mock:request-permission")})).await;
    assert!(
        explicit.response.get("error").is_none(),
        "{}",
        explicit.response
    );
    wait_status(
        &fixture.manager,
        fixture.ids[1],
        SessionStatus::WaitingOnYou,
    )
    .await;
    fixture.host.set_selected_session(Some(fixture.ids[1]));
    let listed = fixture
        .host
        .request(json!({"jsonrpc":"2.0","id":2,"method":"_nori/session/list"}))
        .await;
    assert_eq!(
        listed.response["result"]["selected_session_id"],
        fixture.ids[1].to_string()
    );
    fixture.stop().await;
}

#[tokio::test]
async fn invalid_cursors_are_errors_and_overflow_drops_only_subscriber() {
    let fixture = Fixture::new(2).await;
    let id = fixture.ids[0];
    assert!(
        fixture
            .manager
            .subscribe_from(Some(id), &HashMap::new(), 2)
            .is_err(),
        "initial history expired"
    );
    assert!(
        fixture
            .manager
            .subscribe_from(Some(ConversationId::new()), &HashMap::new(), 2)
            .is_err()
    );
    for cursor in [-1, i64::MAX] {
        assert!(
            fixture
                .manager
                .subscribe_from(Some(id), &HashMap::from([(id.to_string(), cursor)]), 2)
                .is_err()
        );
    }
    let mut observer = fixture.manager.subscribe(4096);
    let mut slow = fixture.manager.subscribe(1);
    fixture
        .manager
        .handle(&id)
        .unwrap()
        .prompt(vec![nori_protocol::acp::v1::ContentBlock::Text(
            nori_protocol::acp::v1::TextContent::new("mock:stream-until-cancel"),
        )])
        .await
        .unwrap();
    for _ in 0..4 {
        tokio::time::timeout(WAIT, observer.recv())
            .await
            .unwrap()
            .unwrap();
    }
    assert!(
        tokio::time::timeout(WAIT, slow.recv())
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        tokio::time::timeout(WAIT, slow.recv())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fixture.manager.get(&id).unwrap().status,
        SessionStatus::Working
    );
    assert!(
        fixture
            .manager
            .subscribe_from(Some(id), &HashMap::from([(id.to_string(), 0)]), 2)
            .is_err()
    );
    fixture.manager.handle(&id).unwrap().cancel().await.unwrap();
    wait_status(&fixture.manager, id, SessionStatus::Ready).await;
    let listed = fixture
        .host
        .request(json!({"jsonrpc":"2.0","id":40,"method":"_nori/session/list"}))
        .await;
    let latest = listed.response["result"]["sessions"][0]["latest_seq"]
        .as_i64()
        .unwrap();
    assert!(
        fixture
            .manager
            .subscribe_from(Some(id), &HashMap::from([(id.to_string(), latest)]), 2)
            .is_ok(),
        "an explicit latest cursor can observe live events after expiry"
    );
    fixture.stop().await;
}

#[tokio::test]
async fn managed_endpoint_requires_initialize_first() {
    let fixture = Fixture::new(4096).await;
    let mut socket = fixture.open().await;
    send(&mut socket, 1, "_nori/session/list", json!({})).await;
    let rejected = receive(&mut socket).await;
    assert_eq!(rejected["error"]["code"], -32001);
    fixture.stop().await;
}

#[tokio::test]
async fn malformed_requests_and_unknown_targets_do_not_drive_any_session() {
    let fixture = Fixture::new(4096).await;
    for request in [
        json!({"method":"_nori/session/list","id":1}),
        json!({"jsonrpc":"2.0","method":"_nori/session/list"}),
        json!({"jsonrpc":"2.0","method":"_nori/session/list","id":1,"params":[]}),
    ] {
        let reply = fixture.host.request(request).await;
        assert_eq!(reply.response["error"]["code"], -32600);
    }
    let reply = fixture.host.request(json!({"jsonrpc":"2.0","id":2,"method":"_nori/session/prompt","params":prompt(ConversationId::new(), "mock:request-permission")})).await;
    assert_eq!(reply.response["error"]["code"], -32002);
    for id in fixture.ids {
        assert_eq!(
            fixture.manager.get(&id).unwrap().status,
            SessionStatus::Ready
        );
    }
    fixture.stop().await;
}

async fn remote_turn(
    socket: &mut Socket,
    rpc_id: i64,
    session_id: ConversationId,
    text: &str,
    after_seq: i64,
) -> Vec<Value> {
    send(
        socket,
        rpc_id,
        "_nori/session/prompt",
        prompt(session_id, text),
    )
    .await;
    let mut admitted = None;
    let mut completed = Vec::new();
    let mut events = Vec::new();
    loop {
        let value = receive(socket).await;
        assert!(value.get("error").is_none(), "{value}");
        if value["id"] == rpc_id {
            assert_eq!(value["result"]["session_id"], session_id.to_string());
            admitted = Some(
                serde_json::from_value::<nori_protocol::acp::v1::RequestId>(
                    value["result"]["request_id"].clone(),
                )
                .unwrap(),
            );
        }
        if let Some(envelope) = event(&value) {
            assert_eq!(envelope.conversation_id, session_id.to_string());
            if let nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
                request_id,
                response: Ok(_),
            }) = envelope.event
                && envelope.seq > after_seq
            {
                completed.push(request_id);
            }
            events.push(value["params"].clone());
        }
        if admitted
            .as_ref()
            .is_some_and(|request| completed.contains(request))
        {
            return events;
        }
    }
}

#[tokio::test]
async fn websocket_subscription_and_precrash_cursor_follow_the_same_id_after_resume() {
    let fixture = Fixture::new(4096).await;
    let id = fixture.ids[0];
    let mut socket = fixture.connect().await;
    send(
        &mut socket,
        1,
        "_nori/session/subscribe",
        json!({"session_id":id.to_string()}),
    )
    .await;
    let before = remote_turn(&mut socket, 2, id, "before crash", 0).await;
    let cursor = before.last().unwrap()["seq"].as_i64().unwrap();
    fixture
        .manager
        .handle(&id)
        .unwrap()
        .flush_transcript()
        .await
        .unwrap();
    let reference = fixture
        .manager
        .subscribe_from(Some(id), &HashMap::from([(id.to_string(), cursor)]), 4096)
        .unwrap();
    // The WebSocket and its original single-session subscription stay open
    // throughout the agent process replacement.
    fixture
        .manager
        .handle(&id)
        .unwrap()
        .prompt(vec![nori_protocol::acp::v1::ContentBlock::Text(
            nori_protocol::acp::v1::TextContent::new("mock:exit-during-prompt"),
        )])
        .await
        .unwrap();
    wait_status(&fixture.manager, id, SessionStatus::Failed).await;
    let resumed = fixture
        .manager
        .resume(
            &id,
            AgentPrepareSpec {
                config: Arc::new(NoriConfig {
                    active_agent: "mock-model".into(),
                    cwd: fixture._homes[0].path().into(),
                    nori_home: fixture._homes[0].path().into(),
                    ..Default::default()
                }),
                cli_version: "managed-remote-test".into(),
                session_context: None,
                initial_context: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(resumed.conversation_id, id);
    wait_status(&fixture.manager, id, SessionStatus::Ready).await;
    let resumed_cursor = fixture.manager.replay_info(&id).unwrap().latest_seq;
    let continued = remote_turn(&mut socket, 3, id, "after crash", resumed_cursor).await;
    assert!(
        continued.iter().any(|envelope| {
            serde_json::from_value::<SessionEnvelope>(envelope.clone()).is_ok_and(|envelope| {
                matches!(
                    envelope.event,
                    nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionStarted(_))
                )
            })
        }),
        "the existing subscription sees the replacement process start"
    );
    let through = continued.last().unwrap()["seq"].as_i64().unwrap();
    assert_eq!(
        continued
            .iter()
            .map(|event| event["seq"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        ((cursor + 1)..=through).collect::<Vec<_>>()
    );
    let mut expected = reference
        .replay
        .into_iter()
        .map(|event| serde_json::to_value(event).unwrap())
        .collect::<Vec<_>>();
    let mut reference_events = reference.events;
    while expected
        .last()
        .and_then(|event| event["seq"].as_i64())
        .unwrap_or(cursor)
        < through
    {
        expected.push(
            serde_json::to_value(
                tokio::time::timeout(WAIT, reference_events.recv())
                    .await
                    .unwrap()
                    .unwrap(),
            )
            .unwrap(),
        );
    }
    assert_eq!(
        continued, expected,
        "the original subscription preserves every envelope"
    );
    drop(socket);
    let mut socket = fixture.connect().await;
    send(&mut socket, 4, "_nori/session/list", json!({})).await;
    let listed = receive(&mut socket).await;
    assert_eq!(listed["result"]["selected_session_id"], id.to_string());
    assert_eq!(
        listed["result"]["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["session_id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>(),
        fixture.ids.map(|id| id.to_string()).to_vec()
    );
    assert!(
        listed["result"]["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row.get("resumed_from").is_none()),
        "restart bookkeeping is internal"
    );
    send(
        &mut socket,
        5,
        "_nori/session/subscribe",
        json!({"session_id":id.to_string(),"cursors":HashMap::from([(id.to_string(),cursor)])}),
    )
    .await;
    let mut replayed = Vec::new();
    while replayed.len() < continued.len() {
        let value = receive(&mut socket).await;
        assert!(value.get("error").is_none(), "{value}");
        if value["method"] == "_nori/session/event" {
            replayed.push(value["params"].clone());
        }
    }
    assert_eq!(
        replayed, continued,
        "a pre-crash cursor replays across restart without gaps or duplicates"
    );
    fixture.stop().await;
}

#[tokio::test]
async fn delegation_remote_admission_approval_and_child_events_keep_their_identity() {
    let fixture = Fixture::new(4096).await;
    let mut controller = fixture.connect().await;
    let parent = fixture.ids[0].to_string();
    send(
        &mut controller,
        2,
        "_nori/session/delegate",
        json!({
            "session_id":parent, "agent":"mock-model-alt", "task":"mock:request-permission"
        }),
    )
    .await;
    let requested = receive(&mut controller).await;
    assert!(requested["error"].is_null(), "{requested}");
    let delegation = &requested["result"];
    assert_eq!(delegation["status"], "awaiting_approval");
    assert_eq!(
        fixture.manager.list().len(),
        2,
        "approval cannot spawn a process"
    );
    send(
        &mut controller,
        3,
        "_nori/session/delegation/resolve",
        json!({
            "session_id":parent, "request_id":delegation["request_id"], "approved":true
        }),
    )
    .await;
    let approved = receive(&mut controller).await;
    assert!(approved["error"].is_null(), "{approved}");
    let child = approved["result"]["child_id"].as_str().unwrap().to_owned();
    let child_id = ConversationId::from_string(&child).unwrap();
    wait_status(&fixture.manager, child_id, SessionStatus::WaitingOnYou).await;
    send(&mut controller, 4, "_nori/session/list", json!({})).await;
    let listed = receive(&mut controller).await;
    let rows = listed["result"]["sessions"].as_array().unwrap();
    let row = rows.iter().find(|row| row["session_id"] == child).unwrap();
    assert_eq!(row["parent"], parent);
    assert_eq!(row["source"], "session");
    assert_eq!(row["depth"], 1);
    assert_eq!(row["status"], "waiting_on_you");
    send(
        &mut controller,
        5,
        "_nori/session/subscribe",
        json!({"session_id":"*"}),
    )
    .await;
    assert!(receive(&mut controller).await["error"].is_null());
    let child_permission = tokio::time::timeout(WAIT, async {
        loop {
            let value = receive(&mut controller).await;
            if let Some(envelope) = event(&value)
                && envelope.conversation_id == child
                && matches!(
                    envelope.event,
                    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Request { .. })
                )
            {
                return envelope;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(child_permission.agent, "mock-model-alt");
    assert!(
        fixture
            .manager
            .get(&fixture.ids[0])
            .unwrap()
            .pending_requests
            .is_empty()
    );
    fixture.stop().await;
}

#[tokio::test]
async fn delegation_remote_decline_and_duplicate_approval_never_spawn_a_child() {
    let fixture = Fixture::new(4096).await;
    let mut socket = fixture.connect().await;
    let parent = fixture.ids[0].to_string();
    send(
        &mut socket,
        1,
        "_nori/session/delegate",
        json!({
            "session_id":parent, "agent":"mock-model-alt", "task":"do not start"
        }),
    )
    .await;
    let requested = receive(&mut socket).await;
    assert!(requested["error"].is_null(), "{requested}");
    let params = json!({"session_id":parent, "request_id":requested["result"]["request_id"], "approved":false});
    send(
        &mut socket,
        2,
        "_nori/session/delegation/resolve",
        params.clone(),
    )
    .await;
    assert_eq!(receive(&mut socket).await["result"]["status"], "declined");
    send(&mut socket, 3, "_nori/session/delegation/resolve", json!({"session_id":parent, "request_id":requested["result"]["request_id"], "approved":true})).await;
    assert!(receive(&mut socket).await["error"].is_object());
    assert_eq!(fixture.manager.list().len(), 2);
    fixture.stop().await;
}

#[tokio::test]
async fn session_list_rows_carry_the_live_title_reply_model_activity_and_context() {
    let fixture = Fixture::new(4096).await;
    let id = fixture.ids[0];
    let before = fixture
        .host
        .request(json!({"jsonrpc":"2.0","id":1,"method":"_nori/session/list"}))
        .await;
    let before = before.response["result"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["session_id"] == id.to_string())
        .cloned()
        .unwrap();
    let sent = fixture
        .host
        .request(
            json!({"jsonrpc":"2.0","id":2,"method":"_nori/session/prompt","params":prompt(
                id,
                "say hi in 3 words\nmock:title Greeting in three words\nmock:usage 38000 475000",
            )}),
        )
        .await;
    assert!(sent.response.get("error").is_none(), "{}", sent.response);
    let row = tokio::time::timeout(WAIT, async {
        loop {
            let listed = fixture
                .host
                .request(json!({"jsonrpc":"2.0","id":3,"method":"_nori/session/list"}))
                .await;
            let row = listed.response["result"]["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["session_id"] == id.to_string())
                .cloned()
                .unwrap();
            if row["status"] == "ready" && row["reply_preview"].is_string() {
                return row;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(before["reply_preview"], Value::Null, "{before}");
    assert_eq!(row["title"], "Greeting in three words", "{row}");
    assert_eq!(row["model"], "Mock Default Model", "{row}");
    assert_eq!(
        (row["context_used"].as_i64(), row["context_size"].as_i64()),
        (Some(38_000), Some(475_000)),
        "{row}"
    );
    let reply = row["reply_preview"].as_str().unwrap();
    assert!(reply.starts_with("Test message 1"), "{row}");
    assert!(
        row["idle_seconds"]
            .as_i64()
            .is_some_and(|idle| (0..5).contains(&idle)),
        "{row}"
    );
    fixture.stop().await;
}

#[tokio::test]
async fn a_waiting_sessions_list_row_says_what_it_asks_for() {
    let fixture = Fixture::new(4096).await;
    let id = fixture.ids[0];
    let sent = fixture
        .host
        .request(
            json!({"jsonrpc":"2.0","id":1,"method":"_nori/session/prompt","params":prompt(
                id,
                "mock:request-permission:cargo test -p parser",
            )}),
        )
        .await;
    assert!(sent.response.get("error").is_none(), "{}", sent.response);
    let row = tokio::time::timeout(WAIT, async {
        loop {
            let listed = fixture
                .host
                .request(json!({"jsonrpc":"2.0","id":2,"method":"_nori/session/list"}))
                .await;
            let row = listed.response["result"]["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["session_id"] == id.to_string())
                .cloned()
                .unwrap();
            if row["status"] == "waiting_on_you" {
                return row;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let request = row["waiting_request"].as_str().unwrap_or_default();
    assert!(
        request.starts_with("approve: ") && request.contains("cargo test -p parser"),
        "{row}"
    );
    let idle = fixture
        .host
        .request(json!({"jsonrpc":"2.0","id":3,"method":"_nori/session/list"}))
        .await;
    let other = idle.response["result"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["session_id"] == fixture.ids[1].to_string())
        .cloned()
        .unwrap();
    assert_eq!(other["waiting_request"], Value::Null, "{other}");

    let answered = fixture
        .host
        .request(
            json!({"jsonrpc":"2.0","id":4,"method":"_nori/session/respond","params":{
                "session_id":id.to_string(), "request_id":row["pending_requests"][0],
                "response":{"outcome":{"outcome":"selected","optionId":"allow"}}
            }}),
        )
        .await;
    assert!(
        answered.response.get("error").is_none(),
        "{}",
        answered.response
    );
    let after = tokio::time::timeout(WAIT, async {
        loop {
            let listed = fixture
                .host
                .request(json!({"jsonrpc":"2.0","id":5,"method":"_nori/session/list"}))
                .await;
            let row = listed.response["result"]["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["session_id"] == id.to_string())
                .cloned()
                .unwrap();
            if row["status"] == "ready" {
                return row;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        after["waiting_request"],
        Value::Null,
        "an answered request clears: {after}"
    );
    fixture.stop().await;
}
