//! Socket ownership and reconnect scheduling, independent of terminal rendering.

use nori_harness::remote_agent::ManagedClient;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;

// Session-list replies arrive every second, including when no session is active.
const LIVENESS_TIMEOUT: Duration = Duration::from_secs(10);

use super::HostEndpoint;

pub(crate) enum ConnectionEvent {
    Connected,
    Message(Value),
    Disconnected(String),
    Stopped(String),
}

/// Requests are never replayed. Only event subscriptions resume from a cursor.
pub(crate) async fn drive(
    mut options: HostEndpoint,
    initial: ManagedClient,
    mut requests: mpsc::UnboundedReceiver<Value>,
    events: mpsc::UnboundedSender<ConnectionEvent>,
) {
    let instance = initial.host_instance();
    let mut client = initial;
    loop {
        if events.send(ConnectionEvent::Connected).is_err() {
            return;
        }
        let mut deadline = Instant::now() + LIVENESS_TIMEOUT;
        let error = loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {
                    break "Host stopped responding".to_string();
                }
                request = requests.recv() => {
                    let Some(request) = request else { return; };
                    match tokio::time::timeout_at(deadline, client.send(request)).await {
                        Ok(Ok(())) => {},
                        Ok(Err(error)) => break error.to_string(),
                        Err(_) => break "Host stopped responding".to_string(),
                    }
                }
                incoming = client.recv() => {
                    match incoming {
                        Ok(message) => {
                            deadline = Instant::now() + LIVENESS_TIMEOUT;
                            if events.send(ConnectionEvent::Message(message)).is_err() {
                                return;
                            }
                        }
                        Err(error) => break error.to_string(),
                    }
                }
            }
        };
        // Release the failed socket before opening its replacement, including
        // when liveness expired without a TCP error.
        drop(client);
        if events.send(ConnectionEvent::Disconnected(error)).is_err() {
            return;
        }
        // The transport cannot determine whether a sent mutation was applied.
        // Clear queued work before reconnecting, so it cannot run unexpectedly.
        while requests.try_recv().is_ok() {}
        loop {
            tokio::select! {
                _ = events.closed() => return,
                _ = tokio::time::sleep(std::time::Duration::from_millis(750)) => {}
            }
            let attempt = options.connect().await;
            let attempt = match attempt {
                Err(error) if ManagedClient::is_authentication_error(&error) => {
                    let refreshed = options
                        .refresh_local_token
                        .as_ref()
                        .and_then(|read| read().ok());
                    let Some(token) = refreshed else {
                        let _ = events.send(ConnectionEvent::Stopped(
                            "Authentication failed; host may have restarted. Re-run nori attach."
                                .into(),
                        ));
                        return;
                    };
                    options.token = token;
                    // One attempt with freshly read credentials, never an endless auth loop.
                    match options.connect().await {
                        Ok(connected) => Ok(connected),
                        Err(_) => {
                            let _ = events.send(ConnectionEvent::Stopped(
                                "Authentication failed after token refresh; host may have restarted. Re-run nori attach.".into(),
                            ));
                            return;
                        }
                    }
                }
                attempt => attempt,
            };
            if let Ok(connected) = attempt {
                let reason = if instance.is_none() {
                    Some("Host identity unavailable; cannot safely resume. Re-run nori attach.")
                } else if connected.host_instance() != instance {
                    Some("Host restarted; sessions were lost. Re-run nori attach.")
                } else {
                    None
                };
                if let Some(reason) = reason {
                    let _ = events.send(ConnectionEvent::Stopped(reason.into()));
                    return;
                }
                while requests.try_recv().is_ok() {}
                client = connected;
                break;
            }
        }
    }
}
