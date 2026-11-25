//! Authenticated WebSocket endpoints for a background host: `/acp`, the
//! WebSocket profile of the ACP remote transport, and its alias `/sessions`.

use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

use axum::extract::State;
use axum::extract::WebSocketUpgrade;
use axum::extract::ws::Message;
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
    clients: Arc<AtomicI64>,
    max_clients: i64,
    lifecycle_clients: Arc<AtomicI64>,
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
            clients: self.clients.clone(),
            max_clients: self.max_clients,
            lifecycle_clients: self.lifecycle_clients.clone(),
            shutdown: self.shutdown.clone(),
        }
    }
}

pub(super) fn router<M: ManagedAgent>(
    host: Arc<M>,
    token: String,
    instance: String,
    control: Arc<Mutex<HostControl>>,
    max_clients: i32,
    shutdown: CancellationToken,
) -> axum::Router {
    let mut router = axum::Router::new().route("/sessions", axum::routing::any(route::<M, false>));
    if host.standard_acp_enabled() {
        router = router.route("/managed", axum::routing::any(route::<M, true>));
    } else {
        router = router.route("/acp", axum::routing::any(route::<M, false>));
    }
    router.with_state(HostState {
        host,
        lanes: Default::default(),
        token: token.into(),
        instance: instance.into(),
        control,
        generation: Arc::new(AtomicI64::new(0)),
        lifecycle_clients: Default::default(),
        clients: Arc::new(AtomicI64::new(0)),
        max_clients: i64::from(max_clients),
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
            [(axum::http::header::UPGRADE, "websocket")],
            "This host serves ACP over WebSocket only; open a WebSocket connection.",
        )
            .into_response();
    };
    // The legacy managed-mode wire is not limited.
    let lifecycle = !STANDARD
        && headers
            .get("x-nori-lifecycle")
            .is_some_and(|value| value == "1");
    let slot = if lifecycle {
        let Some(slot) = super::client_limit::ClientSlot::claim(&state.lifecycle_clients, 16)
        else {
            return (StatusCode::SERVICE_UNAVAILABLE, "Host management is busy.").into_response();
        };
        Some(slot)
    } else if STANDARD || state.host.standard_acp_enabled() {
        None
    } else {
        let Some(slot) = super::client_limit::ClientSlot::claim(&state.clients, state.max_clients)
        else {
            let limit = state.max_clients;
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                format!(
                    "This nori host already serves {limit} clients, its limit. Close another client, or raise [host] max_clients in config.toml and restart the host."
                ),
            )
                .into_response();
        };
        Some(slot)
    };
    let instance = state.instance.to_string();
    let connection = uuid::Uuid::new_v4().to_string();
    let response = upgrade
        .on_upgrade(move |socket| async move {
            let _slot = slot;
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
            let transport = if lifecycle { Transport::Lifecycle } else { Transport::WebSocket };
            let (sink, incoming) = socket.split();
            tokio::select! {
                _ = cancel.cancelled() => {},
                _ = tokio::time::sleep(std::time::Duration::from_secs(5)), if lifecycle => {},
                result = serve_frames(sink, incoming, &state, client, cancel.clone(), transport) => {
                    if let Err(error) = result {
                        tracing::debug!(%error, "managed host connection ended");
                    }
                }
            }
            state.control.lock().await.remove(client);
        })
        .into_response();
    (
        [
            ("x-nori-host-instance", instance),
            ("acp-connection-id", connection),
        ],
        response,
    )
        .into_response()
}

/// Outbound messages one connection may have queued before it counts as
/// too slow and is closed.
const OUTBOUND_CAPACITY: usize = 16384;

/// How a connection's transport differs from a WebSocket's.
#[derive(Clone, Copy)]
pub(super) enum Transport {
    WebSocket,
    Lifecycle,
    /// The host's own stdin and stdout: its one client always controls, and
    /// an ended subscription is reported in band, as the stream cannot close
    /// and reopen.
    Lines(super::HostClientOrigin),
}

/// Lines of this process's stdin, without their newlines, until it closes.
pub(super) fn stdin_lines() -> tokio::sync::mpsc::Receiver<std::io::Result<Vec<u8>>> {
    // A pending read on Tokio's stdin keeps the runtime from shutting down,
    // so lines are read on a plain thread that ends with the process.
    let (lines, input) = tokio::sync::mpsc::channel::<std::io::Result<Vec<u8>>>(64);
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::stdin().lock().split(b'\n') {
            if lines.blocking_send(line).is_err() {
                break;
            }
        }
    });
    input
}

