//! The host that plain `nori` owns: built here, handed to the TUI, and
//! stopped when the TUI quits.
//!
//! The TUI reaches it two ways, both on the same host and the same sessions:
//! an ACP client over an in-process pipe, which is always the controller,
//! and the host's own session manager for foreground features the wire has
//! no method for yet. Only this launcher builds an [`OwnedHost`], so a
//! terminal attached to some other host never holds the in-process half.

use std::sync::Arc;
use std::time::Duration;

use nori_config::NoriConfig;
use nori_config::NoriConfigOverrides;
use nori_harness::remote_agent::RemoteAcpServer;
use nori_harness::session_manager::SessionManager;
use nori_harness::session_manager::SessionManagerConfig;
use nori_tui::HostLauncher;
use nori_tui::OwnedHost;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use super::service::HostService;

/// Builds the TUI's host from the configuration the TUI resolved. The TUI
/// has already loaded the agent registry, including agents its caller
/// injected, so the host does not load it again.
pub(crate) fn launcher() -> HostLauncher {
    Box::new(|config: &NoriConfig, overrides: &NoriConfigOverrides| {
        let manager = SessionManager::new(SessionManagerConfig {
            max_sessions: config.max_sessions,
            ..Default::default()
        });
        let host = super::host_service(config.clone(), overrides.clone(), manager, None, None);
        Ok(serve(host))
    })
}

fn serve(host: Arc<HostService>) -> OwnedHost {
    let (to_host, from_client) = mpsc::channel(64);
    let (to_client, from_host) = mpsc::channel(1024);
    let (stop, stopped) = oneshot::channel::<()>();
    let served = tokio::spawn(RemoteAcpServer::serve_host_pipe(
        host.clone(),
        from_client,
        to_client,
        stopped,
    ));
    OwnedHost {
        manager: host.manager.clone(),
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
