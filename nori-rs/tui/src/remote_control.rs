//! Runtime ownership and presentation for Nori's remote ACP listeners.

use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use nori_harness::remote_agent::HarnessRemoteHost;
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

pub(crate) struct RemoteControlManager {
    host: Arc<HarnessRemoteHost>,
    server: Option<RemoteAcpServer>,
    target: Option<RemoteControlTarget>,
}

impl RemoteControlManager {
    pub(crate) fn new() -> Self {
        Self {
            host: Arc::new(HarnessRemoteHost::new()),
            server: None,
            target: None,
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
        let server = match RemoteAcpServer::bind_many(addrs, self.host.clone()).await {
            Ok(server) => server,
            Err(error) => {
                if let Some((previous_target, previous_addrs)) = displaced {
                    match RemoteAcpServer::bind_many(previous_addrs, self.host.clone()).await {
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
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::io::Read;
    use std::io::Write;
    use std::net::TcpStream;
    use std::net::UdpSocket;
    use std::time::Duration;

    use crate::history_cell::HistoryCell;
    use nori_config::NoriConfig;
    use nori_harness::remote_agent::HostedAgent;
    use nori_harness::runtime::AgentPrepareSpec;
    use nori_harness::runtime::LaunchedSession;
    use nori_harness::runtime::SessionLaunchSpec;
    use nori_harness::runtime::SessionStart;
    use nori_harness::runtime::launch_session;
    use nori_harness::runtime::prepare_agent;
    use nori_protocol::NoriEvent;
    use nori_protocol::SessionEvent;

    fn report_text(report: &RemoteControlReport) -> String {
        report.lines().join("\n")
    }

    fn addr_from_url(url: &str) -> SocketAddr {
        url.strip_prefix("ws://")
            .and_then(|value| value.strip_suffix("/acp"))
            .expect("ACP websocket URL")
            .parse()
            .expect("socket address")
    }

    async fn open_websocket(addr: SocketAddr) -> TcpStream {
        tokio::task::spawn_blocking(move || {
            let mut stream = TcpStream::connect(addr).expect("connect to ACP listener");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
            write!(
                stream,
                "GET /acp HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
            )
            .expect("write websocket handshake");
            let mut response = [0_u8; 512];
            let read = stream.read(&mut response).expect("read websocket handshake");
            let response = String::from_utf8_lossy(&response[..read]);
            assert!(response.starts_with("HTTP/1.1 101"), "{response}");
            stream
        })
        .await
        .expect("handshake task")
    }

    fn nonloopback_local_addr() -> SocketAddr {
        let socket = UdpSocket::bind("0.0.0.0:0").expect("bind route probe");
        socket.connect("192.0.2.1:9").expect("select a local route");
        let ip = socket.local_addr().expect("local route address").ip();
        assert!(
            !ip.is_loopback(),
            "test host needs a non-loopback interface"
        );
        SocketAddr::new(ip, 0)
    }

    fn nonloopback_local_ipv4() -> Ipv4Addr {
        match nonloopback_local_addr().ip() {
            IpAddr::V4(ip) => ip,
            IpAddr::V6(ip) => panic!("test host selected an IPv6 route: {ip}"),
        }
    }

    async fn launch_mock_session(home: &tempfile::TempDir) -> LaunchedSession {
        let config = NoriConfig {
            active_agent: "mock-model".to_string(),
            cwd: home.path().to_path_buf(),
            nori_home: home.path().to_path_buf(),
            ..Default::default()
        };
        let agent = prepare_agent(AgentPrepareSpec {
            config: Arc::new(config),
            cli_version: "remote-control-test".to_string(),
            session_context: None,
            initial_context: None,
        })
        .await
        .expect("prepare mock agent");
        launch_session(SessionLaunchSpec {
            agent,
            start: SessionStart::New,
        })
    }

    async fn wait_started(session: &mut LaunchedSession) -> nori_protocol::SessionStarted {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(SessionEvent::Nori(NoriEvent::SessionStarted(started))) =
                    session.events.recv().await
                {
                    return started;
                }
            }
        })
        .await
        .expect("session should start")
    }

    #[test]
    fn command_forms_parse_without_becoming_agent_prompts() {
        let cases = [
            ("/remote-control", RemoteControlRequest::EnableLocal),
            ("/remote-control on", RemoteControlRequest::EnableLocal),
            (
                "/remote-control on tailnet",
                RemoteControlRequest::EnableTailnet,
            ),
            (
                "/remote-control on 127.0.0.2:4812",
                RemoteControlRequest::EnableExplicit("127.0.0.2:4812".parse().expect("address")),
            ),
            ("/remote-control off", RemoteControlRequest::Disable),
            ("/remote-control status", RemoteControlRequest::Status),
        ];

        for (text, expected) in cases {
            assert_eq!(
                parse_remote_control_request(text),
                Some(Ok(expected)),
                "{text}"
            );
        }
        assert!(parse_remote_control_request("/remote-controller").is_none());
        assert_eq!(
            parse_remote_control_request("/remote-control on 0.0.0.0:4812"),
            Some(Err(
                "Wildcard addresses are not supported; use an exact IP address.\nUsage: /remote-control [on [tailnet|IP:PORT]|off|status]"
                    .to_string()
            ))
        );
        assert_eq!(
            parse_remote_control_request("/remote-control maybe"),
            Some(Err(USAGE.to_string()))
        );
    }

    #[test]
    fn running_tailscale_status_selects_its_exact_ipv4_address() {
        let root_ips = br#"{
            "BackendState": "Running",
            "TailscaleIPs": ["100.101.102.103", "fd7a:115c:a1e0::1"]
        }"#;
        assert_eq!(
            parse_tailscale_status(root_ips),
            Ok(Ipv4Addr::new(100, 101, 102, 103))
        );

        let self_ips = br#"{
            "BackendState": "Running",
            "Self": {"TailscaleIPs": ["100.64.0.9"]}
        }"#;
        assert_eq!(
            parse_tailscale_status(self_ips),
            Ok(Ipv4Addr::new(100, 64, 0, 9))
        );
    }

    #[test]
    fn tailscale_detection_rejects_stopped_invalid_and_ipv6_only_status() {
        assert_eq!(
            parse_tailscale_status(br#"{"BackendState":"Stopped","TailscaleIPs":["100.64.0.9"]}"#),
            Err("Tailscale is not running.".to_string())
        );
        assert_eq!(
            parse_tailscale_status(
                br#"{"BackendState":"Running","TailscaleIPs":["fd7a:115c:a1e0::1"]}"#
            ),
            Err("Tailscale did not report an IPv4 address.".to_string())
        );
        assert_eq!(
            parse_tailscale_status(b"not json"),
            Err("Could not parse `tailscale status --json` output.".to_string())
        );
    }

    #[tokio::test]
    async fn tailscale_command_not_found_and_nonzero_exit_are_actionable_failures() {
        let unavailable = detect_tailnet_ipv4_with(|| async {
            Err::<TailnetCommandOutput, _>(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "tailscale missing",
            ))
        })
        .await;
        assert_eq!(
            unavailable,
            Err("Could not run `tailscale status --json`: tailscale missing".to_string())
        );

        let stopped = detect_tailnet_ipv4_with(|| async {
            Ok(TailnetCommandOutput {
                success: false,
                stdout: Vec::new(),
                stderr: b"Tailscale is stopped".to_vec(),
            })
        })
        .await;
        assert_eq!(
            stopped,
            Err("`tailscale status --json` failed: Tailscale is stopped".to_string())
        );
    }

    #[tokio::test]
    async fn local_enable_and_status_render_only_reachable_urls_plus_tailnet_hint() {
        let mut manager = RemoteControlManager::new();
        let enabled = manager
            .enable(RemoteControlTarget::Local, true)
            .await
            .expect("enable loopback");
        let status = manager.status(true).await;

        let repeated = manager
            .enable(RemoteControlTarget::Local, true)
            .await
            .expect("repeat runtime enable");

        assert_eq!(enabled.urls().len(), 1);
        assert!(enabled.urls()[0].starts_with("ws://127.0.0.1:"));
        assert_eq!(enabled.urls(), status.urls());
        assert_eq!(
            enabled.urls(),
            repeated.urls(),
            "runtime on must be idempotent"
        );
        let rendered = report_text(&status);
        assert!(rendered.contains("Scope: local-only"));
        assert!(rendered.contains("Controller: disconnected"));
        assert!(rendered.contains("/remote-control on tailnet"));
        assert!(!rendered.contains("ws://100."));
        assert!(!rendered.contains("0.0.0.0"));

        let port = enabled.urls()[0]
            .split(':')
            .nth(2)
            .expect("port")
            .split('/')
            .next()
            .expect("port");
        let normalized = rendered.replace(port, "PORT");
        insta::assert_snapshot!(normalized, @r"
Remote control: on
Scope: local-only
ACP URLs:
ws://127.0.0.1:PORT/acp
Controller: disconnected
Hint: Tailscale is available; use /remote-control on tailnet.
");
    }

    #[tokio::test]
    async fn tailnet_enable_binds_loopback_and_the_exact_ip_on_one_port() {
        let exact_ip = nonloopback_local_ipv4();
        let mut manager = RemoteControlManager::new();
        let report = manager
            .enable(RemoteControlTarget::Tailnet(exact_ip), true)
            .await
            .expect("enable two listeners");

        assert_eq!(report.urls().len(), 2);
        let loopback = addr_from_url(&report.urls()[0]);
        let tailnet = addr_from_url(&report.urls()[1]);
        assert_eq!(loopback.ip(), Ipv4Addr::LOCALHOST);
        assert_eq!(tailnet.ip(), exact_ip);
        assert_eq!(loopback.port(), tailnet.port());
        assert!(TcpStream::connect(loopback).is_ok());
        assert!(TcpStream::connect(tailnet).is_ok());
    }

    #[tokio::test]
    async fn repeated_enable_is_idempotent_and_startup_remote_uses_the_same_lifecycle() {
        let startup_addr = nonloopback_local_addr();
        let mut manager = RemoteControlManager::new();
        let first = manager
            .enable_startup(startup_addr, false)
            .await
            .expect("startup --remote enable");
        assert_eq!(first.urls().len(), 2);
        let second = manager.status(false).await;
        assert_eq!(
            first.urls(),
            second.urls(),
            "idempotence must retain the port"
        );
        let repeated = manager
            .enable_startup(startup_addr, false)
            .await
            .expect("repeat startup enable");
        assert_eq!(
            first.urls(),
            repeated.urls(),
            "repeat enable must retain the port"
        );

        manager.disable().await;
        let after_disable = manager.status(false).await;
        assert_eq!(
            report_text(&after_disable),
            "Remote control: off\nScope: disabled\nACP URLs: none\nController: disconnected"
        );

        let reenabled = manager
            .enable(RemoteControlTarget::Local, false)
            .await
            .expect("runtime re-enable after --remote off");
        assert_eq!(reenabled.urls().len(), 1);
    }

    #[tokio::test]
    async fn loopback_startup_remote_is_local_only_and_shows_the_tailnet_hint() {
        let mut manager = RemoteControlManager::new();
        let report = manager
            .enable_startup("127.0.0.1:0".parse().expect("loopback address"), true)
            .await
            .expect("enable startup loopback");
        let rendered = report_text(&report);

        assert!(rendered.contains("Scope: local-only"));
        assert!(rendered.contains("/remote-control on tailnet"));
        assert_eq!(
            target_scope(Some(RemoteControlTarget::Explicit(
                "[::1]:4812".parse().expect("IPv6 loopback")
            ))),
            "local-only"
        );
    }

    #[tokio::test]
    async fn changing_exposure_can_reuse_the_existing_loopback_port() {
        let reserved = std::net::TcpListener::bind("0.0.0.0:0").expect("reserve a free port");
        let port = reserved.local_addr().expect("reserved address").port();
        drop(reserved);

        let mut manager = RemoteControlManager::new();
        manager
            .enable(
                RemoteControlTarget::Explicit(SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::LOCALHOST),
                    port,
                )),
                false,
            )
            .await
            .expect("enable fixed loopback listener");

        let explicit = SocketAddr::new(IpAddr::V4(nonloopback_local_ipv4()), port);
        let report = manager
            .enable(RemoteControlTarget::Explicit(explicit), false)
            .await
            .expect("replace listener while keeping its port");

        assert_eq!(report.urls().len(), 2);
        assert_eq!(addr_from_url(&report.urls()[0]).port(), port);
        assert_eq!(addr_from_url(&report.urls()[1]), explicit);
    }

