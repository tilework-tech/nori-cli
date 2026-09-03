//! End-to-end ownership tests for runtime remote ACP control.

use std::net::TcpListener;
use std::time::Duration;

use serde_json::Value;
use serde_json::json;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
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

fn connect_and_list(port: u16) -> (Client, Vec<String>) {
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
    let session_ids = listed["result"]["sessions"]
        .as_array()
        .expect("session catalog")
        .iter()
        .map(|session| {
            session["sessionId"]
                .as_str()
                .expect("session id")
                .to_string()
        })
        .collect();
    (client, session_ids)
}

fn wait_for_hosted_session(port: u16) -> (Client, String) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let (client, session_ids) = connect_and_list(port);
        if let [session_id] = session_ids.as_slice() {
            return (client, session_id.clone());
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for one hosted session; latest catalog: {session_ids:?}"
        );
        drop(client);
        std::thread::sleep(Duration::from_millis(50));
    }
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
        .with_mock_response("remote response before switch")
        .with_agent_env(
            "MOCK_AGENT_RESPONSE_MOCK_MODEL_ALT",
            "remote response after switch",
        )
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
    let (startup_client, startup_sessions) = connect_and_list(startup_port);
    assert!(
        startup_sessions.is_empty(),
        "prepared startup must not claim a session: {startup_sessions:?}"
    );
    drop(startup_client);

    session.submit_input("/new").expect("submit command");
    let (mut startup_client, original_session_id) = wait_for_hosted_session(startup_port);
    let prompt_before_switch = "remote prompt before switch";
    let response = request(
        &mut startup_client,
        3,
        "session/prompt",
        json!({
            "sessionId": original_session_id,
            "prompt": [{"type": "text", "text": prompt_before_switch}],
        }),
    );
    assert_eq!(response["result"]["stopReason"], json!("end_turn"));
    session
        .wait_for_text("remote response before switch", Duration::from_secs(10))
        .expect("remote turn response in observing TUI");
    assert_eq!(
        session
            .screen_contents()
            .matches(prompt_before_switch)
            .count(),
        1,
        "canonical remote prompt must render exactly once"
    );
    session
        .submit_input("/remote-control status")
        .expect("submit command");
    session
        .wait_for_text("Controller: connected", Duration::from_secs(10))
        .expect("live controller in durable status history");

    session
        .submit_input("/remote-control on tailnet")
        .expect("submit command");
    session
        .wait_for_text(
            "Could not run `tailscale status --json`",
            Duration::from_secs(10),
        )
        .expect("real Tailscale detection failure");

    session
        .submit_input("/remote-control off")
        .expect("submit command");
    session
        .wait_for_text("Remote control disabled.", Duration::from_secs(10))
        .expect("runtime off history");
    drop(startup_client);
    assert!(
        connect(format!("ws://127.0.0.1:{startup_port}/acp").as_str()).is_err(),
        "off must stop the listener created by --remote"
    );

    session
        .submit_input("/remote-control on")
        .expect("submit command");
    session
        .wait_for(
            |screen| has_loopback_acp_port_other_than(screen, startup_port),
            Duration::from_secs(10),
        )
        .expect("runtime local enable history");
    let runtime_port = latest_loopback_acp_port(&session.screen_contents());
    let (runtime_client, current_session_id) = wait_for_hosted_session(runtime_port);
    assert_eq!(current_session_id, original_session_id);
    drop(runtime_client);

    session.submit_input("/agent").expect("submit command");
    session
        .wait_for_text("Select agent", Duration::from_secs(10))
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

    let (mut switched_client, switched_session_id) = wait_for_hosted_session(runtime_port);
    assert_ne!(switched_session_id, original_session_id);
    let prompt_after_switch = "remote prompt after switch";
    let response = request(
        &mut switched_client,
        3,
        "session/prompt",
        json!({
            "sessionId": switched_session_id,
            "prompt": [{"type": "text", "text": prompt_after_switch}],
        }),
    );
    assert_eq!(response["result"]["stopReason"], json!("end_turn"));
    session
        .wait_for_text("remote response after switch", Duration::from_secs(10))
        .expect("switched remote turn response in observing TUI");
    assert_eq!(
        session
            .screen_contents()
            .matches(prompt_after_switch)
            .count(),
        1,
        "switched canonical remote prompt must render exactly once"
    );
}
