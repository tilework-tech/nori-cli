use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use nori_harness::remote_agent::ManagedAgent;
use nori_harness::remote_agent::ManagedClient;
use nori_harness::remote_agent::ManagedReply;
use nori_harness::remote_agent::RemoteAcpServer;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;

use super::AttachOptions;
use super::connection::ConnectionEvent;
use super::connection::drive;

struct EmptyHost;
impl ManagedAgent for EmptyHost {
    async fn request(&self, request: Value) -> ManagedReply {
        ManagedReply {
            delivery_guard: None,
            response: json!({"jsonrpc":"2.0", "id":request["id"], "result":{}}),
            subscription: None,
        }
    }
}

async fn replacement_failure(
    local: bool,
    valid_refresh: Option<bool>,
    rotate_token: bool,
) -> (String, usize) {
    let original = RemoteAcpServer::bind_host(
        "127.0.0.1:0".parse().unwrap(),
        Arc::new(EmptyHost),
        "old".into(),
    )
    .await
    .unwrap();
    let initial =
        ManagedClient::connect(&format!("ws://{}/sessions", original.local_addr()), "old")
            .await
            .unwrap();
    let replacement = RemoteAcpServer::bind_host(
        "127.0.0.1:0".parse().unwrap(),
        Arc::new(EmptyHost),
        if rotate_token { "new" } else { "old" }.into(),
    )
    .await
    .unwrap();
    let endpoint = format!("ws://{}/sessions", replacement.local_addr());
    let reads = Arc::new(AtomicUsize::new(0));
    let counter = reads.clone();
    let options = AttachOptions {
        endpoint,
        token: "old".into(),
        session_id: None,
        refresh_local_token: local.then(|| {
            Box::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                match valid_refresh {
                    Some(true) => Ok("new".into()),
                    Some(false) => Ok("invalid".into()),
                    None => Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied).into()),
                }
            }) as Box<dyn Fn() -> anyhow::Result<String> + Send + Sync>
        }),
    };
    let (_requests, request_rx) = mpsc::unbounded_channel();
    let (events, mut event_rx) = mpsc::unbounded_channel();
    let driver = tokio::spawn(drive(options, initial, request_rx, events));
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(3), event_rx.recv()).await,
        Ok(Some(ConnectionEvent::Connected))
    ));
    original.shutdown().await;
    let failure = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match event_rx.recv().await {
                Some(ConnectionEvent::Stopped(reason)) => break Ok(reason),
                Some(ConnectionEvent::Connected) => {
                    break Err("replacement must not initialize or replay");
                }
                None => break Err("driver must report a terminal failure"),
                Some(ConnectionEvent::Disconnected(_) | ConnectionEvent::Message(_)) => {}
            }
        }
    })
    .await;
    driver.abort();
    let _ = driver.await;
    replacement.shutdown().await;
    (
        failure
            .expect("reconnect must stop")
            .expect("terminal failure"),
        reads.load(Ordering::SeqCst),
    )
}

#[tokio::test]
async fn reconnect_refreshes_local_token_once_then_detects_restarted_host() {
    let (reason, reads) = replacement_failure(true, Some(true), true).await;
    assert_eq!(reads, 1);
    assert!(
        reason.contains("Host restarted") && reason.contains("sessions were lost"),
        "{reason}"
    );
}

#[tokio::test]
async fn reconnect_stops_after_refreshed_token_is_rejected() {
    let (reason, reads) = replacement_failure(true, Some(false), true).await;
    assert_eq!(reads, 1);
    assert!(
        reason.contains("Authentication failed") && reason.contains("nori attach"),
        "{reason}"
    );
}

#[tokio::test]
async fn remote_reconnect_auth_failure_stops_without_local_discovery() {
    let (reason, reads) = replacement_failure(false, Some(false), true).await;
    assert_eq!(reads, 0);
    assert!(reason.contains("Authentication failed"), "{reason}");
}

#[tokio::test]
async fn reconnect_rejects_a_different_host_even_with_the_same_token() {
    let (reason, reads) = replacement_failure(true, Some(true), false).await;
    assert_eq!(reads, 0);
    assert!(reason.contains("Host restarted"), "{reason}");
}

#[tokio::test]
async fn reconnect_stops_when_local_token_cannot_be_read() {
    let (reason, reads) = replacement_failure(true, None, true).await;
    assert_eq!(reads, 1);
    assert!(
        reason.contains("Authentication failed") && reason.contains("nori attach"),
        "{reason}"
    );
}
