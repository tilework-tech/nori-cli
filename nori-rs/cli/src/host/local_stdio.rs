//! A stdio client of a host: relayed to the home's running host when it can
//! share it, and otherwise served by a host of its own on this process's
//! stdin and stdout.

use std::sync::Arc;

use futures::StreamExt;
use std::time::Duration;

use nori_config::NoriConfig;
use nori_config::NoriConfigOverrides;
use nori_harness::remote_agent::RemoteAcpServer;
use nori_harness::session_manager::SessionManager;
use nori_harness::session_manager::SessionManagerConfig;

use super::service::HostService;

/// Why the host stopped before stdin closed.
enum Stop {
    Requested,
    Signal(&'static str),
}

/// A local host held by this process alone.
pub(super) fn local_host(
    config: NoriConfig,
    overrides: NoriConfigOverrides,
) -> anyhow::Result<Arc<HostService>> {
    nori_harness::initialize_registry(config.agents.clone())?;
    nori_harness::set_adapter_launch_overrides(config.acp_adapters.clone())?;
    let manager = SessionManager::new(SessionManagerConfig {
        max_sessions: config.max_sessions,
        ..Default::default()
    });
    Ok(super::host_service(config, overrides, manager, None, None))
}

/// Serve a client that speaks ACP on stdin and stdout.
///
/// A client without configuration of its own joins the home's running host
/// when that host gives it control, and shares that host's sessions, which
/// outlive the client. Otherwise it gets a local host that takes no host
/// lock and publishes no discovery, so it runs beside a background host of
/// the same home, and its sessions end with it.
pub(crate) async fn serve(
    (config, overrides): (NoriConfig, NoriConfigOverrides),
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !config.host.managed,
        "this home sets [host] managed = true; a managed host serves stdio only with `nori host --stdio --launch FILE`"
    );
    let mut lines = RemoteAcpServer::stdin_lines();
    // A running host applies its own configuration, so a client that brings
    // configuration needs a host of its own. The folder is not configuration
    // here: each session names its own, and its configuration applies there.
    let NoriConfigOverrides {
        agent,
        sandbox_mode,
        approval_policy,
        cwd: _,
        additional_writable_roots,
        raw_overrides,
    } = &overrides;
    let shares = agent.is_none()
        && sandbox_mode.is_none()
        && approval_policy.is_none()
        && additional_writable_roots.is_empty()
        && raw_overrides.is_empty();
    let first = if shares {
        match super::join::relay(&config.nori_home, &mut lines).await? {
            super::join::Joined::Relayed => return Ok(()),
            super::join::Joined::Declined(first) => first,
        }
    } else {
        None
    };
    let lines = futures::stream::iter(first.map(Ok))
        .chain(tokio_stream::wrappers::ReceiverStream::new(lines));
    let host = local_host(config, overrides)?;
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
    let served = RemoteAcpServer::serve_host_stdio_lines(host.clone(), lines, stop).await;
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
