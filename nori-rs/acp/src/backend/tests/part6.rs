use super::*;
use crate::broker::CloudConnectionInfo;
use crate::test_support::start_mock_acp_ws_server;

#[tokio::test]
async fn cloud_spawn_connects_and_produces_session_configured() {
    use pretty_assertions::assert_eq;

    let server = start_mock_acp_ws_server(false).await;
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
    let mut found_configured = None;

    while let Ok(Some(event)) = tokio::time::timeout(timeout, backend_event_rx.recv()).await {
        if let BackendEvent::Control(event) = &event
            && let codex_core::protocol::EventMsg::SessionConfigured(configured) = &event.msg
        {
            found_configured = Some(configured.clone());
            break;
        }
    }

    let configured = found_configured.expect("expected SessionConfigured event");
    assert_eq!(
        configured.cwd,
        temp_dir.path().to_path_buf(),
        "cloud session should use config cwd"
    );
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

#[tokio::test]
async fn cloud_disconnect_emits_error_event() {
    let server = start_mock_acp_ws_server(true).await;
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
