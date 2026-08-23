//! Wire-contract tests for the remote ACP transport (WebSocket profile).
//!
//! A fake [`HostedAgent`] stands in for the harness so these tests pin the
//! transport behavior alone: upgrade headers, initialize-first, framing,
//! last-connect-wins, replay ordering, and delegated-request round-trips.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::SinkExt;
use futures::StreamExt;
use nori_acp_host::remote::HostedAgent;
use nori_acp_host::remote::HostedSubscription;
use nori_acp_host::remote::LoadedSession;
use nori_acp_host::remote::RemoteAcpServer;
use nori_acp_host::remote::parse_bind_addr;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEndReason;
use nori_protocol::SessionEnded;
use nori_protocol::SessionEvent;
use nori_protocol::acp::v1 as acp;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

const SESSION_ID: &str = "11111111-2222-3333-4444-555555555555";
const RECV_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Default)]
struct FakeState {
    subscription_seq: i64,
    sink: Option<mpsc::Sender<SessionEvent>>,
    prompts: Vec<(acp::SessionId, Vec<acp::ContentBlock>)>,
    responds: Vec<(acp::RequestId, Result<acp::ClientResponse, acp::Error>)>,
    detaches: Vec<i64>,
    replay: Vec<acp::SessionNotification>,
}

#[derive(Default)]
struct FakeHosted {
    state: Mutex<FakeState>,
}

#[expect(
    clippy::expect_used,
    reason = "fixture failures should fail the test loudly"
)]
impl FakeHosted {
    fn session_id() -> acp::SessionId {
        acp::SessionId::new(SESSION_ID)
    }

    fn check(session_id: &acp::SessionId) -> Result<(), acp::Error> {
        if session_id == &Self::session_id() {
            Ok(())
        } else {
            Err(acp::Error::new(-32002, "unknown session"))
        }
    }

    async fn push(&self, event: SessionEvent) {
        let sink = self.state.lock().await.sink.clone();
        sink.expect("no active subscription")
            .send(event)
            .await
            .expect("subscriber gone");
    }

    fn update(text: &str) -> acp::SessionNotification {
        acp::SessionNotification::new(
            Self::session_id(),
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new(text),
            ))),
        )
    }
}

#[expect(
    clippy::expect_used,
    reason = "fixture failures should fail the test loudly"
)]
impl HostedAgent for FakeHosted {
    async fn list_sessions(&self) -> Result<Vec<acp::SessionInfo>, acp::Error> {
        Ok(vec![
            acp::SessionInfo::new(Self::session_id(), "/tmp/fake").title("fake session"),
        ])
    }

    async fn load_session(&self, session_id: &acp::SessionId) -> Result<LoadedSession, acp::Error> {
        Self::check(session_id)?;
        Ok(LoadedSession {
            replay: self.state.lock().await.replay.clone(),
        })
    }

    async fn resume_session(&self, session_id: &acp::SessionId) -> Result<(), acp::Error> {
        Self::check(session_id)
    }

    async fn prompt(
        &self,
        session_id: &acp::SessionId,
        prompt: Vec<acp::ContentBlock>,
    ) -> Result<acp::RequestId, acp::Error> {
        Self::check(session_id)?;
        let request_id = acp::RequestId::Number(42);
        let sink = {
            let mut state = self.state.lock().await;
            state.prompts.push((session_id.clone(), prompt));
            state.sink.clone()
        };
        if let Some(sink) = sink {
            for text in ["Hello ", "world"] {
                let event = SessionEvent::Acp(AcpEvent::Notification(
                    acp::AgentNotification::SessionNotification(Self::update(text)),
                ));
                sink.send(event).await.expect("subscriber gone");
            }
            let response = SessionEvent::Acp(AcpEvent::Response {
                request_id: request_id.clone(),
                response: Ok(acp::AgentResponse::PromptResponse(
                    acp::PromptResponse::new(acp::StopReason::EndTurn),
                )),
            });
            sink.send(response).await.expect("subscriber gone");
        }
        Ok(request_id)
    }

    async fn cancel(&self, session_id: &acp::SessionId) -> Result<(), acp::Error> {
        Self::check(session_id)
    }

    async fn close_session(&self, session_id: &acp::SessionId) -> Result<(), acp::Error> {
        Self::check(session_id)
    }

    async fn respond(
        &self,
        request_id: acp::RequestId,
        response: Result<acp::ClientResponse, acp::Error>,
    ) -> Result<(), acp::Error> {
        self.state
            .lock()
            .await
            .responds
            .push((request_id, response));
        Ok(())
    }

    async fn subscribe(&self) -> HostedSubscription {
        let (tx, rx) = mpsc::channel(64);
        let mut state = self.state.lock().await;
        state.subscription_seq += 1;
        state.sink = Some(tx);
        HostedSubscription {
            id: state.subscription_seq,
            events: rx,
        }
    }

