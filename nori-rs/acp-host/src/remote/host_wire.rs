//! Authenticated managed-only WebSocket endpoint for a background host.

use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

use axum::extract::State;
use axum::extract::WebSocketUpgrade;
use axum::extract::ws::Message;
use axum::extract::ws::WebSocket;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use futures::StreamExt;
use serde_json::Value;
use serde_json::json;
use tokio::sync::Mutex;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use super::host_control::HostControl;
use super::host_control::HostRequest;
use super::host_control::failure;
use super::managed::ManagedAgent;
use super::managed::ManagedSubscription;
use super::managed::managed_event;
use super::managed_wire::send_frame;

struct HostState<M> {
    host: Arc<M>,
    lanes: super::standard_wire::Lanes,
    token: Arc<str>,
    instance: Arc<str>,
    control: Arc<Mutex<HostControl>>,
    generation: Arc<AtomicI64>,
    shutdown: CancellationToken,
}

impl<M> Clone for HostState<M> {
    fn clone(&self) -> Self {
        Self {
            host: self.host.clone(),
            lanes: self.lanes.clone(),
            token: self.token.clone(),
            instance: self.instance.clone(),
            control: self.control.clone(),
            generation: self.generation.clone(),
            shutdown: self.shutdown.clone(),
        }
    }
}

pub(super) fn router<M: ManagedAgent>(
    host: Arc<M>,
    token: String,
    instance: String,
    control: Arc<Mutex<HostControl>>,
    shutdown: CancellationToken,
) -> axum::Router {
    let mut router = axum::Router::new().route("/sessions", axum::routing::any(route::<M, false>));
    if host.standard_acp_enabled() {
        router = router.route("/managed", axum::routing::any(route::<M, true>));
    }
    router.with_state(HostState {
        host,
        lanes: Default::default(),
        token: token.into(),
        instance: instance.into(),
        control,
        generation: Arc::new(AtomicI64::new(0)),
        shutdown,
    })
}

async fn route<M: ManagedAgent, const STANDARD: bool>(
    State(state): State<HostState<M>>,
    headers: HeaderMap,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    let supplied = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer "));
    let authorized = supplied.is_some_and(|supplied| {
        supplied.len() == state.token.len()
            && supplied
                .bytes()
                .zip(state.token.bytes())
                .fold(0, |difference, (left, right)| difference | (left ^ right))
                == 0
    });
    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            "A valid host bearer token is required.",
        )
            .into_response();
    }
    let Ok(upgrade) = upgrade else {
        return (
            StatusCode::UPGRADE_REQUIRED,
            "Use a WebSocket connection for managed sessions.",
        )
            .into_response();
    };
    let instance = state.instance.to_string();
    let response = upgrade
        .on_upgrade(move |socket| async move {
            let cancel = state.shutdown.child_token();
            if STANDARD {
                tokio::select! {
                    _ = cancel.cancelled() => {},
                    result = super::standard_wire::serve(socket, state.host, state.lanes, state.shutdown) => {
                        if let Err(error) = result {
                            tracing::debug!(%error, "standard ACP machine connection ended");
                        }
                    }
                }
                return;
            }
            let client = state.generation.fetch_add(1, Ordering::Relaxed);
            tokio::select! {
                _ = cancel.cancelled() => {},
                result = serve(socket, &state, client, cancel.clone()) => {
                    if let Err(error) = result {
                        tracing::debug!(%error, "managed host connection ended");
                    }
                }
            }
            state.control.lock().await.remove(client);
        })
        .into_response();
    ([("x-nori-host-instance", instance)], response).into_response()
}

/// Outbound messages one connection may have queued before it counts as
/// too slow and is closed.
const OUTBOUND_CAPACITY: usize = 4096;

