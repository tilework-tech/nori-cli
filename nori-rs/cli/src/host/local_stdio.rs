//! A local host served to one client on this process's stdin and stdout.

use std::time::Duration;

use nori_config::NoriConfig;
use nori_config::NoriConfigOverrides;
use nori_harness::remote_agent::RemoteAcpServer;
use nori_harness::session_manager::SessionManager;
use nori_harness::session_manager::SessionManagerConfig;

/// Why the host stopped before stdin closed.
enum Stop {
    Requested,
    Signal(&'static str),
}

/// Serve a local host whose one client speaks ACP on stdin and stdout. It
/// takes no host lock and publishes no discovery, so it runs beside a
/// background host of the same home, and its sessions end with it.
pub(crate) async fn serve(
    (config, overrides): (NoriConfig, NoriConfigOverrides),
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !config.host.managed,
        "this home sets [host] managed = true; a managed host serves stdio only with `nori host --stdio --launch FILE`"
    );
    nori_harness::initialize_registry(config.agents.clone())?;
    let manager = SessionManager::new(SessionManagerConfig {
        max_sessions: config.max_sessions,
        ..Default::default()
    });
    let host = super::host_service(config, overrides, manager, None, None);
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let stop = async {
        #[cfg(unix)]
        let stop = tokio::select! {
            _ = host.stop.notified() => Stop::Requested,
            _ = tokio::signal::ctrl_c() => Stop::Signal("interrupted"),
            _ = terminate.recv() => Stop::Signal("terminated"),
        };
        #[cfg(not(unix))]
        let stop = tokio::select! {
            _ = host.stop.notified() => Stop::Requested,
            _ = tokio::signal::ctrl_c() => Stop::Signal("interrupted"),
        };
        stop
    };
    let served = RemoteAcpServer::serve_host_stdio(host.clone(), stop).await;
    let report = host.manager.shutdown(Duration::ZERO).await;
    match served? {
        None | Some(Stop::Requested) => {}
        Some(Stop::Signal(signal)) => anyhow::bail!("{signal}"),
    }
    anyhow::ensure!(
        report.timed_out.is_empty() && report.failed.is_empty(),
        "some sessions did not stop cleanly: {report:?}"
    );
    Ok(())
}
