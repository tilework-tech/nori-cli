//! An in-process host connection over channels of JSON-RPC lines.

use serde_json::Value;
use tokio::sync::mpsc;

use super::ConnectionEvent;

/// Drives one pipe to a host the caller owns. There is no reconnect: when
/// the host closes its end the connection stops for good.
pub(crate) async fn drive_pipe(
    to_host: mpsc::Sender<String>,
    mut from_host: mpsc::Receiver<String>,
    mut requests: mpsc::UnboundedReceiver<Value>,
    events: mpsc::UnboundedSender<ConnectionEvent>,
) {
    const STOPPED: &str = "The host this terminal owns stopped.";
    if events.send(ConnectionEvent::Connected).is_err() {
        return;
    }
    loop {
        tokio::select! {
            request = requests.recv() => {
                let Some(request) = request else { return; };
                if to_host.send(request.to_string()).await.is_err() {
                    let _ = events.send(ConnectionEvent::Stopped(STOPPED.into()));
                    return;
                }
            }
            line = from_host.recv() => {
                let Some(line) = line else {
                    let _ = events.send(ConnectionEvent::Stopped(STOPPED.into()));
                    return;
                };
                match serde_json::from_str(&line) {
                    Ok(message) => {
                        if events.send(ConnectionEvent::Message(message)).is_err() {
                            return;
                        }
                    }
                    Err(error) => tracing::warn!("skipping a host line that is not JSON: {error}"),
                }
            }
        }
    }
}
