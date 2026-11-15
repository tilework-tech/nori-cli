//! Authenticated host ownership through real WebSocket connections.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use futures::SinkExt;
use futures::StreamExt;
use nori_acp_host::remote::ManagedAgent;
use nori_acp_host::remote::ManagedReply;
use nori_acp_host::remote::RemoteAcpServer;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Default)]
struct Host {
    mutations: AtomicI64,
}

impl ManagedAgent for Host {
    async fn request(&self, request: Value) -> ManagedReply {
        let result = match request["method"].as_str().unwrap() {
            "initialize" => json!({"capabilities":{}}),
            "_nori/session/list" => json!({"mutations":self.mutations.load(Ordering::SeqCst)}),
            _ => json!({"mutations":self.mutations.fetch_add(1, Ordering::SeqCst) + 1}),
        };
        ManagedReply {
            response: json!({"jsonrpc":"2.0","id":request["id"],"result":result}),
            subscription: None,
        }
    }
}

async fn start() -> RemoteAcpServer {
    RemoteAcpServer::bind_host(
        "127.0.0.1:0".parse().unwrap(),
        Arc::new(Host::default()),
        "secret".into(),
    )
    .await
    .unwrap()
}

async fn connect(
    server: &RemoteAcpServer,
    token: Option<&str>,
    path: &str,
) -> anyhow::Result<Socket> {
    let mut request = format!("ws://{}{path}", server.local_addr()).into_client_request()?;
    if let Some(token) = token {
        request
            .headers_mut()
            .insert("Authorization", format!("Bearer {token}").parse()?);
    }
    Ok(connect_async(request).await?.0)
}

async fn send(socket: &mut Socket, id: i64, method: &str, params: Value) {
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
}

async fn receive(socket: &mut Socket) -> Value {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match socket.next().await.unwrap().unwrap() {
                Message::Text(text) => return serde_json::from_str(&text).unwrap(),
                Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await.unwrap(),
                other => panic!("unexpected frame {other:?}"),
            }
        }
    })
    .await
    .unwrap()
}

async fn initialized(server: &RemoteAcpServer, observe: bool) -> (Socket, Value) {
    let mut socket = connect(server, Some("secret"), "/sessions").await.unwrap();
    send(&mut socket, 1, "initialize", json!({"observe":observe})).await;
    let response = receive(&mut socket).await;
    (socket, response)
}

