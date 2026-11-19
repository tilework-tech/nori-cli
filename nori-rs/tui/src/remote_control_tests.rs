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

// Other tests can fork while a listener is open, retaining its socket until
// exec even after this process closes it. Isolate same-port replacement so
// those unrelated children cannot inherit the listener under test.
async fn run_in_isolated_process(test_name: &str) -> bool {
    const CHILD_TEST: &str = "NORI_REMOTE_CONTROL_ISOLATED_TEST";
    if std::env::var(CHILD_TEST).ok().as_deref() == Some(test_name) {
        return false;
    }

    let mut command = tokio::process::Command::new(
        std::env::current_exe().expect("locate current test executable"),
    );
    command
        .args([
            "--exact",
            test_name,
            "--test-threads=1",
            "--nocapture",
            "--format=pretty",
        ])
        .env(CHILD_TEST, test_name)
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(15), command.output())
        .await
        .expect("isolated listener test timed out")
        .expect("run isolated listener test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("test result: ok. 1 passed;"),
        "isolated test {test_name} must run and pass exactly once: {}\n{stdout}\n{stderr}",
        output.status,
    );
    true
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
    if run_in_isolated_process(
        "remote_control::tests::changing_exposure_can_reuse_the_existing_loopback_port",
    )
    .await
    {
        return;
    }

    let reserved = std::net::TcpListener::bind("0.0.0.0:0").expect("reserve a free port");
    let port = reserved.local_addr().expect("reserved address").port();
    drop(reserved);

    let mut manager = RemoteControlManager::new();
    manager
        .enable(
            RemoteControlTarget::Explicit(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)),
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
    if run_in_isolated_process(
        "remote_control::tests::failed_same_port_replacement_restores_the_previous_surface",
    )
    .await
    {
        return;
    }

    let reserved = std::net::TcpListener::bind("0.0.0.0:0").expect("reserve a free port");
    let port = reserved.local_addr().expect("reserved address").port();
    drop(reserved);

    let mut manager = RemoteControlManager::new();
    let original = manager
        .enable(
            RemoteControlTarget::Explicit(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)),
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
async fn an_unstarted_selection_clears_the_next_remote_default_without_interrupting_the_controller()
{
    let sessions = nori_harness::session_manager::SessionManager::new(Default::default());
    let mut manager = RemoteControlManager::with_sessions(sessions);
    let home = tempfile::tempdir().expect("home");
    let mut current = launch_mock_session(&home).await;
    let started = wait_started(&mut current).await;
    let id = ConversationId::from_string(started.transcript_id.as_ref().expect("id")).unwrap();
    manager
        .remember_started(
            1,
            current.handle.clone(),
            home.path().to_path_buf(),
            started,
        )
        .await
        .unwrap();
    manager.select_session(1, Some(id));
    let host = manager.host();
    let first = host.subscribe().await;
    assert_eq!(host.list_sessions().await.unwrap().len(), 1);
    manager.select_session(2, None);
    assert_eq!(
        host.list_sessions().await.unwrap().len(),
        1,
        "existing controller stays pinned"
    );
    let second = host.subscribe().await;
    assert!(
        host.list_sessions().await.unwrap().is_empty(),
        "a fresh controller must not inherit the previous session"
    );
    host.detach(first.id).await;
    host.detach(second.id).await;
    current.handle.shutdown().await.unwrap();
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