    #[tokio::test]
    async fn failed_same_port_replacement_restores_the_previous_surface() {
        let reserved = std::net::TcpListener::bind("0.0.0.0:0").expect("reserve a free port");
        let port = reserved.local_addr().expect("reserved address").port();
        drop(reserved);

        let mut manager = RemoteControlManager::new();
        let original = manager
            .enable(
                RemoteControlTarget::Explicit(SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::LOCALHOST),
                    port,
                )),
                false,
            )
            .await
            .expect("enable fixed loopback listener");
        let original_addr = addr_from_url(&original.urls()[0]);
        let blocked_addr = SocketAddr::new(IpAddr::V4(nonloopback_local_ipv4()), port);
        let blocker = std::net::TcpListener::bind(blocked_addr).expect("occupy explicit address");

        let error = manager
            .enable(RemoteControlTarget::Explicit(blocked_addr), false)
            .await
            .expect_err("replacement should fail while its exact address is occupied");
        assert!(error.contains("Could not enable remote control"));
        assert_eq!(manager.status(false).await.urls(), original.urls());
        assert!(TcpStream::connect(original_addr).is_ok());

        drop(blocker);
    }

    #[tokio::test]
    async fn requests_cover_tailnet_failure_and_one_shot_explicit_confirmation() {
        let tailnet_ip = nonloopback_local_ipv4();
        let mut manager = RemoteControlManager::new();
        assert_eq!(
            manager
                .execute_request_with_detection(
                    RemoteControlRequest::EnableTailnet,
                    Err("Tailscale is not running.".to_string()),
                )
                .await,
            RemoteControlOutcome::Error("Tailscale is not running.".to_string())
        );
        assert!(manager.status(false).await.urls().is_empty());

        let tailnet = manager
            .execute_request_with_detection(RemoteControlRequest::EnableTailnet, Ok(tailnet_ip))
            .await;
        let RemoteControlOutcome::Report(tailnet) = tailnet else {
            panic!("tailnet enable should report endpoints");
        };
        assert_eq!(tailnet.urls().len(), 2);
        manager.disable().await;

        let explicit = nonloopback_local_addr();
        assert_eq!(
            manager
                .execute_request_with_detection(
                    RemoteControlRequest::EnableExplicit(explicit),
                    Err("unused".to_string()),
                )
                .await,
            RemoteControlOutcome::ConfirmationRequired(explicit)
        );
        assert!(manager.status(false).await.urls().is_empty());

        let accepted = manager.confirm_explicit(explicit).await;
        let RemoteControlOutcome::Report(accepted) = accepted else {
            panic!("accepted confirmation should enable listeners");
        };
        assert_eq!(accepted.urls().len(), 2);
        let loopback = addr_from_url(&accepted.urls()[0]);
        let requested = addr_from_url(&accepted.urls()[1]);
        assert_eq!(requested.ip(), explicit.ip());
        assert_eq!(requested.port(), loopback.port());
        assert!(TcpStream::connect(requested).is_ok());

        manager.disable().await;
        let cancelled = manager
            .execute_request_with_detection(
                RemoteControlRequest::EnableExplicit(explicit),
                Err("unused".to_string()),
            )
            .await;
        assert_eq!(
            cancelled,
            RemoteControlOutcome::ConfirmationRequired(explicit)
        );
        assert!(
            manager.status(false).await.urls().is_empty(),
            "leaving the confirmation without accepting must leave remote control off"
        );
    }

    #[tokio::test]
    async fn disable_disconnects_the_controller_and_stops_every_listener() {
        let tailnet_ip = nonloopback_local_ipv4();
        let mut manager = RemoteControlManager::new();
        let report = manager
            .enable(RemoteControlTarget::Tailnet(tailnet_ip), false)
            .await
            .expect("enable listeners");
        let addrs: Vec<SocketAddr> = report.urls().iter().map(|url| addr_from_url(url)).collect();
        let mut controller = open_websocket(addrs[1]).await;
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if report_text(&manager.status(false).await).contains("Controller: connected") {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("controller should become visible in status");

        let disabled = manager.disable().await;
        assert_eq!(report_text(&disabled), "Remote control disabled.");

        tokio::task::spawn_blocking(move || {
            let mut frame = [0_u8; 256];
            let result = controller.read(&mut frame);
            match result {
                Ok(0) => {}
                Ok(_read) => assert_eq!(frame[0] & 0x0f, 0x08, "expected a WebSocket close frame"),
                Err(error) => assert!(
                    !matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ),
                    "controller stayed open: {error}"
                ),
            }
        })
        .await
        .expect("controller close task");
        for addr in addrs {
            tokio::time::timeout(Duration::from_secs(2), async {
                while TcpStream::connect(addr).is_ok() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap_or_else(|_| panic!("listener remains at {addr}"));
        }
    }

    #[tokio::test]
    async fn listener_and_host_survive_switch_until_the_replacement_session_started() {
        let mut manager = RemoteControlManager::new();
        let enabled = manager
            .enable(RemoteControlTarget::Local, false)
            .await
            .expect("enable");
        let urls = enabled.urls().to_vec();
        let current_home = tempfile::tempdir().expect("current home");
        let mut current = launch_mock_session(&current_home).await;
        let current_started = wait_started(&mut current).await;
        manager
            .attach_started(
                current.handle.clone(),
                current_home.path().to_path_buf(),
                current_started.clone(),
            )
            .await
            .expect("attach current session");
        let host = manager.host();
        let current_info = host
            .list_sessions()
            .await
            .expect("list current")
            .pop()
            .expect("current session");

        let candidate_home = tempfile::tempdir().expect("candidate home");
        let mut candidate = launch_mock_session(&candidate_home).await;
        let candidate_started = wait_started(&mut candidate).await;
        assert_eq!(
            host.list_sessions().await.expect("list before commit"),
            vec![current_info],
            "candidate startup must not replace the hosted session"
        );

        manager
            .attach_started(
                candidate.handle.clone(),
                candidate_home.path().to_path_buf(),
                candidate_started.clone(),
            )
            .await
            .expect("commit candidate at SessionStarted");
        let switched = host
            .list_sessions()
            .await
            .expect("list replacement")
            .pop()
            .expect("replacement session");
        assert_eq!(
            switched.session_id.to_string(),
            candidate_started.transcript_id.expect("transcript id")
        );
        assert_eq!(manager.status(false).await.urls(), urls);

        manager.disable().await;
        assert_eq!(
            host.list_sessions().await.expect("list while disabled"),
            vec![switched],
            "off must stop listeners without closing the harness"
        );

        current.handle.shutdown().await.expect("shutdown current");
        candidate
            .handle
            .shutdown()
            .await
            .expect("shutdown candidate");
    }

    #[tokio::test]
    async fn enable_and_status_reports_render_as_durable_history_cells() {
        let mut manager = RemoteControlManager::new();
        let enabled = manager
            .enable(RemoteControlTarget::Local, false)
            .await
            .expect("enable");
        let status = manager.status(false).await;

        for report in [enabled, status] {
            let cell = crate::history_cell::new_remote_control_event(report.lines());
            let rendered = cell
                .display_lines(80)
                .into_iter()
                .map(|line| {
                    line.spans
                        .into_iter()
                        .map(|span| span.content.into_owned())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(rendered.contains("ws://127.0.0.1:"));
            assert!(rendered.contains("Controller: disconnected"));
        }
    }
}
