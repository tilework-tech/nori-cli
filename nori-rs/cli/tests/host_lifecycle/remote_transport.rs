//! The host's ACP remote transport: the WebSocket profile of the ACP
//! "Streamable HTTP & WebSocket Transport" RFD at `/acp`, with `/sessions`
//! as an alias, bearer authentication, a client limit, and sessions that
//! outlive the connections that use them.

use std::time::Duration;

use futures::SinkExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use super::acp_client::Acp;
use super::acp_client::mentions;
use super::acp_v1::controller;
use super::acp_v1::host_with;
use super::acp_v1::new_session;
use super::acp_v1::text;
use super::*;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn discovery(host: &Host) -> Value {
    serde_json::from_str(&std::fs::read_to_string(host.0.path().join("host/host.json")).unwrap())
        .unwrap()
}

/// The host's endpoint with its path replaced by `path`.
fn at_path(host: &Host, path: &str) -> String {
    let endpoint = discovery(host)["endpoint"].as_str().unwrap().to_owned();
    let (origin, _) = endpoint.rsplit_once('/').unwrap();
    format!("{origin}{path}")
}

/// Open a WebSocket to `url`, sending `token` as a bearer token if given.
async fn upgrade(
    url: &str,
    token: Option<&str>,
) -> Result<(Socket, tungstenite::handshake::client::Response), tungstenite::Error> {
    let mut request = url.into_client_request().unwrap();
    if let Some(token) = token {
        request
            .headers_mut()
            .insert("Authorization", format!("Bearer {token}").parse().unwrap());
    }
    tokio_tungstenite::connect_async(request).await
}

fn token(host: &Host) -> String {
    discovery(host)["token"].as_str().unwrap().to_owned()
}

fn refusal(
    result: Result<(Socket, tungstenite::handshake::client::Response), tungstenite::Error>,
) -> (u16, String) {
    match result {
        Err(tungstenite::Error::Http(response)) => (
            response.status().as_u16(),
            String::from_utf8_lossy(response.body().as_deref().unwrap_or_default()).into_owned(),
        ),
        Err(error) => panic!("expected an HTTP refusal, got {error}"),
        Ok(_) => panic!("expected an HTTP refusal, the upgrade succeeded"),
    }
}

/// Send a WebSocket upgrade request over plain HTTP and read the whole
/// response, for refusals whose body must be read in full.
async fn raw_upgrade(url: &str, token: Option<&str>) -> (u16, String) {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    let rest = url.strip_prefix("ws://").unwrap();
    let (authority, path) = rest.split_at(rest.find('/').unwrap());
    let mut stream = tokio::net::TcpStream::connect(authority).await.unwrap();
    let authorization = token
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    stream
        .write_all(
            format!(
                "GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n{authorization}\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    let mut buffer = [0; 4096];
    let read = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let count = stream.read(&mut buffer).await.unwrap();
            response.extend_from_slice(&buffer[..count]);
            let text = String::from_utf8_lossy(&response).into_owned();
            if let Some((head, body)) = text.split_once("\r\n\r\n") {
                let length = head
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())?
                    })
                    .unwrap_or(0);
                if body.len() >= length || count == 0 {
                    return text;
                }
            } else if count == 0 {
                return text;
            }
        }
    })
    .await
    .expect("the host answers the upgrade");
    let (head, body) = read.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, body.to_owned())
}

/// The next response on `socket`, skipping notifications and requests.
async fn next_reply(socket: &mut Socket) -> Value {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(30), socket.next())
            .await
            .expect("the host answers")
            .expect("the socket stays open")
            .unwrap();
        if let tungstenite::Message::Text(text) = frame {
            let message: Value = serde_json::from_str(text.as_str()).unwrap();
            if message.get("method").is_none() {
                return message;
            }
        }
    }
}

async fn send_json(socket: &mut Socket, message: Value) {
    socket
        .send(tungstenite::Message::Text(message.to_string().into()))
        .await
        .unwrap();
}

