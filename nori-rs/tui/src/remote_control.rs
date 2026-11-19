//! Runtime ownership and presentation for Nori's remote ACP listeners.

use std::collections::HashMap;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::app_event::SessionGeneration;
use nori_harness::ConversationId;
use nori_harness::remote_agent::HarnessRemoteHost;
use nori_harness::remote_agent::ManagedRemoteHost;
use nori_harness::remote_agent::RemoteAcpServer;
use nori_harness::runtime::HarnessHandle;

const USAGE: &str = "Usage: /remote-control [on [tailnet|IP:PORT]|off|status]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteControlRequest {
    EnableLocal,
    EnableTailnet,
    EnableExplicit(SocketAddr),
    Disable,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteControlTarget {
    Local,
    Tailnet(Ipv4Addr),
    Explicit(SocketAddr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteControlReport {
    lines: Vec<String>,
    urls: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RemoteControlOutcome {
    Report(RemoteControlReport),
    ConfirmationRequired(SocketAddr),
    Error(String),
}

impl RemoteControlReport {
    pub(crate) fn lines(&self) -> &[String] {
        &self.lines
    }

    pub(crate) fn urls(&self) -> &[String] {
        &self.urls
    }
}

struct HostedSession {
    handle: HarnessHandle,
    nori_home: PathBuf,
    started: nori_protocol::SessionStarted,
}

pub(crate) struct RemoteControlManager {
    host: Arc<HarnessRemoteHost>,
    managed: Option<Arc<ManagedRemoteHost>>,
    sessions: HashMap<SessionGeneration, HostedSession>,
    selected: Option<(SessionGeneration, Option<ConversationId>)>,
    server: Option<RemoteAcpServer>,
    target: Option<RemoteControlTarget>,
}

impl RemoteControlManager {
    pub(crate) fn new() -> Self {
        Self {
            host: Arc::new(HarnessRemoteHost::new()),
            managed: None,
            sessions: HashMap::new(),
            selected: None,
            server: None,
            target: None,
        }
    }

    pub(crate) fn with_sessions(manager: nori_harness::session_manager::SessionManager) -> Self {
        Self {
            managed: Some(Arc::new(ManagedRemoteHost::new(manager))),
            ..Self::new()
        }
    }

    /// Navigation changes the next connection's default. A connected legacy
    /// controller keeps the session it chose until it reconnects.
    pub(crate) fn select_session(
        &mut self,
        generation: SessionGeneration,
        id: Option<ConversationId>,
    ) {
        let Some(managed) = &self.managed else {
            return;
        };
        let id = id.filter(|_| self.sessions.contains_key(&generation));
        managed.set_selected_session(id);
        if self.selected == Some((generation, id)) {
            return;
        }
        self.selected = Some((generation, id));
        if let Some(session) = self.sessions.get(&generation) {
            let mut started = session.started.clone();
            if let Some(id) = id {
                started.transcript_id = Some(id.to_string());
            }
            self.host.set_default_started(
                session.handle.clone(),
                session.nori_home.clone(),
                started,
            );
        } else {
            self.host.clear_default_session();
        }
    }

    pub(crate) fn forget_session(&mut self, generation: SessionGeneration) {
        self.sessions.remove(&generation);
        if self
            .selected
            .is_some_and(|(selected, _)| selected == generation)
        {
            self.selected = None;
            self.select_session(generation, None);
        }
    }

    async fn bind(
        &self,
        addrs: impl IntoIterator<Item = SocketAddr>,
    ) -> anyhow::Result<RemoteAcpServer> {
        match &self.managed {
            Some(managed) => {
                RemoteAcpServer::bind_many_managed(addrs, self.host.clone(), managed.clone()).await
            }
            None => RemoteAcpServer::bind_many(addrs, self.host.clone()).await,
        }
    }

    #[cfg(test)]
    fn host(&self) -> Arc<HarnessRemoteHost> {
        self.host.clone()
    }

    pub(crate) async fn enable(
        &mut self,
        target: RemoteControlTarget,
        tailnet_detected: bool,
    ) -> Result<RemoteControlReport, String> {
        if self.target == Some(target) && self.server.is_some() {
            return Ok(self.status(tailnet_detected).await);
        }

        let addrs = target_addrs(target)?;
        let must_release_existing_port = self.server.as_ref().is_some_and(|server| {
            addrs.iter().any(|addr| {
                addr.port() != 0 && server.local_addrs().iter().any(|bound| bound == addr)
            })
        });
        let displaced = if must_release_existing_port {
            let Some(previous) = self.server.take() else {
                return Err(
                    "Could not enable remote control: the current listener disappeared during replacement."
                        .to_string(),
                );
            };
            let previous_target = self.target.take();
            let previous_addrs = previous.local_addrs().to_vec();
            previous.shutdown().await;
            Some((previous_target, previous_addrs))
        } else {
            None
        };
        let server = match self.bind(addrs).await {
            Ok(server) => server,
            Err(error) => {
                if let Some((previous_target, previous_addrs)) = displaced {
                    match self.bind(previous_addrs).await {
                        Ok(previous) => {
                            self.server = Some(previous);
                            self.target = previous_target;
                        }
                        Err(restore_error) => {
                            return Err(format!(
                                "Could not enable remote control: {error}. The previous remote-control surface also could not be restored: {restore_error}"
                            ));
                        }
                    }
                }
                return Err(format!("Could not enable remote control: {error}"));
            }
        };

        if let Some(previous) = self.server.replace(server) {
            previous.shutdown().await;
        }
        self.target = Some(target);
        Ok(self.status(tailnet_detected).await)
    }

    pub(crate) async fn enable_startup(
        &mut self,
        addr: SocketAddr,
        tailnet_detected: bool,
    ) -> Result<RemoteControlReport, String> {
        self.enable(RemoteControlTarget::Explicit(addr), tailnet_detected)
            .await
    }

    pub(crate) async fn execute_request_with_detection(
        &mut self,
        request: RemoteControlRequest,
        tailnet: Result<Ipv4Addr, String>,
    ) -> RemoteControlOutcome {
        match request {
            RemoteControlRequest::EnableLocal => self
                .enable(RemoteControlTarget::Local, tailnet.is_ok())
                .await
                .map_or_else(RemoteControlOutcome::Error, RemoteControlOutcome::Report),
            RemoteControlRequest::EnableTailnet => match tailnet {
                Ok(ip) => self
                    .enable(RemoteControlTarget::Tailnet(ip), true)
                    .await
                    .map_or_else(RemoteControlOutcome::Error, RemoteControlOutcome::Report),
                Err(error) => RemoteControlOutcome::Error(error),
            },
            RemoteControlRequest::EnableExplicit(addr) if !addr.ip().is_loopback() => {
                RemoteControlOutcome::ConfirmationRequired(addr)
            }
            RemoteControlRequest::EnableExplicit(addr) => self
                .enable(RemoteControlTarget::Explicit(addr), tailnet.is_ok())
                .await
                .map_or_else(RemoteControlOutcome::Error, RemoteControlOutcome::Report),
            RemoteControlRequest::Disable => RemoteControlOutcome::Report(self.disable().await),
            RemoteControlRequest::Status => {
                RemoteControlOutcome::Report(self.status(tailnet.is_ok()).await)
            }
        }
    }

    pub(crate) async fn confirm_explicit(&mut self, addr: SocketAddr) -> RemoteControlOutcome {
        self.enable(RemoteControlTarget::Explicit(addr), false)
            .await
            .map_or_else(RemoteControlOutcome::Error, RemoteControlOutcome::Report)
    }

    pub(crate) async fn remember_started(
        &mut self,
        generation: SessionGeneration,
        handle: HarnessHandle,
        nori_home: PathBuf,
        started: nori_protocol::SessionStarted,
    ) -> Result<(), String> {
        if self.managed.is_none() {
            return self.attach_started(handle, nori_home, started).await;
        }
        self.sessions.insert(
            generation,
            HostedSession {
                handle,
                nori_home,
                started,
            },
        );
        if let Some((selected, id)) = self.selected
            && selected == generation
        {
            self.selected = None;
            self.select_session(generation, id);
        }
        Ok(())
    }

    pub(crate) async fn attach_started(
        &self,
        handle: HarnessHandle,
        nori_home: PathBuf,
        started: nori_protocol::SessionStarted,
    ) -> Result<(), String> {
        self.host
            .attach_started(handle, nori_home, started)
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn status(&self, tailnet_detected: bool) -> RemoteControlReport {
        let Some(server) = self.server.as_ref() else {
            return RemoteControlReport {
                lines: vec![
                    "Remote control: off".to_string(),
                    "Scope: disabled".to_string(),
                    "ACP URLs: none".to_string(),
                    "Controller: disconnected".to_string(),
                ],
                urls: Vec::new(),
            };
        };

        let urls = server
            .local_addrs()
            .iter()
            .map(|addr| format!("ws://{addr}/acp"))
            .collect::<Vec<_>>();
        let scope = target_scope(self.target);
        let mut lines = vec![
            "Remote control: on".to_string(),
            format!("Scope: {scope}"),
            "ACP URLs:".to_string(),
        ];
        lines.extend(urls.iter().cloned());
        lines.push(format!(
            "Controller: {}",
            if server.controller_connected().await {
                "connected"
            } else {
                "disconnected"
            }
        ));
        if scope == "local-only" && tailnet_detected {
            lines.push("Hint: Tailscale is available; use /remote-control on tailnet.".to_string());
        }
        RemoteControlReport { lines, urls }
    }

    pub(crate) async fn disable(&mut self) -> RemoteControlReport {
        if let Some(server) = self.server.take() {
            server.shutdown().await;
        }
        self.target = None;
        // Give the aborted accept loops and cancelled socket a chance to drop
        // before the success message reaches the UI.
        tokio::task::yield_now().await;
        RemoteControlReport {
            lines: vec!["Remote control disabled.".to_string()],
            urls: Vec::new(),
        }
    }
}

fn target_scope(target: Option<RemoteControlTarget>) -> &'static str {
    match target {
        Some(RemoteControlTarget::Local) => "local-only",
        Some(RemoteControlTarget::Explicit(addr)) if addr.ip().is_loopback() => "local-only",
        Some(RemoteControlTarget::Tailnet(_)) => "local + tailnet",
        Some(RemoteControlTarget::Explicit(_)) => "local + explicit address",
        None => "disabled",
    }
}

fn target_addrs(target: RemoteControlTarget) -> Result<Vec<SocketAddr>, String> {
    let loopback = |port| SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let addrs = match target {
        RemoteControlTarget::Local => vec![loopback(0)],
        RemoteControlTarget::Tailnet(ip) => {
            vec![loopback(0), SocketAddr::new(IpAddr::V4(ip), 0)]
        }
        RemoteControlTarget::Explicit(addr) => {
            if addr.ip().is_unspecified() {
                return Err(
                    "Wildcard addresses are not supported; use an exact IP address.".to_string(),
                );
            }
            let local = loopback(addr.port());
            if addr == local {
                vec![addr]
            } else {
                vec![local, addr]
            }
        }
    };
    Ok(addrs)
}

pub(crate) fn parse_remote_control_request(
    text: &str,
) -> Option<Result<RemoteControlRequest, String>> {
    let mut words = text.split_whitespace();
    if words.next()? != "/remote-control" {
        return None;
    }
    let rest = words.collect::<Vec<_>>();
    let request = match rest.as_slice() {
        [] | ["on"] => Ok(RemoteControlRequest::EnableLocal),
        ["on", "tailnet"] => Ok(RemoteControlRequest::EnableTailnet),
        ["on", addr] => match addr.parse::<SocketAddr>() {
            Ok(addr) if addr.ip().is_unspecified() => Err(format!(
                "Wildcard addresses are not supported; use an exact IP address.\n{USAGE}"
            )),
            Ok(addr) => Ok(RemoteControlRequest::EnableExplicit(addr)),
            Err(_) => Err(USAGE.to_string()),
        },
        ["off"] => Ok(RemoteControlRequest::Disable),
        ["status"] => Ok(RemoteControlRequest::Status),
        _ => Err(USAGE.to_string()),
    };
    Some(request)
}

pub(crate) fn parse_tailscale_status(output: &[u8]) -> Result<Ipv4Addr, String> {
    let value: serde_json::Value = serde_json::from_slice(output)
        .map_err(|_| "Could not parse `tailscale status --json` output.".to_string())?;
    if value
        .get("BackendState")
        .and_then(serde_json::Value::as_str)
        != Some("Running")
    {
        return Err("Tailscale is not running.".to_string());
    }
    let ips = value
        .get("TailscaleIPs")
        .or_else(|| {
            value
                .get("Self")
                .and_then(|value| value.get("TailscaleIPs"))
        })
        .and_then(serde_json::Value::as_array);
    ips.into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .filter_map(|ip| ip.parse::<Ipv4Addr>().ok())
        .find(|ip| !ip.is_unspecified())
        .ok_or_else(|| "Tailscale did not report an IPv4 address.".to_string())
}

pub(crate) async fn detect_tailnet_ipv4() -> Result<Ipv4Addr, String> {
    match tokio::time::timeout(
        Duration::from_secs(3),
        detect_tailnet_ipv4_with(|| async {
            let output = tokio::process::Command::new("tailscale")
                .args(["status", "--json"])
                .output()
                .await?;
            Ok(TailnetCommandOutput {
                success: output.status.success(),
                stdout: output.stdout,
                stderr: output.stderr,
            })
        }),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err("`tailscale status --json` timed out.".to_string()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TailnetCommandOutput {
    success: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

async fn detect_tailnet_ipv4_with<F, Fut>(run: F) -> Result<Ipv4Addr, String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = std::io::Result<TailnetCommandOutput>>,
{
    let output = run()
        .await
        .map_err(|error| format!("Could not run `tailscale status --json`: {error}"))?;
    if !output.success {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            "unknown error".to_string()
        } else {
            stderr
        };
        return Err(format!("`tailscale status --json` failed: {detail}"));
    }
    parse_tailscale_status(&output.stdout)
}

#[cfg(test)]
#[path = "remote_control_tests.rs"]
mod tests;
