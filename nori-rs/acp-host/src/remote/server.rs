//! Listener and WebSocket upgrade for the remote ACP transport.
//!
//! One `/acp` endpoint, WebSocket-only (the RFD permits WS-only servers).
//! Plain HTTP requests get `426 Upgrade Required`. Each upgrade response
//! carries a fresh `Acp-Connection-Id`; the server keeps a single live
//! connection, and a newer connection replaces the current one (last connect
//! wins).

use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

use axum::extract::State;
use axum::extract::WebSocketUpgrade;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::http::HeaderValue;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use tokio_util::sync::CancellationToken;

use super::connection;
use super::hosted_agent::HostedAgent;

/// Upgrade response header naming the transport-level connection identity.
const ACP_CONNECTION_ID_HEADER: &str = "Acp-Connection-Id";

/// The live connection slot: a cancellation token per accepted socket, tagged
/// with a generation so a finished connection only clears its own entry.
type ActiveConnection = Arc<tokio::sync::Mutex<Option<(i64, CancellationToken)>>>;

struct RemoteState<H> {
    hosted: Arc<H>,
    active: ActiveConnection,
    generation: Arc<AtomicI64>,
    shutdown: CancellationToken,
}

impl<H> Clone for RemoteState<H> {
    fn clone(&self) -> Self {
        Self {
            hosted: self.hosted.clone(),
            active: self.active.clone(),
            generation: self.generation.clone(),
            shutdown: self.shutdown.clone(),
        }
    }
}

/// A running remote ACP listener.
///
/// Dropping the server stops accepting new connections; the current
/// connection, if any, is closed by [`RemoteAcpServer::shutdown`].
pub struct RemoteAcpServer {
    local_addrs: Vec<SocketAddr>,
    serve_tasks: Vec<tokio::task::JoinHandle<()>>,
    active: ActiveConnection,
    shutdown: CancellationToken,
}

impl RemoteAcpServer {
    /// Bind the listener and start serving `/acp`.
    pub async fn bind<H: HostedAgent>(addr: SocketAddr, hosted: Arc<H>) -> anyhow::Result<Self> {
        Self::bind_many([addr], hosted).await
    }

    /// Bind one ACP surface on multiple exact addresses. A port of zero on
    /// later addresses reuses the port allocated for the first listener.
    pub async fn bind_many<H: HostedAgent>(
        addrs: impl IntoIterator<Item = SocketAddr>,
        hosted: Arc<H>,
    ) -> anyhow::Result<Self> {
        let mut requested = addrs.into_iter();
        let Some(first_addr) = requested.next() else {
            anyhow::bail!("at least one remote ACP listen address is required");
        };

        // Bind every socket before serving any of them. If one bind fails,
        // dropping this vector closes the already-bound sockets atomically.
        let first = tokio::net::TcpListener::bind(first_addr).await?;
        let first_local_addr = first.local_addr()?;
        let shared_port = first_local_addr.port();
        let mut listeners = vec![first];
        let mut local_addrs = vec![first_local_addr];
        for mut addr in requested {
            if addr.port() == 0 {
                addr.set_port(shared_port);
            }
            let listener = tokio::net::TcpListener::bind(addr).await?;
            local_addrs.push(listener.local_addr()?);
            listeners.push(listener);
        }

        let active: ActiveConnection = Arc::new(tokio::sync::Mutex::new(None));
        let shutdown = CancellationToken::new();
        let state = RemoteState {
            hosted,
            active: active.clone(),
            generation: Arc::new(AtomicI64::new(0)),
            shutdown: shutdown.clone(),
        };
        let serve_tasks = listeners
            .into_iter()
            .map(|listener| {
                let router = axum::Router::new()
                    .route("/acp", axum::routing::any(acp_route::<H>))
                    .with_state(state.clone());
                tokio::spawn(async move {
                    if let Err(error) = axum::serve(listener, router).await {
                        tracing::warn!("remote ACP server exited: {error}");
                    }
                })
            })
            .collect();
        Ok(Self {
            local_addrs,
            serve_tasks,
            active,
            shutdown,
        })
    }

