//! Listener for the remote ACP transport.
//!
//! Background hosts use authenticated `/sessions` listeners with separate
//! controller and watcher connections.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

/// A running remote ACP listener.
///
/// Dropping the server stops accepting new connections; open connections
/// are closed by [`RemoteAcpServer::shutdown`].
pub struct RemoteAcpServer {
    local_addrs: Vec<SocketAddr>,
    serve_tasks: Vec<tokio::task::JoinHandle<()>>,
    host_control: Arc<tokio::sync::Mutex<super::host_control::HostControl>>,
    shutdown: CancellationToken,
}

impl RemoteAcpServer {
    /// Bind an authenticated background host. Every connection requires the
    /// token.
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
            host_control: control,
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

    /// Serve one in-process host client over channels of JSON-RPC lines,
    /// with the same methods as stdio, until `input` closes (`None`) or
    /// `stop` completes (its value).
    pub async fn serve_host_pipe<M: super::ManagedAgent, T>(
        managed: Arc<M>,
        input: tokio::sync::mpsc::Receiver<String>,
        output: tokio::sync::mpsc::Sender<String>,
        stop: impl std::future::Future<Output = T>,
    ) -> anyhow::Result<Option<T>> {
        let instance = managed
            .instance_id()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        super::host_wire::serve_pipe(managed, instance, input, output, stop).await
    }

    /// Every address on which this server is listening.
    pub fn local_addrs(&self) -> &[SocketAddr] {
        &self.local_addrs
    }

    /// Whether a remote controller currently owns the WebSocket connection.
    pub async fn controller_connected(&self) -> bool {
        self.host_control.lock().await.connected()
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