/// Serve one host client whose requests are `lines` and whose replies go to
/// stdout, until `lines` ends or `stop` completes. A stop lets the reply
/// already being written finish and returns the stop's value.
pub(super) async fn serve_stdio<M: ManagedAgent, T>(
    host: Arc<M>,
    instance: String,
    lines: impl futures::Stream<Item = std::io::Result<Vec<u8>>> + Send,
    stop: impl std::future::Future<Output = T>,
) -> anyhow::Result<Option<T>> {
    let incoming = lines.filter_map(|line| async {
        match line.map(String::from_utf8) {
            Ok(Ok(line)) => Some(Ok(line)),
            // Not UTF-8, so not JSON either: a replacement character earns
            // the same parse error as any other line that is not JSON.
            Ok(Err(_)) => Some(Ok(char::REPLACEMENT_CHARACTER.to_string())),
            Err(error) => Some(Err(error)),
        }
    });
    let sink = futures::sink::unfold(
        tokio::io::stdout(),
        |mut output, frame: Message| async move {
            use tokio::io::AsyncWriteExt;
            if let Message::Text(line) = frame {
                output
                    .write_all(format!("{}\n", line.as_str()).as_bytes())
                    .await?;
                output.flush().await?;
            }
            Ok::<_, std::io::Error>(output)
        },
    );
    serve_lines(
        host,
        instance,
        super::HostClientOrigin::Independent,
        incoming,
        sink,
        stop,
    )
    .await
}

/// Serve one in-process host client over channels of JSON-RPC lines until
/// `input` closes or `stop` completes.
pub(super) async fn serve_pipe<M: ManagedAgent, T>(
    host: Arc<M>,
    instance: String,
    origin: super::HostClientOrigin,
    input: tokio::sync::mpsc::Receiver<String>,
    output: tokio::sync::mpsc::Sender<String>,
    stop: impl std::future::Future<Output = T>,
) -> anyhow::Result<Option<T>> {
    let incoming = tokio_stream::wrappers::ReceiverStream::new(input).map(Ok::<_, std::io::Error>);
    let sink = futures::sink::unfold(output, |output, frame: Message| async move {
        if let Message::Text(line) = frame {
            output
                .send(line.as_str().to_owned())
                .await
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
        }
        Ok::<_, std::io::Error>(output)
    });
    serve_lines(host, instance, origin, incoming, sink, stop).await
}

/// The client of a line transport always controls; blank lines are skipped
/// and only text frames are written.
async fn serve_lines<M: ManagedAgent, T>(
    host: Arc<M>,
    instance: String,
    origin: super::HostClientOrigin,
    incoming: impl futures::Stream<Item = std::io::Result<String>> + Send,
    sink: impl futures::Sink<Message, Error = std::io::Error> + Send,
    stop: impl std::future::Future<Output = T>,
) -> anyhow::Result<Option<T>> {
    let shutdown = CancellationToken::new();
    let state = HostState {
        host,
        lanes: Default::default(),
        token: "".into(),
        instance: instance.into(),
        control: Default::default(),
        generation: Arc::new(AtomicI64::new(0)),
        lifecycle_clients: Default::default(),
        clients: Arc::new(AtomicI64::new(0)),
        max_clients: 1,
        shutdown: shutdown.clone(),
    };
    let incoming = incoming.filter_map(|line| async {
        match line {
            Ok(line) if line.trim().is_empty() => None,
            Ok(line) => Some(Ok::<_, axum::Error>(Message::Text(line.into()))),
            Err(error) => Some(Err(axum::Error::new(error))),
        }
    });
    let sink = Box::pin(sink);
    let cancel = shutdown.child_token();
    let serving = serve_frames(
        sink,
        Box::pin(incoming),
        &state,
        0,
        cancel.clone(),
        Transport::Lines(origin),
    );
    tokio::pin!(serving);
    let stopped = tokio::select! {
        served = &mut serving => {
            served?;
            None
        }
        stopped = stop => {
            shutdown.cancel();
            tokio::time::timeout(std::time::Duration::from_secs(5), serving)
                .await
                .map_err(|_| anyhow::anyhow!("the client stopped reading"))??;
            Some(stopped)
        }
    };
    // The host closes a client that falls too far behind; a line client
    // cannot reconnect, so that ends the host with an error.
    anyhow::ensure!(
        stopped.is_some() || !cancel.is_cancelled(),
        "the client fell too far behind and was disconnected"
    );
    Ok(stopped)
}

/// Releases the host's per-client resources even when the transport drops
/// the serving future while evicting a slow reader.
struct RegisteredClient<'a, M: ManagedAgent>(&'a M, i64);