    /// Every address on which this server is listening.
    pub fn local_addrs(&self) -> &[SocketAddr] {
        &self.local_addrs
    }

    /// Whether a remote controller currently owns the WebSocket connection.
    pub async fn controller_connected(&self) -> bool {
        self.active.lock().await.is_some()
    }

    /// The address the listener actually bound (resolves port 0).
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addrs[0]
    }

    /// Stop accepting connections and close the current one, if any.
    pub async fn shutdown(mut self) {
        // Cancel first so connections whose HTTP upgrade was accepted but
        // whose callback has not registered yet inherit the stopped state.
        self.shutdown.cancel();
        if let Some((_, cancel)) = self.active.lock().await.take() {
            cancel.cancel();
        }
        for task in self.serve_tasks.drain(..) {
            task.abort();
            let _ = task.await;
        }
    }
}

impl Drop for RemoteAcpServer {
    fn drop(&mut self) {
        self.shutdown.cancel();
        for task in &self.serve_tasks {
            task.abort();
        }
    }
}

/// Parse a `--remote` listen spec: either a bare port (loopback) or a full
/// `IP:PORT`. Non-loopback binds require their own explicit opt-in.
pub fn parse_bind_addr(spec: &str, allow_nonloopback: bool) -> anyhow::Result<SocketAddr> {
    let addr = match spec.parse::<SocketAddr>() {
        Ok(addr) => addr,
        Err(_) => {
            let port = spec
                .parse::<i32>()
                .ok()
                .filter(|port| (0..=65535).contains(port));
            let Some(port) = port else {
                anyhow::bail!("invalid remote listen address '{spec}': expected PORT or IP:PORT");
            };
            #[expect(clippy::cast_sign_loss, reason = "range-checked above")]
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port as u16)
        }
    };
    if !addr.ip().is_loopback() && !allow_nonloopback {
        anyhow::bail!(
            "remote listen address {addr} is not loopback; pass --remote-allow-nonloopback \
             to expose the unauthenticated ACP surface beyond this machine"
        );
    }
    Ok(addr)
}

async fn acp_route<H: HostedAgent>(
    State(state): State<RemoteState<H>>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    let Ok(upgrade) = upgrade else {
        return (
            StatusCode::UPGRADE_REQUIRED,
            "This ACP endpoint is WebSocket-only; connect with a WebSocket upgrade.",
        )
            .into_response();
    };
    let connection_id = uuid::Uuid::new_v4().to_string();
    let mut response = upgrade.on_upgrade(move |socket| async move {
        let cancel = state.shutdown.child_token();
        let generation = state.generation.fetch_add(1, Ordering::Relaxed);
        // Replace the live connection and take the hosted subscription under
        // the same lock, so subscriptions always follow socket-accept order
        // and a superseded connection can never displace its replacement.
        let subscription = {
            let mut active = state.active.lock().await;
            if cancel.is_cancelled() {
                return;
            }
            if let Some((_, previous)) = active.replace((generation, cancel.clone())) {
                previous.cancel();
            }
            state.hosted.subscribe().await
        };
        connection::serve_connection(socket, state.hosted.clone(), subscription, cancel).await;
        let mut active = state.active.lock().await;
        if active
            .as_ref()
            .is_some_and(|(current, _)| *current == generation)
        {
            *active = None;
        }
    });
    match HeaderValue::from_str(&connection_id) {
        Ok(value) => {
            response
                .headers_mut()
                .insert(ACP_CONNECTION_ID_HEADER, value);
        }
        Err(error) => {
            tracing::warn!("failed to set {ACP_CONNECTION_ID_HEADER}: {error}");
        }
    }
    response
}
