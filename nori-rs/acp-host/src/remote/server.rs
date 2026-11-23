//! Listener and WebSocket upgrade for the remote ACP transport.
//!
//! Legacy `/acp` listeners keep one live connection; a newer connection
//! replaces it. Background hosts use authenticated `/sessions` listeners
//! with separate controller and watcher connections.

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
pub(super) type ActiveConnection = Arc<tokio::sync::Mutex<Option<(i64, CancellationToken)>>>;

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
    host_control: Option<Arc<tokio::sync::Mutex<super::host_control::HostControl>>>,
    shutdown: CancellationToken,
}

type ManagedRouterFactory =
    Box<dyn FnOnce(ActiveConnection, Arc<AtomicI64>, CancellationToken) -> axum::Router + Send>;

impl RemoteAcpServer {
    /// Bind an authenticated managed-only background host. Legacy ACP is not
    /// exposed on this listener, and every connection requires the token.
    pub async fn bind_host<M: super::ManagedAgent>(
        addr: SocketAddr,
        managed: Arc<M>,
        token: String,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(!token.trim().is_empty(), "host token must not be empty");
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        let shutdown = CancellationToken::new();
        let control = Arc::new(tokio::sync::Mutex::new(
            super::host_control::HostControl::default(),
        ));
        let instance = match (managed.instance_id(), managed.recovery()) {
            (Some(instance), _) => instance,
            (None, Some(recovery)) => {
                recovery
                    .run(|r| Ok(r.journal.incarnation().to_owned()))
                    .await?
            }
            (None, None) => uuid::Uuid::new_v4().to_string(),
        };
        let router =
            super::host_wire::router(managed, token, instance, control.clone(), shutdown.clone());
        let task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, router).await {
                tracing::warn!(%error, "managed host listener exited");
            }
        });
        Ok(Self {
            local_addrs: vec![local_addr],
            serve_tasks: vec![task],
            active: Arc::new(tokio::sync::Mutex::new(None)),
            host_control: Some(control),
            shutdown,
        })
    }

    /// Serve one host client on this process's stdin and stdout, with the
    /// same methods as an authenticated `/sessions` connection, until stdin
    /// closes (`None`) or `stop` completes (its value). The client always
    /// holds control.
    pub async fn serve_host_stdio<M: super::ManagedAgent, T>(
        managed: Arc<M>,
        stop: impl std::future::Future<Output = T>,
    ) -> anyhow::Result<Option<T>> {
        let instance = managed
            .instance_id()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        super::host_wire::serve_stdio(managed, instance, stop).await
    }

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
        Self::bind_many_inner(addrs, hosted, None).await
    }

    /// Expose the managed protocol beside ACP. Call only when multi-session is enabled.
    pub async fn bind_many_managed<H: HostedAgent, M: super::ManagedAgent>(
        addrs: impl IntoIterator<Item = SocketAddr>,
        hosted: Arc<H>,
        managed: Arc<M>,
    ) -> anyhow::Result<Self> {
        Self::bind_many_inner(
            addrs,
            hosted,
            Some(Box::new(move |active, generation, shutdown| {
                super::managed_wire::router(managed, active, generation, shutdown)
            })),
        )
        .await
    }

    async fn bind_many_inner<H: HostedAgent>(
        addrs: impl IntoIterator<Item = SocketAddr>,
        hosted: Arc<H>,
        managed: Option<ManagedRouterFactory>,
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
        let managed_router =
            managed.map(|make| make(active.clone(), state.generation.clone(), shutdown.clone()));
        let serve_tasks = listeners
            .into_iter()
            .map(|listener| {
                let router = axum::Router::new()
                    .route("/acp", axum::routing::any(acp_route::<H>))
                    .with_state(state.clone());
                let router = if let Some(managed) = &managed_router {
                    router.merge(managed.clone())
                } else {
                    router
                };
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
            host_control: None,
            shutdown,
        })
    }

    /// Every address on which this server is listening.
    pub fn local_addrs(&self) -> &[SocketAddr] {
        &self.local_addrs
    }

    /// Whether a remote controller currently owns the WebSocket connection.
    pub async fn controller_connected(&self) -> bool {
        if let Some(control) = &self.host_control {
            control.lock().await.connected()
        } else {
            self.active.lock().await.is_some()
        }
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
