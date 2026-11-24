//! The host that plain `nori` owns: built here, handed to the TUI, and
//! stopped when the TUI quits.
//!
//! The TUI reaches it two ways, both on the same host and the same sessions:
//! an ACP client over an in-process pipe, which is always the controller,
//! and the host's own session manager for foreground features the wire has
//! no method for yet. Only this launcher builds an [`OwnedHost`], so a
//! terminal attached to some other host never holds the in-process half.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use nori_config::NoriConfig;
use nori_config::NoriConfigOverrides;
use nori_harness::remote_agent::RemoteAcpServer;
use nori_harness::session_manager::SessionManager;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use super::service::HostService;

/// A host the TUI owns for its whole run: an ACP client pipe that is
/// always the host's controller, and the host's own session manager for
/// foreground features the wire has no method for yet. Both reach the same
/// sessions. Only the foreground launcher builds one, so an attached
/// terminal never holds the in-process half.
pub struct OwnedHost {
    /// The host's session manager. The TUI never builds one of its own.
    pub manager: SessionManager,
    /// JSON-RPC lines to the host.
    pub to_host: mpsc::Sender<String>,
    /// JSON-RPC lines from the host.
    pub from_host: mpsc::Receiver<String>,
    /// Stops serving the pipe and ends every session the host owns.
    pub stop: StopHost,
    /// The terminal's current configuration for agents its pipe prepares.
    pub foreground_config: ForegroundConfig,
}

/// Hands the terminal's live configuration to its own host. Agents the
/// foreground pipe prepares use the latest one; other connections keep the
/// host's launch configuration.
#[derive(Clone)]
pub struct ForegroundConfig(Arc<HostService>);

impl ForegroundConfig {
    pub fn set(&self, config: NoriConfig) {
        *self
            .0
            .foreground_config
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(config));
    }
}

/// Stops an owned host; resolves once its sessions have ended.
pub type StopHost =
    Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> + Send>;

/// Builds the TUI's host from the configuration the TUI resolved. The TUI
/// has already loaded the agent registry, including agents its caller
/// injected, so the host does not load it again. The host is private to
/// the terminal: `[host] managed` does not apply, and only the TUI stops it.
pub fn launch(config: &NoriConfig, overrides: &NoriConfigOverrides) -> anyhow::Result<OwnedHost> {
    launch_with_context(config, overrides, None)
}

pub fn launch_with_context(
    config: &NoriConfig,
    overrides: &NoriConfigOverrides,
    context: Option<nori_harness::SessionContext>,
) -> anyhow::Result<OwnedHost> {
    let host = HostService::with_context(config.clone(), overrides.clone(), None, context)?;
    Ok(serve(host))
}

fn serve(host: Arc<HostService>) -> OwnedHost {
    let (to_host, from_client) = mpsc::channel(64);
    let (to_client, from_host) = mpsc::channel(1024);
    let (stop, stopped) = oneshot::channel::<()>();
    let served = tokio::spawn(RemoteAcpServer::serve_host_pipe_from(
        host.clone(),
        nori_harness::remote_agent::HostClientOrigin::Foreground,
        from_client,
        to_client,
        stopped,
    ));
    OwnedHost {
        manager: host.manager.clone(),
        foreground_config: ForegroundConfig(host.clone()),
        to_host,
        from_host,
        stop: Box::new(move || {
            Box::pin(async move {
                let _ = stop.send(());
                let served = served.await;
                let report = host.manager.shutdown(Duration::ZERO).await;
                served??;
                anyhow::ensure!(
                    report.timed_out.is_empty() && report.failed.is_empty(),
                    "some sessions did not stop cleanly: {report:?}"
                );
                Ok(())
            })
        }),
    }
}

#[cfg(test)]
#[path = "tui_owned_tests.rs"]
mod tests;