    async fn detach(&self, subscription_id: i64) {
        self.state.lock().await.detaches.push(subscription_id);
    }
}

struct TestServer {
    server: RemoteAcpServer,
    hosted: Arc<FakeHosted>,
}

#[expect(
    clippy::expect_used,
    reason = "fixture failures should fail the test loudly"
)]
async fn start_server() -> TestServer {
    let hosted = Arc::new(FakeHosted::default());
    let server = RemoteAcpServer::bind(
        "127.0.0.1:0".parse::<SocketAddr>().expect("addr"),
        hosted.clone(),
    )
    .await
    .expect("bind remote server");
    TestServer { server, hosted }
}

struct WsClient {
    ws: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    next_id: i64,
}

#[expect(
    clippy::expect_used,
    reason = "fixture failures should fail the test loudly"
)]
impl WsClient {
    async fn connect(
        addr: SocketAddr,
    ) -> (
        Self,
        tokio_tungstenite::tungstenite::handshake::client::Response,
    ) {
        let (ws, response) = connect_async(format!("ws://{addr}/acp"))
            .await
            .expect("websocket connect");
        (Self { ws, next_id: 0 }, response)
    }

    async fn send_json(&mut self, message: Value) {
        self.ws
            .send(Message::text(message.to_string()))
            .await
            .expect("send frame");
    }

    /// Next raw frame within the timeout; None on close/EOF.
    async fn next_frame(&mut self) -> Option<Message> {
        let frame = tokio::time::timeout(RECV_TIMEOUT, self.ws.next())
            .await
            .expect("timed out waiting for frame")?;
        Some(frame.expect("read frame"))
    }

    /// Next text frame parsed as one JSON-RPC message.
    async fn next_json(&mut self) -> Value {
        loop {
            match self.next_frame().await.expect("connection closed") {
                Message::Text(text) => {
                    return serde_json::from_str(text.as_str()).expect("frame is one JSON message");
                }
                Message::Ping(_) | Message::Pong(_) => {}
                other => panic!("unexpected frame: {other:?}"),
            }
        }
    }

    /// Send a request and read messages until its response arrives, returning
    /// (messages seen before the response, response result-or-error object).
    async fn request(&mut self, method: &str, params: Value) -> (Vec<Value>, Value) {
        self.next_id += 1;
        let id = self.next_id;
        self.send_json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await;
        let mut before = Vec::new();
        loop {
            let message = self.next_json().await;
            if message.get("id") == Some(&json!(id)) {
                return (before, message);
            }
            before.push(message);
        }
    }

    async fn initialize(&mut self) -> Value {
        let (_, response) = self
            .request(
                "initialize",
                json!({ "protocolVersion": 1, "clientCapabilities": {} }),
            )
            .await;
        response
    }
}

#[tokio::test]
async fn upgrade_response_carries_fresh_acp_connection_id() {
    let test = start_server().await;
    let (_client_a, response_a) = WsClient::connect(test.server.local_addr()).await;
    let (_client_b, response_b) = WsClient::connect(test.server.local_addr()).await;

    let id_a = response_a
        .headers()
        .get("Acp-Connection-Id")
        .expect("Acp-Connection-Id on 101 response")
        .to_str()
        .expect("header is ASCII")
        .to_owned();
    let id_b = response_b
        .headers()
        .get("Acp-Connection-Id")
        .expect("Acp-Connection-Id on 101 response")
        .to_str()
        .expect("header is ASCII")
        .to_owned();
    assert!(!id_a.is_empty());
    assert_ne!(id_a, id_b);
}

#[tokio::test]
async fn plain_http_request_gets_upgrade_required() {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;

    let test = start_server().await;
    let mut stream = tokio::net::TcpStream::connect(test.server.local_addr())
        .await
        .expect("tcp connect");
    stream
        .write_all(b"GET /acp HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("write request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .expect("read response");
    assert!(
        response.starts_with("HTTP/1.1 426"),
        "expected 426 Upgrade Required, got: {response}"
    );
}

#[tokio::test]
async fn first_message_must_be_initialize() {
    let test = start_server().await;
    let (mut client, _) = WsClient::connect(test.server.local_addr()).await;
    client
        .send_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "session/list",
            "params": {},
        }))
        .await;

    match client.next_frame().await {
        Some(Message::Close(Some(frame))) => {
            assert_eq!(frame.code, CloseCode::Protocol);
        }
        other => panic!("expected protocol-error close, got: {other:?}"),
    }
}

