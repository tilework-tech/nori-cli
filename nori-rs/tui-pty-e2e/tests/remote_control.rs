//! End-to-end ownership tests for runtime remote ACP control.

use std::net::TcpListener;
use std::time::Duration;

use serde_json::Value;
use serde_json::json;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;
use tungstenite::Message;
use tungstenite::WebSocket;
use tungstenite::connect;
use tungstenite::stream::MaybeTlsStream;

type Client = WebSocket<MaybeTlsStream<std::net::TcpStream>>;

fn reserve_loopback_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("reserve loopback port")
        .local_addr()
        .expect("reserved address")
        .port()
}

fn request(client: &mut Client, id: i64, method: &str, params: Value) -> Value {
    client
        .send(Message::Text(
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            })
            .to_string()
            .into(),
        ))
        .expect("send ACP request");
    loop {
        let Message::Text(text) = client.read().expect("read ACP response") else {
            continue;
        };
        let value: Value = serde_json::from_str(text.as_str()).expect("ACP JSON");
        if value.get("id") == Some(&json!(id)) {
            return value;
        }
    }
}

fn connect_and_list(port: u16) -> (Client, String) {
    let (mut client, _) = connect(format!("ws://127.0.0.1:{port}/acp").as_str())
        .expect("connect to remote ACP listener");
    let initialized = request(
        &mut client,
        1,
        "initialize",
        json!({"protocolVersion": 1, "clientCapabilities": {}}),
    );
    assert!(initialized.get("result").is_some(), "{initialized}");
    let listed = request(&mut client, 2, "session/list", json!({}));
    let session_id = listed["result"]["sessions"][0]["sessionId"]
        .as_str()
        .expect("one hosted session")
        .to_string();
    (client, session_id)
}

fn run_command(session: &mut TuiSession, command: &str) {
    session.send_str(command).expect("type command");
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).expect("submit command");
}

fn latest_loopback_acp_port(screen: &str) -> u16 {
    let regex = regex::Regex::new(r"ws://127\.0\.0\.1:(\d+)/acp").expect("URL regex");
    regex
        .captures_iter(screen)
        .last()
        .and_then(|captures| captures.get(1))
        .expect("loopback ACP URL")
        .as_str()
        .parse()
        .expect("ACP port")
}

fn has_loopback_acp_port_other_than(screen: &str, old_port: u16) -> bool {
    let regex = regex::Regex::new(r"ws://127\.0\.0\.1:(\d+)/acp").expect("URL regex");
    regex.captures_iter(screen).any(|captures| {
        captures
            .get(1)
            .and_then(|port| port.as_str().parse::<u16>().ok())
            .is_some_and(|port| port != old_port)
    })
}

#[test]
fn startup_runtime_disable_reenable_and_agent_switch_share_one_remote_control_owner() {
    let startup_port = reserve_loopback_port();
    let config = SessionConfig::new()
        .with_agent("mock-model".to_string())
        .with_arg("--remote")
        .with_arg(startup_port.to_string())
        .with_excluded_binary("tailscale");
    let mut session = TuiSession::spawn_with_config(30, 100, config).expect("spawn Nori TUI");
    session.wait_for_text("›", TIMEOUT).expect("TUI startup");

    session
        .wait_for_text("ACP URLs:", Duration::from_secs(10))
        .expect("startup endpoint in durable history");
    let startup_screen = session.screen_contents();
    assert!(
        startup_screen.contains(&format!("ws://127.0.0.1:{startup_port}/acp")),
        "startup --remote did not retain requested port {startup_port}: {startup_screen}"
    );
    let (startup_client, original_session_id) = connect_and_list(startup_port);
    run_command(&mut session, "/remote-control status");
    session
        .wait_for_text("Controller: connected", Duration::from_secs(10))
        .expect("live controller in durable status history");

    run_command(&mut session, "/remote-control on tailnet");
    session
        .wait_for_text(
            "Could not run `tailscale status --json`",
            Duration::from_secs(10),
        )
        .expect("real Tailscale detection failure");

    run_command(&mut session, "/remote-control off");
    session
        .wait_for_text("Remote control disabled.", Duration::from_secs(10))
        .expect("runtime off history");
    drop(startup_client);
    assert!(
        connect(format!("ws://127.0.0.1:{startup_port}/acp").as_str()).is_err(),
        "off must stop the listener created by --remote"
    );

    run_command(&mut session, "/remote-control on");
    session
        .wait_for(
            |screen| has_loopback_acp_port_other_than(screen, startup_port),
            Duration::from_secs(10),
        )
        .expect("runtime local enable history");
    let runtime_port = latest_loopback_acp_port(&session.screen_contents());
    let (runtime_client, current_session_id) = connect_and_list(runtime_port);
    assert_eq!(current_session_id, original_session_id);
    drop(runtime_client);

    run_command(&mut session, "/agent");
    session
        .wait_for_text("Select Agent", Duration::from_secs(10))
        .expect("agent picker");
    session.send_key(Key::Down).expect("select alternate agent");
    session
        .send_key(Key::Enter)
        .expect("prepare alternate agent");
    session
        .wait_for_text("Start a new session", Duration::from_secs(10))
        .expect("candidate session picker");
    session.send_key(Key::Enter).expect("activate candidate");
    session
        .wait_for_text(
            "Started new conversation with agent: Mock ACP Alt",
            Duration::from_secs(10),
        )
        .expect("candidate SessionStarted commit");

    let (_switched_client, switched_session_id) = connect_and_list(runtime_port);
    assert_ne!(switched_session_id, original_session_id);
}
