//! WebSocket adapter for Nori's managed-session commands and envelopes.

use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

use axum::extract::State;
use axum::extract::WebSocketUpgrade;
use axum::extract::ws::Message;
use axum::extract::ws::WebSocket;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use futures::SinkExt;
use futures::StreamExt;
use serde_json::Value;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::managed::ManagedAgent;
use super::managed::ManagedSubscription;
use super::managed::managed_event;
use super::server::ActiveConnection;

struct ManagedState<M> {
    host: Arc<M>,
    active: ActiveConnection,
    generation: Arc<AtomicI64>,
    shutdown: CancellationToken,
}

impl<M> Clone for ManagedState<M> {
    fn clone(&self) -> Self {
        Self {
            host: self.host.clone(),
            active: self.active.clone(),
            generation: self.generation.clone(),
            shutdown: self.shutdown.clone(),
        }
    }
}

pub(super) fn router<M: ManagedAgent>(
    host: Arc<M>,
    active: ActiveConnection,
    generation: Arc<AtomicI64>,
    shutdown: CancellationToken,
) -> axum::Router {
    axum::Router::new()
        .route("/sessions", axum::routing::any(route::<M>))
        .with_state(ManagedState {
            host,
            active,
            generation,
            shutdown,
        })
}

async fn route<M: ManagedAgent>(
    State(state): State<ManagedState<M>>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    let Ok(upgrade) = upgrade else {
        return (
            StatusCode::UPGRADE_REQUIRED,
            "Use a WebSocket connection for managed sessions.",
        )
            .into_response();
    };
    upgrade.on_upgrade(move |socket| async move {
        let cancel = state.shutdown.child_token();
        let generation = state.generation.fetch_add(1, Ordering::Relaxed);
        {
            let mut active = state.active.lock().await;
            if cancel.is_cancelled() { return; }
            if let Some((_, previous)) = active.replace((generation, cancel.clone())) { previous.cancel(); }
        }
        tokio::select! {
            _ = cancel.cancelled() => {},
            result = serve(socket, state.host, cancel.clone()) => {
                if let Err(error) = result { tracing::debug!(%error, "managed session connection ended"); }
            }
        }
        let mut active = state.active.lock().await;
        if active.as_ref().is_some_and(|(current, _)| *current == generation) { *active = None; }
    }).into_response()
}

async fn serve<M: ManagedAgent>(
    socket: WebSocket,
    host: Arc<M>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let (mut sink, mut incoming) = socket.split();
    let mut subscription: Option<ManagedSubscription> = None;
    let mut initialized = false;
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break,
            frame = incoming.next() => {
                let Some(frame) = frame else { break; };
                match frame? {
                    Message::Text(text) => {
                        let request: Value = match serde_json::from_str(&text) {
                            Ok(request) => request,
                            Err(_) => {
                                send_frame(&mut sink, Message::Text(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"invalid JSON"}}).to_string().into())).await?;
                                continue;
                            }
                        };
                        if !initialized && request["method"] != "initialize" {
                            send_frame(&mut sink, Message::Text(json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32001,"message":"initialize must be the first request"}}).to_string().into())).await?;
                            break;
                        }
                        let reply = host.request(request).await;
                        initialized |= reply.response.get("error").is_none();
                        send_frame(&mut sink, Message::Text(reply.response.to_string().into())).await?;
                        if let Some(mut next) = reply.subscription {
                            for envelope in next.replay.drain(..) {
                                send_frame(&mut sink, Message::Text(managed_event(&envelope).to_string().into())).await?;
                            }
                            subscription = Some(next);
                        }
                    }
                    Message::Ping(bytes) => send_frame(&mut sink, Message::Pong(bytes)).await?,
                    Message::Close(_) => break,
                    Message::Pong(_) => {},
                    Message::Binary(_) => anyhow::bail!("managed sessions require text JSON frames"),
                }
            }
            event = async {
                match subscription.as_mut() {
                    Some(subscription) => subscription.events.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                let Some(envelope) = event else { break; };
                send_frame(&mut sink, Message::Text(managed_event(&envelope).to_string().into())).await?;
            }
        }
    }
    let _ = send_frame(&mut sink, Message::Close(None)).await;
    Ok(())
}

pub(super) async fn send_frame(
    sink: &mut futures::stream::SplitSink<WebSocket, Message>,
    frame: Message,
) -> anyhow::Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(30), sink.send(frame))
        .await
        .map_err(|_| anyhow::anyhow!("managed remote consumer stalled"))??;
    Ok(())
}