#[tokio::test]
async fn initialize_advertises_load_session_and_session_capabilities() {
    let test = start_server().await;
    let (mut client, _) = WsClient::connect(test.server.local_addr()).await;
    let response = client.initialize().await;

    let result = &response["result"];
    assert_eq!(result["protocolVersion"], json!(1));
    assert_eq!(result["agentCapabilities"]["loadSession"], json!(true));
    assert!(result["agentCapabilities"]["sessionCapabilities"]["list"].is_object());
    assert!(result["agentCapabilities"]["sessionCapabilities"]["resume"].is_object());
    assert_eq!(result["agentInfo"]["name"], json!("nori"));
}

#[tokio::test]
async fn binary_frames_are_ignored() {
    let test = start_server().await;
    let (mut client, _) = WsClient::connect(test.server.local_addr()).await;
    client
        .ws
        .send(Message::binary(vec![1, 2, 3]))
        .await
        .expect("send binary");
    let response = client.initialize().await;
    assert_eq!(
        response["result"]["agentCapabilities"]["loadSession"],
        json!(true)
    );
}

#[tokio::test]
async fn unparseable_first_frame_gets_parse_error_then_initialize_still_works() {
    let test = start_server().await;
    let (mut client, _) = WsClient::connect(test.server.local_addr()).await;
    client
        .ws
        .send(Message::text("this is not json"))
        .await
        .expect("send junk");
    let error = client.next_json().await;
    assert_eq!(error["error"]["code"], json!(-32700));
    assert_eq!(error["id"], Value::Null);

    let response = client.initialize().await;
    assert_eq!(
        response["result"]["agentCapabilities"]["loadSession"],
        json!(true)
    );
}

#[tokio::test]
async fn session_new_is_rejected_with_guidance() {
    let test = start_server().await;
    let (mut client, _) = WsClient::connect(test.server.local_addr()).await;
    client.initialize().await;
    let (_, response) = client
        .request("session/new", json!({ "cwd": "/tmp", "mcpServers": [] }))
        .await;
    assert_eq!(response["error"]["code"], json!(-32600));
    let message = response["error"]["message"].as_str().expect("message");
    assert!(
        message.contains("session/list"),
        "unexpected message: {message}"
    );
}

#[tokio::test]
async fn list_load_prompt_flow_streams_updates_then_response() {
    let test = start_server().await;
    let (mut client, _) = WsClient::connect(test.server.local_addr()).await;
    client.initialize().await;

    let (_, list) = client.request("session/list", json!({})).await;
    assert_eq!(
        list["result"]["sessions"][0]["sessionId"],
        json!(SESSION_ID)
    );

    let (_, load) = client
        .request(
            "session/load",
            json!({ "sessionId": SESSION_ID, "cwd": "/tmp", "mcpServers": [] }),
        )
        .await;
    assert!(load.get("result").is_some(), "load failed: {load}");

    let (before, response) = client
        .request(
            "session/prompt",
            json!({
                "sessionId": SESSION_ID,
                "prompt": [{ "type": "text", "text": "hi" }],
            }),
        )
        .await;
    assert_eq!(response["result"]["stopReason"], json!("end_turn"));
    let updates: Vec<&Value> = before
        .iter()
        .filter(|message| message["method"] == json!("session/update"))
        .collect();
    assert_eq!(updates.len(), 2);
    assert_eq!(updates[0]["params"]["sessionId"], json!(SESSION_ID));
    assert_eq!(
        updates[0]["params"]["update"]["content"]["text"],
        json!("Hello ")
    );

    let prompts = test.hosted.state.lock().await.prompts.clone();
    assert_eq!(prompts.len(), 1);
}

#[tokio::test]
async fn session_load_replays_history_before_the_response() {
    let test = start_server().await;
    test.hosted.state.lock().await.replay = vec![
        FakeHosted::update("old user turn"),
        FakeHosted::update("old answer"),
    ];

    let (mut client, _) = WsClient::connect(test.server.local_addr()).await;
    client.initialize().await;
    let (before, response) = client
        .request(
            "session/load",
            json!({ "sessionId": SESSION_ID, "cwd": "/tmp", "mcpServers": [] }),
        )
        .await;

    assert!(response.get("result").is_some(), "load failed: {response}");
    let texts: Vec<&Value> = before
        .iter()
        .filter(|message| message["method"] == json!("session/update"))
        .map(|message| &message["params"]["update"]["content"]["text"])
        .collect();
    assert_eq!(texts, vec![&json!("old user turn"), &json!("old answer")]);
}

#[tokio::test]
async fn unknown_session_load_is_an_error() {
    let test = start_server().await;
    let (mut client, _) = WsClient::connect(test.server.local_addr()).await;
    client.initialize().await;
    let (_, response) = client
        .request(
            "session/load",
            json!({ "sessionId": "nope", "cwd": "/tmp", "mcpServers": [] }),
        )
        .await;
    assert_eq!(response["error"]["code"], json!(-32002));
}