async fn serve<M: ManagedAgent>(
    socket: WebSocket,
    state: &HostState<M>,
    client: i64,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let (mut sink, mut incoming) = socket.split();
    let mut subscription: Option<ManagedSubscription> = None;
    let mut initialized = false;
    let (changes, mut roles) = watch::channel("watcher");
    let (outbound, mut queued) = tokio::sync::mpsc::channel::<Value>(OUTBOUND_CAPACITY);
    let result = async {
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                changed = roles.changed(), if initialized => {
                    changed?;
                    let role = *roles.borrow_and_update();
                    send_frame(&mut sink, Message::Text(json!({"jsonrpc":"2.0","method":"_nori/control/changed","params":{"role":role}}).to_string().into())).await?;
                }
                message = queued.recv() => {
                    let Some(message) = message else { break; };
                    send_frame(&mut sink, Message::Text(message.to_string().into())).await?;
                }
                frame = incoming.next() => {
                    let Some(frame) = frame else { break; };
                    match frame? {
                        Message::Text(text) => {
                            let request: Value = match serde_json::from_str(&text) {
                                Ok(request) => request,
                                Err(_) => {
                                    send_frame(&mut sink, Message::Text(failure(Value::Null, -32700, "invalid JSON").response.to_string().into())).await?;
                                    continue;
                                }
                            };
                            let id = request.get("id").cloned().unwrap_or(Value::Null);
                            let id_valid = id.is_string() || id.is_i64() || id.is_null();
                            // Notifications and answers to the host's own
                            // requests carry no reply.
                            if initialized
                                && request["jsonrpc"] == "2.0"
                                && ((request["method"].is_string() && request.get("id").is_none())
                                    || (request.get("method").is_none()
                                        && id_valid
                                        && (request.get("result").is_some() || request.get("error").is_some())))
                            {
                                state.host.message_from(client, request);
                                continue;
                            }
                            let valid = request["jsonrpc"] == "2.0"
                                && request["method"].is_string()
                                && request.get("id").is_some()
                                && id_valid
                                && request.get("params").is_none_or(Value::is_object);
                            if !valid {
                                send_frame(&mut sink, Message::Text(failure(id, -32600, "expected a JSON-RPC request with an id and object params").response.to_string().into())).await?;
                                continue;
                            }
                            if !initialized && request["method"] != "initialize" {
                                send_frame(&mut sink, Message::Text(failure(id, -32001, "initialize must be the first request").response.to_string().into())).await?;
                                break;
                            }
                            let reply = if request["method"] == "initialize" {
                                let observe = request["params"]["observe"] == true
                                    || request["params"]["_meta"]["nori.dev/host"]["observe"] == true;
                                let mut reply = state.host.request_from(client, request).await;
                                if reply.response.get("error").is_none() {
                                    reply.response["result"]["control"] = state.control.lock().await.register(client, &changes, observe);
                                    if !initialized {
                                        state.host.connection_opened(super::managed::HostConnection {
                                            client,
                                            outbound: outbound.clone(),
                                            close: cancel.clone(),
                                            role: roles.clone(),
                                        });
                                    }
                                }
                                reply
                            } else {
                                let admitted = state.control.lock().await.admit(client, &changes, request);
                                match admitted {
                                    HostRequest::Reply(reply) => reply,
                                    HostRequest::Forward(request) => {
                                        let method = request["method"].as_str().unwrap_or_default();
                                        if state.host.runs_concurrently(method) {
                                            let host = state.host.clone();
                                            let outbound = outbound.clone();
                                            tokio::spawn(async move {
                                                let reply = host.request_from(client, request).await;
                                                let _ = outbound.send(reply.response).await;
                                            });
                                            continue;
                                        }
                                        state.host.request_from(client, request).await
                                    }
                                }
                            };
                            initialized |= reply.response.get("error").is_none();
                            send_frame(&mut sink, Message::Text(reply.response.to_string().into())).await?;
                            drop(reply.delivery_guard);
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
        anyhow::Ok(())
    }
    .await;
    if initialized {
        state.host.connection_closed(client);
    }
    let _ = send_frame(&mut sink, Message::Close(None)).await;
    result
}