fn connect_at(url: &str, token: &str, observe: bool) -> Acp {
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let mut client = nori_harness::remote_agent::ManagedClient::connect(url, token)
                .await
                .unwrap();
            client
                .request(
                    "initialize",
                    json!({"protocolVersion": 1, "observe": observe}),
                )
                .await
                .unwrap();
            Acp::new(client)
        })
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn the_acp_path_and_the_sessions_alias_reach_the_same_sessions() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let endpoint = discovery(&host)["endpoint"].as_str().unwrap().to_owned();
    let state: Value = serde_json::from_str(
        &std::fs::read_to_string(host.0.path().join("host/state.json")).unwrap(),
    )
    .unwrap();
    let mut on_acp = connect_at(&at_path(&host, "/acp"), &token(&host), false);
    let (id, _) = new_session(&mut on_acp, folder.path()).await;
    on_acp
        .call(
            "session/prompt",
            json!({"sessionId": id, "prompt": text("hello over acp")}),
        )
        .await
        .unwrap();

    let mut on_alias = connect_at(&at_path(&host, "/sessions"), &token(&host), true);
    let listing = on_alias.call("session/list", json!({})).await.unwrap();
    on_alias
        .call(
            "session/load",
            json!({"sessionId": id, "cwd": canonical(folder.path()), "mcpServers": []}),
        )
        .await
        .unwrap();

    assert!(endpoint.ends_with("/acp"), "{endpoint}");
    assert_eq!(state["endpoint"], endpoint.as_str());
    assert!(
        listing["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["sessionId"] == id.as_str()),
        "{listing}"
    );
    assert!(
        mentions(&on_alias.inbox, &id, "hello over acp"),
        "{:?}",
        on_alias.inbox
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn every_connection_is_named_in_its_upgrade_response() {
    let host = host_with("", &[]);
    let url = at_path(&host, "/acp");

    let (_first, first) = upgrade(&url, Some(&token(&host))).await.unwrap();
    let (_second, second) = upgrade(&url, Some(&token(&host))).await.unwrap();

    let id = |response: &tungstenite::handshake::client::Response| {
        response
            .headers()
            .get("Acp-Connection-Id")
            .unwrap_or_else(|| panic!("Acp-Connection-Id is sent: {response:?}"))
            .to_str()
            .unwrap()
            .to_owned()
    };
    let (_alias, on_alias) = upgrade(&at_path(&host, "/sessions"), Some(&token(&host)))
        .await
        .unwrap();
    let ids = [id(&first), id(&second), id(&on_alias)];
    assert_ne!(ids[0], ids[1]);
    assert_ne!(ids[1], ids[2]);
    assert_ne!(ids[0], ids[2]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_or_wrong_token_is_refused_before_the_upgrade() {
    let host = host_with("", &[]);

    for path in ["/acp", "/sessions"] {
        let url = at_path(&host, path);
        let (missing, _) = refusal(upgrade(&url, None).await);
        let (wrong, _) = refusal(upgrade(&url, Some("not-the-token")).await);
        assert_eq!((missing, wrong), (401, 401), "{path}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn binary_frames_are_ignored_and_batches_are_refused() {
    let host = host_with("", &[]);
    let (mut socket, _) = upgrade(&at_path(&host, "/acp"), Some(&token(&host)))
        .await
        .unwrap();

    socket
        .send(tungstenite::Message::Binary(vec![1, 2, 3].into()))
        .await
        .unwrap();
    send_json(
        &mut socket,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}),
    )
    .await;
    let initialized = next_reply(&mut socket).await;
    send_json(
        &mut socket,
        json!([{"jsonrpc":"2.0","id":2,"method":"session/list","params":{}}]),
    )
    .await;
    let batch = next_reply(&mut socket).await;
    send_json(
        &mut socket,
        json!({"jsonrpc":"2.0","id":3,"method":"session/list","params":{}}),
    )
    .await;
    let listed = next_reply(&mut socket).await;

    assert_eq!(initialized["id"], 1, "{initialized}");
    assert_eq!(initialized["result"]["protocolVersion"], 1, "{initialized}");
    assert_eq!(batch["error"]["code"], -32600, "{batch}");
    assert_eq!(batch["id"], Value::Null, "{batch}");
    assert!(listed["result"]["sessions"].is_array(), "{listed}");
}

#[tokio::test(flavor = "multi_thread")]
async fn clients_beyond_the_limit_are_refused_with_a_clear_error() {
    let host = host_with("[host]\nmax_clients = 2", &[]);
    let url = at_path(&host, "/acp");
    let token = token(&host);
    let first = upgrade(&url, Some(&token)).await.unwrap();
    let _second = upgrade(&at_path(&host, "/sessions"), Some(&token))
        .await
        .unwrap();

    let (status, message) = raw_upgrade(&url, Some(&token)).await;
    let (unauthenticated, _) = raw_upgrade(&url, None).await;
    drop(first);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match upgrade(&url, Some(&token)).await {
            Ok(_) => break,
            Err(tungstenite::Error::Http(response))
                if response.status() == 503 && Instant::now() < deadline =>
            {
                tokio::time::sleep(Duration::from_millis(50)).await
            }
            Err(error) => panic!("the freed slot was not reused: {error}"),
        }
    }

    assert_eq!(status, 503);
    assert!(
        message.contains("2 clients") && message.contains("max_clients"),
        "{message}"
    );
    assert_eq!(unauthenticated, 401, "authentication is checked first");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_full_host_still_reports_itself_running_and_can_be_stopped() {
    let host = host_with("[host]\nmax_clients = 1", &[]);
    let _only = upgrade(&at_path(&host, "/acp"), Some(&token(&host)))
        .await
        .unwrap();

    let started = tokio::task::block_in_place(|| host.run(&["host", "--port", "0"]));
    let stopped = tokio::task::block_in_place(|| host.run(&["host", "stop"]));

    assert!(started.status.success(), "{started:?}");
    assert!(
        String::from_utf8_lossy(&started.stdout).contains("already running"),
        "{started:?}"
    );
    assert!(stopped.status.success(), "{stopped:?}");
    assert!(!host.0.path().join("host/host.json").exists());
}

/// Text of the message chunks for `session` among `frames`, leaving out
/// session titles and other metadata.
fn chunk_text(frames: &[Value], session: &str) -> String {
    frames
        .iter()
        .filter(|frame| {
            frame["method"] == "session/update"
                && frame["params"]["sessionId"] == session
                && matches!(
                    frame["params"]["update"]["sessionUpdate"].as_str(),
                    Some("user_message_chunk" | "agent_message_chunk")
                )
        })
        .map(|frame| frame["params"]["update"].to_string())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_stays_live_after_the_client_that_started_it_disconnects() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut first = controller(&host);
    let (id, _) = new_session(&mut first, folder.path()).await;
    first
        .call(
            "session/prompt",
            json!({"sessionId": id, "prompt": text("before leaving")}),
        )
        .await
        .unwrap();
    drop(first);

    let mut returning = controller(&host);
    returning
        .call("_nori/control/take", json!({}))
        .await
        .unwrap();
    let listing = returning.call("session/list", json!({})).await.unwrap();
    returning
        .call(
            "session/resume",
            json!({"sessionId": id, "cwd": canonical(folder.path())}),
        )
        .await
        .unwrap();
    let ended = returning
        .call(
            "session/prompt",
            json!({"sessionId": id, "prompt": text("after returning")}),
        )
        .await
        .unwrap();

    let row = listing["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["sessionId"] == id.as_str())
        .unwrap_or_else(|| panic!("the session is still listed: {listing}"))
        .clone();
    assert_eq!(row["_meta"]["nori/session"], json!({"live": true}), "{row}");
    assert_eq!(ended["stopReason"], "end_turn");
}

#[tokio::test(flavor = "multi_thread")]
async fn resuming_a_live_session_after_a_reconnect_does_not_replay_its_history() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut first = controller(&host);
    let (id, _) = new_session(&mut first, folder.path()).await;
    first
        .call(
            "session/prompt",
            json!({"sessionId": id, "prompt": text("earlier turn")}),
        )
        .await
        .unwrap();
    drop(first);

    let mut again = controller(&host);
    again.call("_nori/control/take", json!({})).await.unwrap();
    again
        .call(
            "session/resume",
            json!({"sessionId": id, "cwd": canonical(folder.path())}),
        )
        .await
        .unwrap();
    again
        .call(
            "session/prompt",
            json!({"sessionId": id, "prompt": text("later turn")}),
        )
        .await
        .unwrap();
    again.collect_for(Duration::from_millis(300)).await;
    let seen = chunk_text(&again.log, &id);

    assert!(seen.contains("later turn"), "{seen}");
    assert!(
        !seen.contains("earlier turn"),
        "resume does not replay: {seen}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn lifecycle_connection_at_capacity_is_limited_to_host_management() {
    let host = host_with("[host]\nmax_clients = 1", &[]);
    let endpoint = at_path(&host, "/acp");
    let _full = upgrade(&endpoint, Some(&token(&host))).await.unwrap();
    let mut admin =
        nori_harness::remote_agent::ManagedClient::connect_lifecycle(&endpoint, &token(&host))
            .await
            .unwrap();
    let initialized = admin
        .request("initialize", json!({"observe":true}))
        .await
        .unwrap();
    assert_eq!(initialized["control"]["role"], "watcher");
    assert!(
        admin
            .request("_nori/session/list", json!({}))
            .await
            .is_err()
    );
}

#[test]
fn stop_refuses_a_discovery_record_for_a_different_instance() {
    let host = Host::new();
    success(host.run(&["host", "--port", "0"]));
    let path = host.0.path().join("host/host.json");
    let original = std::fs::read(&path).unwrap();
    let mut other: Value = serde_json::from_slice(&original).unwrap();
    other["instanceId"] = json!(uuid::Uuid::new_v4().to_string());
    std::fs::write(&path, other.to_string()).unwrap();
    let stopped = host.run(&["host", "stop"]);
    std::fs::write(path, original).unwrap();
    assert!(!stopped.status.success(), "{stopped:?}");
    success(host.run(&["host", "ls"]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_completes_for_its_instance_while_a_replacement_serves_the_same_home() {
    let host = host_with("[host]\nmax_clients = 1", &[]);
    let first = discovery(&host);
    let mut observer = tokio::task::block_in_place(|| host.observer());
    let mut stopping = tokio::process::Command::from(host.command(&["host", "stop"]))
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The reserved stop connection works even with the only session slot held.
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Ok(Some(_)) = observer.recv_text().await {}
    })
    .await
    .expect("stop never disconnected the existing observer");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let result = host.run(&["host", "--port", "0"]);
        if result.status.success() && discovery(&host)["instanceId"] != first["instanceId"] {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "replacement never started: {result:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let replacement = discovery(&host);
    assert_ne!(first["instanceId"], replacement["instanceId"]);
    let stopped = tokio::time::timeout(Duration::from_secs(5), stopping.wait())
        .await
        .expect("stop waited for the replacement")
        .unwrap();
    assert!(stopped.success());
    success(host.run(&["host", "ls"]));
}