#[tokio::test]
async fn loopback_requires_token_and_host_does_not_expose_an_acp_bypass() {
    let server = start().await;
    assert!(connect(&server, None, "/sessions").await.is_err());
    assert!(connect(&server, Some("wrong"), "/sessions").await.is_err());
    assert!(connect(&server, Some("secret"), "/acp").await.is_err());
    assert!(connect(&server, Some("secret"), "/sessions").await.is_ok());
    server.shutdown().await;
    assert!(
        RemoteAcpServer::bind_host(
            "127.0.0.1:0".parse().unwrap(),
            Arc::new(Host::default()),
            String::new()
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn watcher_cannot_mutate_and_takeover_notifies_both_without_disconnecting() {
    let server = start().await;
    let (mut first, response) = initialized(&server, false).await;
    assert_eq!(response["result"]["control"]["role"], "controller");
    let (mut second, response) = initialized(&server, false).await;
    assert_eq!(response["result"]["control"]["role"], "watcher");
    for method in [
        "_nori/session/prompt",
        "_nori/session/respond",
        "_nori/session/new",
        "_nori/session/close",
        "_nori/session/cancel",
        "_nori/session/resume",
        "_nori/session/delegate",
        "_nori/session/delegation/resolve",
    ] {
        send(&mut second, 2, method, json!({})).await;
        let response = receive(&mut second).await;
        assert_eq!(response["error"]["code"], -32016);
        assert!(
            response["error"]["message"]
                .as_str()
                .unwrap()
                .contains("watcher")
        );
    }
    send(&mut first, 3, "_nori/session/list", json!({})).await;
    assert_eq!(receive(&mut first).await["result"]["mutations"], 0);
    send(&mut second, 4, "_nori/control/take", json!({})).await;
    assert_eq!(receive(&mut second).await["result"]["role"], "controller");
    assert_eq!(receive(&mut second).await["params"]["role"], "controller");
    let changed = receive(&mut first).await;
    assert_eq!(changed["method"], "_nori/control/changed");
    assert_eq!(changed["params"]["role"], "watcher");
    send(&mut first, 5, "_nori/session/prompt", json!({})).await;
    assert_eq!(receive(&mut first).await["error"]["code"], -32016);
    send(&mut second, 6, "_nori/session/prompt", json!({})).await;
    assert_eq!(receive(&mut second).await["result"]["mutations"], 1);
    send(&mut first, 7, "_nori/host/stop", json!({})).await;
    assert!(receive(&mut first).await.get("result").is_some());
    server.shutdown().await;
}

#[tokio::test]
async fn observer_does_not_claim_control_and_disconnect_allows_a_new_controller() {
    let server = start().await;
    let (mut observer, response) = initialized(&server, true).await;
    assert_eq!(response["result"]["control"]["role"], "watcher");
    let (mut controller, response) = initialized(&server, false).await;
    assert_eq!(response["result"]["control"]["role"], "controller");
    controller.close(None).await.unwrap();
    // A status round trip observes the controller's departure without relying on a sleep.
    for _ in 0..100 {
        send(&mut observer, 2, "_nori/control/status", json!({})).await;
        if receive(&mut observer).await["result"]["controller_connected"] == false {
            break;
        }
        tokio::task::yield_now().await;
    }
    let (_next, response) = initialized(&server, false).await;
    assert_eq!(response["result"]["control"]["role"], "controller");
    server.shutdown().await;
}

#[tokio::test]
async fn client_round_trips_requests_and_reports_watcher_errors() {
    use nori_acp_host::remote::ManagedClient;
    let server = start().await;
    let url = format!("ws://{}/sessions", server.local_addr());
    assert!(ManagedClient::connect(&url, "wrong").await.is_err());
    let mut client = ManagedClient::connect(&url, "secret").await.unwrap();
    assert_eq!(
        client.request("initialize", json!({})).await.unwrap()["control"]["role"],
        "controller"
    );
    let (mut other, _) = initialized(&server, false).await;
    send(&mut other, 2, "_nori/control/take", json!({})).await;
    assert_eq!(receive(&mut other).await["result"]["role"], "controller");
    assert_eq!(client.recv().await.unwrap()["params"]["role"], "watcher");
    assert!(
        client
            .request("_nori/session/prompt", json!({}))
            .await
            .unwrap_err()
            .to_string()
            .contains("watcher")
    );
    client
        .send(json!({"jsonrpc":"2.0","id":20,"method":"_nori/session/list","params":{}}))
        .await
        .unwrap();
    assert_eq!(client.recv().await.unwrap()["result"]["mutations"], 0);
    server.shutdown().await;
}

#[derive(Default)]
struct BlockingHost {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

impl ManagedAgent for BlockingHost {
    async fn request(&self, request: Value) -> ManagedReply {
        if request["params"]["hold"] == true {
            self.entered.notify_one();
            self.release.notified().await;
        }
        ManagedReply {
            response: json!({"jsonrpc":"2.0","id":request["id"],"result":{"method":request["method"]}}),
            subscription: None,
        }
    }
}

async fn blocking_server() -> (RemoteAcpServer, Arc<BlockingHost>) {
    let host = Arc::new(BlockingHost::default());
    let server = RemoteAcpServer::bind_host(
        "127.0.0.1:0".parse().unwrap(),
        host.clone(),
        "secret".into(),
    )
    .await
    .unwrap();
    (server, host)
}

#[tokio::test]
async fn takeover_does_not_wait_for_admitted_work_and_refuses_later_old_owner_requests() {
    let (server, host) = blocking_server().await;
    let (mut controller, _) = initialized(&server, false).await;
    let (mut watcher, _) = initialized(&server, false).await;
    send(
        &mut controller,
        2,
        "_nori/session/prompt",
        json!({"hold":true}),
    )
    .await;
    tokio::time::timeout(Duration::from_secs(3), host.entered.notified())
        .await
        .unwrap();
    send(&mut watcher, 3, "_nori/control/take", json!({})).await;
    assert_eq!(receive(&mut watcher).await["result"]["role"], "controller");
    assert_eq!(receive(&mut watcher).await["params"]["role"], "controller");
    send(&mut watcher, 4, "_nori/session/prompt", json!({})).await;
    assert_eq!(
        receive(&mut watcher).await["result"]["method"],
        "_nori/session/prompt"
    );

    // This request arrives after takeover while its sender's earlier work is
    // still pending. Admission must use its current role when processing it.
    send(&mut controller, 5, "_nori/session/prompt", json!({})).await;
    host.release.notify_one();
    let admitted = receive(&mut controller).await;
    assert_eq!(admitted["id"], 2);
    assert_eq!(admitted["result"]["method"], "_nori/session/prompt");
    assert_eq!(receive(&mut controller).await["params"]["role"], "watcher");
    let rejected = receive(&mut controller).await;
    assert_eq!(rejected["id"], 5);
    assert_eq!(rejected["error"]["code"], -32016);
    server.shutdown().await;
}

#[tokio::test]
async fn blocked_agent_work_does_not_prevent_observer_initialize_list_or_host_stop() {
    let (server, host) = blocking_server().await;
    let (mut controller, _) = initialized(&server, false).await;
    send(
        &mut controller,
        2,
        "_nori/session/prompt",
        json!({"hold":true}),
    )
    .await;
    tokio::time::timeout(Duration::from_secs(3), host.entered.notified())
        .await
        .unwrap();

    let (mut observer, reply) = initialized(&server, true).await;
    assert_eq!(reply["result"]["control"]["role"], "watcher");
    for (id, method) in [(3, "_nori/session/list"), (4, "_nori/host/stop")] {
        send(&mut observer, id, method, json!({})).await;
        let reply = receive(&mut observer).await;
        assert_eq!(reply["id"], id);
        assert_eq!(reply["result"]["method"], method);
    }
    host.release.notify_one();
    assert_eq!(receive(&mut controller).await["id"], 2);
    server.shutdown().await;
}

#[tokio::test]
async fn slow_initialize_does_not_reserve_control_or_block_other_clients() {
    let (server, host) = blocking_server().await;
    let mut slow = connect(&server, Some("secret"), "/sessions").await.unwrap();
    send(&mut slow, 1, "initialize", json!({"hold":true})).await;
    tokio::time::timeout(Duration::from_secs(3), host.entered.notified())
        .await
        .unwrap();

    let (mut first_ready, reply) = initialized(&server, false).await;
    assert_eq!(reply["result"]["control"]["role"], "controller");
    send(&mut first_ready, 2, "_nori/session/list", json!({})).await;
    assert_eq!(
        receive(&mut first_ready).await["result"]["method"],
        "_nori/session/list"
    );
    host.release.notify_one();
    assert_eq!(
        receive(&mut slow).await["result"]["control"]["role"],
        "watcher"
    );
    server.shutdown().await;
}

#[tokio::test]
async fn host_identity_is_stable_across_connections_but_not_host_lifetimes() {
    let first = start().await;
    let second = start().await;
    let mut identities = Vec::new();
    for server in [&first, &first, &second] {
        let mut request = format!("ws://{}/sessions", server.local_addr())
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("Authorization", "Bearer secret".parse().unwrap());
        let (mut socket, response) = connect_async(request).await.unwrap();
        let identity = response
            .headers()
            .get("x-nori-host-instance")
            .expect("authenticated connection must identify its host lifetime")
            .to_str()
            .unwrap()
            .to_owned();
        assert!(!identity.is_empty());
        identities.push(identity);
        socket.close(None).await.unwrap();
    }
    assert_eq!(identities[0], identities[1]);
    assert_ne!(
        identities[0], identities[2],
        "same token must not imply the same host"
    );
    first.shutdown().await;
    second.shutdown().await;
}

#[tokio::test]
async fn missing_or_malformed_host_identity_is_unavailable_for_safe_replay() {
    use nori_acp_host::remote::ManagedClient;
    use tokio_tungstenite::tungstenite::handshake::server::Request;
    use tokio_tungstenite::tungstenite::handshake::server::Response;
    for identity in [None, Some("not-a-host-uuid")] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}/sessions", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            tokio_tungstenite::accept_hdr_async(
                stream,
                move |_: &Request, mut response: Response| {
                    if let Some(identity) = identity {
                        response
                            .headers_mut()
                            .insert("x-nori-host-instance", identity.parse().unwrap());
                    }
                    Ok(response)
                },
            )
            .await
            .unwrap()
        });
        let connection = ManagedClient::connect(&endpoint, "token").await;
        let _socket = server.await.unwrap();
        assert_eq!(connection.unwrap().host_instance(), None);
    }
}
