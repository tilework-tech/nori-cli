#![allow(clippy::expect_used)]

use std::io::Read;
use std::io::Write;
use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use assert_cmd::Command;
use pretty_assertions::assert_eq;
use serde_json::Value;
use tempfile::TempDir;

struct CapturedRequest {
    request_line: String,
    authorization: Option<String>,
    body: Value,
}

fn read_request(mut stream: TcpStream) -> CapturedRequest {
    stream
        .set_nonblocking(false)
        .expect("make accepted analytics stream blocking");
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
    let mut lines = headers.lines();
    let request_line = lines.next().expect("request line").to_string();
    let authorization = lines.find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("authorization")
            .then(|| value.trim().to_string())
    });
    let body = serde_json::from_slice(&bytes[header_end..header_end + content_length])
        .expect("analytics JSON body");
    stream
        .write_all(
            b"HTTP/1.1 202 Accepted\r\nContent-Type: application/json\r\nContent-Length: 17\r\nConnection: close\r\n\r\n{\"accepted\":true}",
        )
        .expect("write analytics response");
    CapturedRequest {
        request_line,
        authorization,
        body,
    }
}

fn analytics_server() -> (String, mpsc::Receiver<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind analytics listener");
    let address = listener.local_addr().expect("analytics listener address");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let (request_tx, request_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((stream, _)) => {
                    if request_tx.send(read_request(stream)).is_err() {
                        return;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept analytics request: {error}"),
            }
        }
    });
    (
        format!("http://{address}/api/analytics/v1/events"),
        request_rx,
    )
}

fn home_with_auth(username: Option<&str>) -> TempDir {
    let home = TempDir::new().expect("temporary home");
    let expires_at = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("system clock")
        .as_millis()
        + 60_000;
    if let Some(username) = username {
        std::fs::write(
            home.path().join(".nori-config.json"),
            serde_json::json!({
                "auth": {
                    "username": username,
                    "idToken": "firebase-id-token",
                    "idTokenExpiresAt": expires_at,
                }
            })
            .to_string(),
        )
        .expect("write authenticated config");
    }
    home
}

fn nori_command(home: &TempDir, analytics_url: &str) -> Command {
    let mut command = Command::cargo_bin("nori").expect("built nori binary");
    command
        .env("HOME", home.path())
        .env("NORI_HOME", home.path().join(".nori/cli"))
        .env("NORI_ANALYTICS_URL", analytics_url)
        .env(
            "MOCK_ACP_AGENT_BIN",
            std::env::var("MOCK_ACP_AGENT_BIN").expect("MOCK_ACP_AGENT_BIN"),
        )
        .env("MOCK_AGENT_ECHO_PROMPT", "1")
        .args(["--agent", "mock-model"]);
    command
}

#[test]
fn exec_reports_one_authenticated_agent_session_without_changing_output() {
    let home = home_with_auth(Some("human@example.com"));
    let (analytics_url, requests) = analytics_server();
    let output = nori_command(&home, &analytics_url)
        .args(["exec", "meaningful work"])
        .output()
        .expect("run nori exec");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 output"),
        "meaningful work\n"
    );

    let request = requests
        .recv_timeout(Duration::from_secs(3))
        .expect("authenticated analytics request");
    assert_eq!(
        request.request_line,
        "POST /api/analytics/v1/events HTTP/1.1"
    );
    assert_eq!(
        request.authorization.as_deref(),
        Some("Bearer firebase-id-token")
    );
    assert_eq!(
        request.body.get("event"),
        Some(&Value::String("nori_agent_session_started".to_string()))
    );
    assert_eq!(
        request.body.get("product"),
        Some(&Value::String("nori".to_string()))
    );
    assert_eq!(
        request.body.get("surface"),
        Some(&Value::String("cli".to_string()))
    );
    assert_eq!(
        request.body.get("properties"),
        Some(&serde_json::json!({ "session_mode": "exec" }))
    );
    let object = request.body.as_object().expect("analytics envelope object");
    assert_eq!(
        object
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            "activity_id",
            "app_version",
            "event",
            "occurred_at",
            "product",
            "properties",
            "schema_version",
            "surface",
        ])
    );
    assert_eq!(request.body.get("schema_version"), Some(&Value::from(1)));
    let activity_id = request.body["activity_id"].as_str().expect("activity UUID");
    assert_eq!(
        activity_id.split('-').map(str::len).collect::<Vec<_>>(),
        [8, 4, 4, 4, 12]
    );
    assert!(
        request.body["occurred_at"]
            .as_str()
            .is_some_and(|value| value.ends_with('Z'))
    );
    assert!(
        request.body["app_version"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(requests.recv_timeout(Duration::from_millis(400)).is_err());
}

#[test]
fn analytics_opt_out_and_ineligible_identities_emit_nothing() {
    for (username, opt_out) in [
        (Some("human@example.com"), true),
        (Some("nori-service:acme"), false),
        (None, false),
    ] {
        let home = home_with_auth(username);
        let (analytics_url, requests) = analytics_server();
        let mut command = nori_command(&home, &analytics_url);
        if opt_out {
            command.env("NORI_NO_ANALYTICS", "1");
        }
        let output = command
            .args(["exec", "meaningful work"])
            .output()
            .expect("run nori exec");

        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).expect("UTF-8 output"),
            "meaningful work\n"
        );
        assert!(requests.recv_timeout(Duration::from_millis(400)).is_err());
    }
}

#[test]
fn unavailable_analytics_never_changes_exec_output_or_exit() {
    let home = home_with_auth(Some("human@example.com"));
    let started = Instant::now();
    let output = nori_command(&home, "http://127.0.0.1:9/api/analytics/v1/events")
        .args(["exec", "meaningful work"])
        .output()
        .expect("run nori exec");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 output"),
        "meaningful work\n"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
}
