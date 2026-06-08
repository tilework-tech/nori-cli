//! Test-only support fixtures shared across the crate's unit tests.

use futures::SinkExt;
use futures::StreamExt;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// A mock ACP-over-WebSocket server bound to a random loopback port.
///
/// The background accept task is owned by `_task` and aborted on drop.
pub struct MockWsServer {
    pub port: i32,
    _task: tokio::task::JoinHandle<()>,
}

/// Spawn a mock ACP server that answers `initialize` and `session/new` over a
/// WebSocket. It always reports empty agent capabilities and the fixed session
/// id `test-session-1`.
///
/// When `disconnect_after_session` is `true`, the server drops the socket
/// shortly after replying to `session/new`, simulating a mid-session cloud
/// disconnect so the relay's disconnect-detection path can be exercised.
pub async fn start_mock_acp_ws_server(disconnect_after_session: bool) -> MockWsServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind to random port");
    let port = listener.local_addr().expect("get local addr").port() as i32;

    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let ws_stream = tokio_tungstenite::accept_async(stream)
                .await
                .expect("ws handshake");
            let (mut write, mut read) = ws_stream.split();

            tokio::spawn(async move {
                while let Some(Ok(Message::Text(text))) = read.next().await {
                    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) else {
                        continue;
                    };
                    let method = parsed.get("method").and_then(serde_json::Value::as_str);
                    let id = parsed
                        .get("id")
                        .cloned()
                        .unwrap_or_else(|| serde_json::Value::Number(serde_json::Number::from(0)));

                    let result = match method {
                        Some("initialize") => serde_json::json!({
                            "protocolVersion": 1,
                            "agentCapabilities": {},
                            "agentInfo": {
                                "name": "mock-acp-agent",
                                "version": "0.1.0",
                                "title": "Mock ACP Agent"
                            }
                        }),
                        Some("session/new") => serde_json::json!({
                            "sessionId": "test-session-1"
                        }),
                        _ => continue,
                    };

                    let response = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": result,
                    });
                    let _ = write
                        .send(Message::Text(
                            serde_json::to_string(&response)
                                .expect("serialize response")
                                .into(),
                        ))
                        .await;

                    if disconnect_after_session && method == Some("session/new") {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        drop(write);
                        return;
                    }
                }
            });
        }
    });

    MockWsServer { port, _task: task }
}
