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
}

impl<H> Clone for RemoteState<H> {
    fn clone(&self) -> Self {
        Self {
            hosted: self.hosted.clone(),
            active: self.active.clone(),
            generation: self.generation.clone(),
        }
    }
}

/// A running remote ACP listener.
///
/// Dropping the server stops accepting new connections; the current
/// connection, if any, is closed by [`RemoteAcpServer::shutdown`].
pub struct RemoteAcpServer {
    local_addr: SocketAddr,
    serve_task: tokio::task::JoinHandle<()>,
    active: ActiveConnection,
}

impl RemoteAcpServer {
    /// Bind the listener and start serving `/acp`.
    pub async fn bind<H: HostedAgent>(addr: SocketAddr, hosted: Arc<H>) -> anyhow::Result<Self> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        let active: ActiveConnection = Arc::new(tokio::sync::Mutex::new(None));
        let state = RemoteState {
            hosted,
            active: active.clone(),
            generation: Arc::new(AtomicI64::new(0)),
        };
        let router = axum::Router::new()
            .route("/acp", axum::routing::any(acp_route::<H>))
            .with_state(state);
        let serve_task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, router).await {
                tracing::warn!("remote ACP server exited: {error}");
            }
        });
        Ok(Self {
            local_addr,
            serve_task,
            active,
        })
    }

    /// The address the listener actually bound (resolves port 0).
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Stop accepting connections and close the current one, if any.
    pub async fn shutdown(&self) {
        if let Some((_, cancel)) = self.active.lock().await.take() {
            cancel.cancel();
        }
        self.serve_task.abort();
    }
}

impl Drop for RemoteAcpServer {
    fn drop(&mut self) {
        self.serve_task.abort();
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
        let cancel = CancellationToken::new();
        let generation = state.generation.fetch_add(1, Ordering::Relaxed);
        {
            let mut active = state.active.lock().await;
            if let Some((_, previous)) = active.replace((generation, cancel.clone())) {
                previous.cancel();
            }
        }
        connection::serve_connection(socket, state.hosted.clone(), cancel).await;
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
