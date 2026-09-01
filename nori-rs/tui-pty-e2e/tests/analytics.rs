use std::io::Read;
use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use pretty_assertions::assert_eq;
use serde_json::Value;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

fn analytics_server() -> (String, mpsc::Receiver<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind analytics listener");
    let address = listener.local_addr().expect("analytics listener address");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let (request_tx, request_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(error) => panic!("accept analytics request: {error}"),
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("set request timeout");
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 4096];
            let header_end = loop {
                let count = stream.read(&mut buffer).expect("read analytics request");
                assert!(count > 0, "analytics request closed before headers");
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let headers = String::from_utf8(bytes[..header_end].to_vec()).expect("UTF-8 headers");
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().expect("content length"))
                })
                .expect("content-length header");
            while bytes.len() - header_end < content_length {
                let count = stream.read(&mut buffer).expect("read analytics body");
                assert!(count > 0, "analytics request closed before body");
                bytes.extend_from_slice(&buffer[..count]);
            }
            request_tx
                .send(
                    serde_json::from_slice(&bytes[header_end..header_end + content_length])
                        .expect("analytics JSON body"),
                )
                .expect("deliver request");
            stream
            .write_all(
                b"HTTP/1.1 202 Accepted\r\nContent-Type: application/json\r\nContent-Length: 17\r\nConnection: close\r\n\r\n{\"accepted\":true}",
            )
            .expect("write analytics response");
        }
    });
    (
        format!("http://{address}/api/analytics/v1/events"),
        request_rx,
    )
}

fn fake_handroll() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("fake Handroll directory");
    let mock = std::env::current_exe()
        .expect("test executable")
        .parent()
        .and_then(|path| path.parent())
        .expect("target directory")
        .join("mock_acp_agent");
    assert!(mock.exists(), "missing mock agent at {}", mock.display());
    let script = dir.path().join("nori-handroll");
    std::fs::write(&script, format!("#!/bin/sh\nexec '{}'\n", mock.display()))
        .expect("write fake Handroll");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("make fake Handroll executable");
    }
    (dir, script)
}

fn authenticated_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("temporary home");
    let expires_at = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("system clock")
        .as_millis()
        + 60_000;
    std::fs::write(
        home.path().join(".nori-config.json"),
        serde_json::json!({
            "auth": {
                "username": "human@example.com",
                "idToken": "interactive-id-token",
                "idTokenExpiresAt": expires_at,
            }
        })
        .to_string(),
    )
    .expect("write authenticated config");
    home
}

#[test]
fn interactive_prompt_reports_authenticated_agent_session() {
    let home = authenticated_home();
    let (analytics_url, requests) = analytics_server();
    let config = SessionConfig::new()
        .with_agent_env("HOME", home.path().to_string_lossy())
        .with_agent_env("NORI_ANALYTICS_URL", analytics_url);
    let mut session = TuiSession::spawn_with_config(18, 80, config).expect("spawn Nori TUI");

    session.wait_for_text("›", TIMEOUT).expect("prompt ready");
    assert!(requests.recv_timeout(Duration::from_millis(300)).is_err());
    session.send_str("meaningful work").expect("type prompt");
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).expect("submit prompt");
    session
        .wait_for_text("Test message 2", TIMEOUT)
        .expect("agent response");

    let request = requests
        .recv_timeout(Duration::from_secs(3))
        .expect("authenticated analytics request");
    assert_eq!(
        request.get("event"),
        Some(&Value::String("nori_agent_session_started".to_string()))
    );
    assert_eq!(
        request.get("properties"),
        Some(&serde_json::json!({ "session_mode": "interactive" }))
    );
    session
        .send_str("second prompt")
        .expect("type second prompt");
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).expect("submit second prompt");
    session
        .wait_for_text("Test message 2", TIMEOUT)
        .expect("second agent response");
    assert!(requests.recv_timeout(Duration::from_millis(400)).is_err());
}

#[test]
fn cloud_prompt_reports_cloud_mode_after_the_agent_connects() {
    let home = authenticated_home();
    let (_handroll_dir, handroll) = fake_handroll();
    let (analytics_url, requests) = analytics_server();
    let config = SessionConfig::new()
        .with_subcommand("cloud")
        .with_arg("--onboard")
        .with_agent_env("HOME", home.path().to_string_lossy())
        .with_agent_env("NORI_ANALYTICS_URL", analytics_url)
        .with_agent_env("NORI_HANDROLL_BIN", handroll.to_string_lossy());
    let mut session = TuiSession::spawn_with_config(18, 80, config).expect("spawn Nori cloud");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("cloud prompt ready");
    assert!(requests.recv_timeout(Duration::from_millis(300)).is_err());
    session
        .send_str("meaningful cloud work")
        .expect("type prompt");
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).expect("submit prompt");
    session
        .wait_for_text("Test message 2", TIMEOUT)
        .expect("cloud agent response");

    let request = requests
        .recv_timeout(Duration::from_secs(3))
        .expect("cloud analytics request");
    assert_eq!(
        request.get("properties"),
        Some(&serde_json::json!({ "session_mode": "cloud" }))
    );
    assert!(requests.recv_timeout(Duration::from_millis(400)).is_err());
}
