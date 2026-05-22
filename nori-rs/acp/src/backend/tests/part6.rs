use super::*;
use crate::broker::CloudConnectionInfo;
use futures::SinkExt;
use futures::StreamExt;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

struct MockWsServer {
    port: i32,
    _task: tokio::task::JoinHandle<()>,
}

async fn start_mock_acp_ws_server() -> MockWsServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind to random port");
    let port = listener.local_addr().expect("get local addr").port() as i32;

    let task =
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let ws_stream = tokio_tungstenite::accept_async(stream)
                    .await
                    .expect("ws handshake");
                let (mut write, mut read) = ws_stream.split();

                tokio::spawn(async move {
                    while let Some(Ok(msg)) = read.next().await {
                        if let Message::Text(text) = msg {
                            let text_str: &str = &text;
                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(text_str)
                            {
                                if parsed.get("method").and_then(|m| m.as_str())
                                    == Some("initialize")
                                {
                                    let id = parsed.get("id").cloned().unwrap_or(
                                        serde_json::Value::Number(serde_json::Number::from(0)),
                                    );
                                    let response = serde_json::json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "result": {
                                            "protocolVersion": 1,
                                            "agentCapabilities": {},
                                            "agentInfo": {
                                                "name": "mock-cloud-agent",
                                                "version": "0.1.0",
                                                "title": "Mock Cloud Agent"
                                            }
                                        }
                                    });
                                    let _ = write
                                        .send(Message::Text(
                                            serde_json::to_string(&response)
                                                .expect("serialize response")
                                                .into(),
                                        ))
                                        .await;
                                } else if parsed.get("method").and_then(|m| m.as_str())
                                    == Some("session/new")
                                {
                                    let id = parsed.get("id").cloned().unwrap_or(
                                        serde_json::Value::Number(serde_json::Number::from(0)),
                                    );
                                    let response = serde_json::json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "result": {
                                            "sessionId": "cloud-session-1"
                                        }
                                    });
                                    let _ = write
                                        .send(Message::Text(
                                            serde_json::to_string(&response)
                                                .expect("serialize response")
                                                .into(),
                                        ))
                                        .await;
                                }
                            }
                        }
                    }
                });
            }
        });

    MockWsServer { port, _task: task }
}

#[tokio::test]
async fn cloud_spawn_connects_and_produces_session_configured() {
    let server = start_mock_acp_ws_server().await;
    let temp_dir = tempfile::tempdir().unwrap();

    let mut config = build_test_config(temp_dir.path());
    config.agent = "cloud".to_string();
    config.cloud_connection = Some(CloudConnectionInfo {
        ws_url: format!("ws://127.0.0.1:{}", server.port),
        auth_token: "test-token".to_string(),
    });

    let (backend_event_tx, mut backend_event_rx) = tokio::sync::mpsc::channel(32);
    let backend = AcpBackend::spawn(&config, backend_event_tx).await;

    assert!(backend.is_ok(), "cloud spawn should succeed");
    let _backend = backend.unwrap();

    let event = tokio::time::timeout(std::time::Duration::from_secs(5), backend_event_rx.recv())
        .await
        .expect("should receive event within 5s")
        .expect("event channel should not be closed");

    match event {
        BackendEvent::Control(event) => {
            assert!(
                matches!(
                    event.msg,
                    codex_core::protocol::EventMsg::SessionConfigured(_)
                ),
                "expected SessionConfigured event, got: {:?}",
                event.msg
            );
        }
        other => panic!("expected Control(SessionConfigured), got: {other:?}"),
    }
}

#[tokio::test]
async fn cloud_spawn_fails_with_unreachable_url() {
    let temp_dir = tempfile::tempdir().unwrap();

    let mut config = build_test_config(temp_dir.path());
    config.agent = "cloud".to_string();
    config.cloud_connection = Some(CloudConnectionInfo {
        ws_url: "ws://127.0.0.1:1".to_string(),
        auth_token: "test-token".to_string(),
    });

    let (backend_event_tx, _backend_event_rx) = tokio::sync::mpsc::channel(32);
    let result = AcpBackend::spawn(&config, backend_event_tx).await;

    assert!(
        result.is_err(),
        "cloud spawn should fail with unreachable URL"
    );
    let err = result.err().unwrap().to_string();
    assert!(
        err.contains("Cloud connection failed"),
        "error should mention cloud connection failure, got: {err}"
    );
}

async fn start_mock_acp_ws_server_that_disconnects_after_session() -> MockWsServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind to random port");
    let port = listener.local_addr().expect("get local addr").port() as i32;

    let task =
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let ws_stream = tokio_tungstenite::accept_async(stream)
                    .await
                    .expect("ws handshake");
                let (mut write, mut read) = ws_stream.split();

                tokio::spawn(async move {
                    while let Some(Ok(msg)) = read.next().await {
                        if let Message::Text(text) = msg {
                            let text_str: &str = &text;
                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(text_str)
                            {
                                if parsed.get("method").and_then(|m| m.as_str())
                                    == Some("initialize")
                                {
                                    let id = parsed.get("id").cloned().unwrap_or(
                                        serde_json::Value::Number(serde_json::Number::from(0)),
                                    );
                                    let response = serde_json::json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "result": {
                                            "protocolVersion": 1,
                                            "agentCapabilities": {},
                                            "agentInfo": {
                                                "name": "mock-cloud-agent",
                                                "version": "0.1.0",
                                                "title": "Mock Cloud Agent"
                                            }
                                        }
                                    });
                                    let _ = write
                                        .send(Message::Text(
                                            serde_json::to_string(&response)
                                                .expect("serialize response")
                                                .into(),
                                        ))
                                        .await;
                                } else if parsed.get("method").and_then(|m| m.as_str())
                                    == Some("session/new")
                                {
                                    let id = parsed.get("id").cloned().unwrap_or(
                                        serde_json::Value::Number(serde_json::Number::from(0)),
                                    );
                                    let response = serde_json::json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "result": {
                                            "sessionId": "cloud-session-disconnect"
                                        }
                                    });
                                    let _ = write
                                        .send(Message::Text(
                                            serde_json::to_string(&response)
                                                .expect("serialize response")
                                                .into(),
                                        ))
                                        .await;

                                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                                    drop(write);
                                    return;
                                }
                            }
                        }
                    }
                });
            }
        });

    MockWsServer { port, _task: task }
}

#[tokio::test]
async fn cloud_disconnect_emits_error_event() {
    let server = start_mock_acp_ws_server_that_disconnects_after_session().await;
    let temp_dir = tempfile::tempdir().unwrap();

    let mut config = build_test_config(temp_dir.path());
    config.agent = "cloud".to_string();
    config.cloud_connection = Some(CloudConnectionInfo {
        ws_url: format!("ws://127.0.0.1:{}", server.port),
        auth_token: "test-token".to_string(),
    });

    let (backend_event_tx, mut backend_event_rx) = tokio::sync::mpsc::channel(32);
    let _backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("cloud spawn should succeed");

    let timeout = std::time::Duration::from_secs(5);
    let mut found_disconnect_error = false;

    while let Ok(Some(event)) = tokio::time::timeout(timeout, backend_event_rx.recv()).await {
        if let BackendEvent::Control(event) = &event
            && let codex_core::protocol::EventMsg::Error(err) = &event.msg
            && err.message.contains("Cloud session disconnected")
        {
            found_disconnect_error = true;
            break;
        }
    }

    assert!(
        found_disconnect_error,
        "expected a 'Cloud session disconnected' error event after WS close"
    );
}