#[tokio::test]
async fn last_connect_wins_closes_the_previous_connection() {
    let test = start_server().await;
    let (mut client_a, _) = WsClient::connect(test.server.local_addr()).await;
    client_a.initialize().await;

    let (mut client_b, _) = WsClient::connect(test.server.local_addr()).await;
    // Force the replacement to be observable before asserting on A: B must
    // reach the server task, which happens by the time initialize returns.
    let response_b = client_b.initialize().await;
    assert_eq!(
        response_b["result"]["agentCapabilities"]["loadSession"],
        json!(true)
    );

    match client_a.next_frame().await {
        None | Some(Message::Close(_)) => {}
        other => panic!("expected the replaced connection to close, got: {other:?}"),
    }
}

#[tokio::test]
async fn session_ended_event_closes_the_connection() {
    let test = start_server().await;
    let (mut client, _) = WsClient::connect(test.server.local_addr()).await;
    client.initialize().await;

    test.hosted
        .push(SessionEvent::Nori(NoriEvent::SessionEnded(SessionEnded {
            reason: SessionEndReason::Shutdown,
            message: None,
        })))
        .await;

    loop {
        match client.next_frame().await {
            None | Some(Message::Close(_)) => break,
            Some(Message::Text(_) | Message::Ping(_) | Message::Pong(_)) => {}
            Some(other) => panic!("unexpected frame: {other:?}"),
        }
    }
}

#[tokio::test]
async fn delegated_permission_request_round_trips_to_the_controller() {
    let test = start_server().await;
    let (mut client, _) = WsClient::connect(test.server.local_addr()).await;
    client.initialize().await;

    let permission = acp::RequestPermissionRequest::new(
        FakeHosted::session_id(),
        acp::ToolCallUpdate::new("tool-1", acp::ToolCallUpdateFields::default()),
        vec![acp::PermissionOption::new(
            "allow",
            "Allow",
            acp::PermissionOptionKind::AllowOnce,
        )],
    );
    test.hosted
        .push(SessionEvent::Acp(AcpEvent::Request {
            request_id: acp::RequestId::Number(7),
            request: acp::AgentRequest::RequestPermissionRequest(permission),
        }))
        .await;

    let request = client.next_json().await;
    assert_eq!(request["method"], json!("session/request_permission"));
    let wire_id = request["id"].clone();
    client
        .send_json(json!({
            "jsonrpc": "2.0",
            "id": wire_id,
            "result": { "outcome": { "outcome": "selected", "optionId": "allow" } },
        }))
        .await;

    let deadline = tokio::time::Instant::now() + RECV_TIMEOUT;
    loop {
        let responds = test.hosted.state.lock().await.responds.clone();
        if !responds.is_empty() {
            assert_eq!(responds[0].0, acp::RequestId::Number(7));
            assert!(responds[0].1.is_ok());
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "hosted.respond was never called"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn disconnect_detaches_the_subscription() {
    let test = start_server().await;
    let (mut client, _) = WsClient::connect(test.server.local_addr()).await;
    client.initialize().await;
    client.ws.close(None).await.expect("close");

    let deadline = tokio::time::Instant::now() + RECV_TIMEOUT;
    loop {
        let detaches = test.hosted.state.lock().await.detaches.clone();
        if detaches == vec![1] {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "detach was never called; saw {detaches:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[test]
fn parse_bind_addr_defaults_to_loopback_and_gates_nonloopback() {
    assert_eq!(
        parse_bind_addr("7137", false).expect("bare port"),
        "127.0.0.1:7137".parse::<SocketAddr>().expect("addr")
    );
    assert_eq!(
        parse_bind_addr("127.0.0.1:0", false).expect("loopback addr"),
        "127.0.0.1:0".parse::<SocketAddr>().expect("addr")
    );
    assert!(parse_bind_addr("0.0.0.0:7137", false).is_err());
    assert_eq!(
        parse_bind_addr("0.0.0.0:7137", true).expect("opted in"),
        "0.0.0.0:7137".parse::<SocketAddr>().expect("addr")
    );
    assert!(parse_bind_addr("not-an-addr", false).is_err());
}

#[tokio::test]
async fn malformed_initialize_params_close_the_connection() {
    let test = start_server().await;
    let (mut client, _) = WsClient::connect(test.server.local_addr()).await;
    client
        .send_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "bogus": true },
        }))
        .await;

    match client.next_frame().await {
        Some(Message::Close(Some(frame))) => {
            assert_eq!(frame.code, CloseCode::Protocol);
        }
        other => panic!("expected protocol-error close, got: {other:?}"),
    }
}