impl<M: ManagedAgent> Drop for RegisteredClient<'_, M> {
    fn drop(&mut self) {
        self.0.connection_closed(self.1);
    }
}

async fn serve_frames<M, S, I, E>(
    mut sink: S,
    mut incoming: I,
    state: &HostState<M>,
    client: i64,
    cancel: CancellationToken,
    transport: Transport,
) -> anyhow::Result<()>
where
    M: ManagedAgent,
    S: futures::Sink<Message> + Unpin + Send,
    S::Error: std::error::Error + Send + Sync + 'static,
    I: futures::Stream<Item = Result<Message, E>> + Unpin + Send,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut subscription: Option<ManagedSubscription> = None;
    let mut initialized = false;
    let mut approval_modes = false;
    let mut registration = None;
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
                            if matches!(transport, Transport::Lifecycle)
                                && !(request["jsonrpc"] == "2.0" && request.get("id").is_some()
                                    && matches!(request["method"].as_str(), Some("initialize" | "_nori/host/stop")))
                            {
                                send_frame(&mut sink, Message::Text(failure(id, -32601, "host management connections only support initialize and stop").response.to_string().into())).await?;
                                break;
                            }
                            let id_valid = id.is_string() || id.is_i64() || id.is_null();
                            // Notifications and answers to the host's own
                            // requests carry no reply.
                            if initialized
                                && state.host.accepts_client_messages()
                                && request["jsonrpc"] == "2.0"
                                && ((request["method"].is_string() && request.get("id").is_none())
                                    || (request.get("method").is_none()
                                        && id_valid
                                        && (request.get("result").is_some() || request.get("error").is_some())))
                            {
                                if request["method"] == "session/cancel" {
                                    // Admit the session-scoped notification before reading the next
                                    // frame. A later prompt or control handoff may change its target.
                                    state.host.message_from(client, request).await;
                                } else {
                                    let host = state.host.clone();
                                    tokio::spawn(async move { host.message_from(client, request).await });
                                }
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
                                let observe = match transport {
                                    Transport::WebSocket => request["params"]["observe"] == true,
                                    Transport::Lifecycle => true,
                                    Transport::Lines(_) => false,
                                };
                                let declares_approval_modes = request["params"]["capabilities"]["nori_sessions"]["approval_modes"] == true;
                                let mut reply = state.host.request_from(client, request).await;
                                if reply.response.get("error").is_none() {
                                    reply.response["result"]["control"] = state.control.lock().await.register(client, &changes, observe);
                                    // The client learns its first role from this reply,
                                    // not from a change notification.
                                    roles.borrow_and_update();
                                    if !initialized {
                                        approval_modes = declares_approval_modes;
                                        registration = Some(RegisteredClient(state.host.as_ref(), client));
                                        state.host.connection_opened(super::managed::HostConnection {
                                            client,
                                            origin: match transport {
                                                Transport::WebSocket | Transport::Lifecycle => super::HostClientOrigin::Independent,
                                                Transport::Lines(origin) => origin,
                                            },
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
                                        if matches!(transport, Transport::Lines(super::HostClientOrigin::Foreground))
                                            && let Some(receipt) = state.host.admit_foreground_input(&request)
                                        {
                                            let outbound = outbound.clone();
                                            tokio::spawn(async move {
                                                let _ = outbound.send(receipt.await).await;
                                            });
                                            continue;
                                        }
                                        if state.host.request_runs_concurrently(&request) {
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
                                    send_frame(&mut sink, Message::Text(managed_event(&envelope, approval_modes).to_string().into())).await?;
                                }
                                subscription = Some(next);
                            }
                        }
                        Message::Ping(bytes) => send_frame(&mut sink, Message::Pong(bytes)).await?,
                        Message::Close(_) => break,
                        Message::Pong(_) => {},
                        // The ACP remote transport ignores binary frames.
                        Message::Binary(_) => {},
                    }
                }
                event = async {
                    match subscription.as_mut() {
                        Some(subscription) => subscription.events.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    let Some(envelope) = event else {
                        match transport {
                            Transport::WebSocket | Transport::Lifecycle => break,
                            Transport::Lines(_) => {
                                subscription = None;
                                send_frame(&mut sink, Message::Text(json!({"jsonrpc":"2.0","method":"_nori/session/subscription_ended","params":{"reason":"replay_required"}}).to_string().into())).await?;
                                continue;
                            }
                        }
                    };
                    send_frame(&mut sink, Message::Text(managed_event(&envelope, approval_modes).to_string().into())).await?;
                }
            }
        }
        anyhow::Ok(())
    }
    .await;
    drop(registration);
    let _ = send_frame(&mut sink, Message::Close(None)).await;
    result
}
