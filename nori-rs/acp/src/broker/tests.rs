use super::*;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

fn make_jwt(exp: i64) -> String {
    let header = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        r#"{"alg":"HS256","typ":"JWT"}"#,
    );
    let payload = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        serde_json::json!({"exp": exp, "sub": "user@test.com"}).to_string(),
    );
    format!("{header}.{payload}.fakesignature")
}

fn future_jwt() -> String {
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 3600;
    make_jwt(exp)
}

fn expired_jwt() -> String {
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        - 3600;
    make_jwt(exp)
}

// ── JWT expiry detection ────────────────────────────────────────────

#[test]
fn expired_token_is_detected() {
    assert!(is_token_expired(&expired_jwt()));
}

#[test]
fn valid_token_is_not_expired() {
    assert!(!is_token_expired(&future_jwt()));
}

#[test]
fn malformed_token_is_treated_as_expired() {
    assert!(is_token_expired("not-a-jwt"));
    assert!(is_token_expired("only.two"));
    assert!(is_token_expired(""));
}

#[test]
fn token_without_exp_claim_is_expired() {
    let header = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        r#"{"alg":"HS256"}"#,
    );
    let payload = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        r#"{"sub":"user@test.com"}"#,
    );
    let token = format!("{header}.{payload}.sig");
    assert!(is_token_expired(&token));
}

// ── Credential persistence ──────────────────────────────────────────

#[test]
fn save_and_load_credentials_round_trip() {
    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: "https://broker.example.com".to_string(),
        auth_token: future_jwt(),
    };

    save_credentials(dir.path(), &creds).unwrap();
    let loaded = load_credentials(dir.path());

    assert_eq!(loaded, Some(creds));
}

#[test]
fn load_credentials_returns_none_when_file_missing() {
    let dir = tempdir().unwrap();
    assert_eq!(load_credentials(dir.path()), None);
}

#[test]
fn save_credentials_creates_directory_if_needed() {
    let dir = tempdir().unwrap();
    let nested = dir.path().join("deeply").join("nested");
    let creds = CloudCredentials {
        broker_url: "https://broker.example.com".to_string(),
        auth_token: "some-token".to_string(),
    };

    save_credentials(&nested, &creds).unwrap();
    let loaded = load_credentials(&nested);
    assert_eq!(loaded, Some(creds));
}

#[cfg(unix)]
#[test]
fn save_credentials_sets_restrictive_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: "https://broker.example.com".to_string(),
        auth_token: "secret-token".to_string(),
    };

    save_credentials(dir.path(), &creds).unwrap();
    let path = dir.path().join("cloud-auth.json");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

// ── Callback URL token extraction ──────────────────────────────────

#[test]
fn extracts_token_from_callback_url() {
    let token = extract_token_from_callback("/callback?token=abc123");
    assert_eq!(token, Some("abc123".to_string()));
}

#[test]
fn extracts_token_with_extra_params() {
    let token = extract_token_from_callback("/callback?state=xyz&token=mytoken&other=val");
    assert_eq!(token, Some("mytoken".to_string()));
}

#[test]
fn returns_none_when_no_token_param() {
    let token = extract_token_from_callback("/callback?code=abc");
    assert_eq!(token, None);
}

#[test]
fn returns_none_for_url_without_query_string() {
    let token = extract_token_from_callback("/callback");
    assert_eq!(token, None);
}

#[test]
fn builds_cli_auth_url_under_api_prefix() {
    let url = build_cli_auth_url("https://broker.test", 4321);
    assert_eq!(
        url,
        "https://broker.test/api/auth/cli?redirect_uri=http://localhost:4321/callback"
    );
}

#[tokio::test]
async fn authenticate_prints_url_during_browser_login() {
    let dir = tempdir().unwrap();
    let broker_url = "https://broker.test".to_string();
    let mut client = BrokerClient::new(broker_url.clone(), dir.path().to_path_buf());
    let token = future_jwt();
    let callback_token = token.clone();
    let mut output = Vec::new();

    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.authenticate_with(&mut output, move |auth_url| {
            let auth_url = auth_url.to_string();
            let callback_token = callback_token;
            std::thread::spawn(move || complete_auth_callback(&auth_url, &callback_token));
            true
        }),
    )
    .await
    .unwrap()
    .unwrap();

    let output = String::from_utf8(output).unwrap();
    let expected_url_prefix = "https://broker.test/api/auth/cli?redirect_uri=http://localhost:";
    assert!(
        output.contains(expected_url_prefix),
        "expected auth output to include URL prefix {expected_url_prefix}, got {output:?}"
    );

    assert_eq!(
        load_credentials(dir.path()),
        Some(CloudCredentials {
            broker_url,
            auth_token: token,
        })
    );
}

fn complete_auth_callback(auth_url: &str, token: &str) {
    use std::io::Read as _;
    use std::io::Write as _;

    let auth_url = url::Url::parse(auth_url).unwrap();
    let redirect_uri = auth_url
        .query_pairs()
        .find(|(key, _)| key == "redirect_uri")
        .map(|(_, value)| value.into_owned())
        .unwrap();
    let redirect_uri = url::Url::parse(&redirect_uri).unwrap();
    let port = redirect_uri.port().unwrap();
    let request_path = format!("{}?token={token}", redirect_uri.path());
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        stream,
        "GET {request_path} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(
        response.contains("Authentication successful"),
        "expected success response, got {response:?}"
    );
}

// ── BrokerClient construction ──────────────────────────────────────

#[test]
fn new_client_loads_existing_credentials() {
    let dir = tempdir().unwrap();
    let token = future_jwt();
    let creds = CloudCredentials {
        broker_url: "https://broker.test".to_string(),
        auth_token: token,
    };
    save_credentials(dir.path(), &creds).unwrap();

    let client = BrokerClient::new("https://broker.test".to_string(), dir.path().to_path_buf());
    assert!(client.has_valid_token());
}

#[test]
fn new_client_ignores_credentials_for_different_broker() {
    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: "https://other-broker.test".to_string(),
        auth_token: future_jwt(),
    };
    save_credentials(dir.path(), &creds).unwrap();

    let client = BrokerClient::new("https://broker.test".to_string(), dir.path().to_path_buf());
    assert!(!client.has_valid_token());
}

#[test]
fn new_client_detects_expired_stored_token() {
    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: "https://broker.test".to_string(),
        auth_token: expired_jwt(),
    };
    save_credentials(dir.path(), &creds).unwrap();

    let client = BrokerClient::new("https://broker.test".to_string(), dir.path().to_path_buf());
    assert!(!client.has_valid_token());
}

// ── acquire_session integration tests ───────────────────────────────

#[tokio::test]
async fn acquire_session_sends_auth_and_parses_response() {
    let mock_server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = mock_server.server_addr().to_ip().unwrap().port();
    let broker_url = format!("http://127.0.0.1:{port}");
    let token = future_jwt();

    let expected_token = token.clone();
    let server_handle = std::thread::spawn(move || {
        let mut request = mock_server.recv().unwrap();
        assert_eq!(request.method(), &tiny_http::Method::Post);
        assert!(request.url().contains("/api/sessions/acquire"));

        let auth_header = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Authorization"))
            .map(|h| h.value.to_string());
        assert_eq!(auth_header, Some(format!("Bearer {expected_token}")));

        let mut body = String::new();
        request.as_reader().read_to_string(&mut body).unwrap();
        let body_json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body_json, serde_json::json!({"source": "cli"}));

        let response_body = serde_json::json!({
            "session_id": "sess-abc123",
            "ws_url": "wss://broker.test/ws/sess-abc123"
        });
        let response = tiny_http::Response::from_string(response_body.to_string()).with_header(
            "Content-Type: application/json"
                .parse::<tiny_http::Header>()
                .unwrap(),
        );
        request.respond(response).unwrap();
    });

    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: token,
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let info = client.acquire_session().await.unwrap();
    assert_eq!(
        info,
        SessionInfo {
            session_id: "sess-abc123".to_string(),
            ws_url: "wss://broker.test/ws/sess-abc123".to_string(),
        }
    );

    server_handle.join().unwrap();
}

#[tokio::test]
async fn acquire_session_returns_token_expired_on_401() {
    let mock_server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = mock_server.server_addr().to_ip().unwrap().port();
    let broker_url = format!("http://127.0.0.1:{port}");
    let token = future_jwt();

    let server_handle = std::thread::spawn(move || {
        let request = mock_server.recv().unwrap();
        let response = tiny_http::Response::from_string("Unauthorized").with_status_code(401);
        request.respond(response).unwrap();
    });

    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: token,
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let err = client.acquire_session().await.unwrap_err();
    assert!(matches!(err, BrokerError::TokenExpired));

    server_handle.join().unwrap();
}

#[tokio::test]
async fn acquire_session_errors_without_token() {
    let dir = tempdir().unwrap();
    let client = BrokerClient::new("http://unused.test".to_string(), dir.path().to_path_buf());

    let err = client.acquire_session().await.unwrap_err();
    assert!(matches!(err, BrokerError::AuthRequired));
}

#[tokio::test]
async fn acquire_session_returns_error_on_server_error() {
    let mock_server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = mock_server.server_addr().to_ip().unwrap().port();
    let broker_url = format!("http://127.0.0.1:{port}");
    let token = future_jwt();

    let server_handle = std::thread::spawn(move || {
        let request = mock_server.recv().unwrap();
        let response =
            tiny_http::Response::from_string("Internal Server Error").with_status_code(500);
        request.respond(response).unwrap();
    });

    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: token,
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let err = client.acquire_session().await.unwrap_err();
    assert!(matches!(
        err,
        BrokerError::AcquireFailed { status: 500, .. }
    ));

    server_handle.join().unwrap();
}

// ── release_session ────────────────────────────────────────────────

#[tokio::test]
async fn release_session_sends_post_with_auth() {
    let mock_server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = mock_server.server_addr().to_ip().unwrap().port();
    let broker_url = format!("http://127.0.0.1:{port}");
    let token = future_jwt();

    let expected_token = token.clone();
    let server_handle = std::thread::spawn(move || {
        let request = mock_server.recv().unwrap();
        assert_eq!(request.method(), &tiny_http::Method::Post);
        assert!(request.url().contains("/api/sessions/sess-abc123/release"));

        let auth_header = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Authorization"))
            .map(|h| h.value.to_string());
        assert_eq!(auth_header, Some(format!("Bearer {expected_token}")));

        let response = tiny_http::Response::from_string("{}").with_status_code(200);
        request.respond(response).unwrap();
    });

    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: token,
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    client.release_session("sess-abc123").await.unwrap();
    server_handle.join().unwrap();
}

#[tokio::test]
async fn acquire_session_returns_token_expired_for_locally_expired_jwt() {
    let dir = tempdir().unwrap();
    let broker_url = "http://unused.test".to_string();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: expired_jwt(),
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let err = client.acquire_session().await.unwrap_err();
    assert!(matches!(err, BrokerError::TokenExpired));
}

#[tokio::test]
async fn release_session_returns_token_expired_for_locally_expired_jwt() {
    let dir = tempdir().unwrap();
    let broker_url = "http://unused.test".to_string();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: expired_jwt(),
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let err = client.release_session("sess-abc123").await.unwrap_err();
    assert!(matches!(err, BrokerError::TokenExpired));
}

#[tokio::test]
async fn acquire_session_returns_error_for_malformed_response() {
    let mock_server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = mock_server.server_addr().to_ip().unwrap().port();
    let broker_url = format!("http://127.0.0.1:{port}");
    let token = future_jwt();

    let server_handle = std::thread::spawn(move || {
        let request = mock_server.recv().unwrap();
        let response = tiny_http::Response::from_string("not valid json").with_status_code(200);
        request.respond(response).unwrap();
    });

    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: token,
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let err = client.acquire_session().await.unwrap_err();
    assert!(matches!(err, BrokerError::InvalidResponse(_)));

    server_handle.join().unwrap();
}

#[tokio::test]
async fn release_session_returns_token_expired_on_401() {
    let mock_server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = mock_server.server_addr().to_ip().unwrap().port();
    let broker_url = format!("http://127.0.0.1:{port}");
    let token = future_jwt();

    let server_handle = std::thread::spawn(move || {
        let request = mock_server.recv().unwrap();
        let response = tiny_http::Response::from_string("Unauthorized").with_status_code(401);
        request.respond(response).unwrap();
    });

    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: token,
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let err = client.release_session("sess-abc123").await.unwrap_err();
    assert!(matches!(err, BrokerError::TokenExpired));

    server_handle.join().unwrap();
}

#[tokio::test]
async fn release_session_returns_error_on_404() {
    let mock_server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = mock_server.server_addr().to_ip().unwrap().port();
    let broker_url = format!("http://127.0.0.1:{port}");
    let token = future_jwt();

    let server_handle = std::thread::spawn(move || {
        let request = mock_server.recv().unwrap();
        let response = tiny_http::Response::from_string("Not Found").with_status_code(404);
        request.respond(response).unwrap();
    });

    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: token,
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let err = client.release_session("sess-abc123").await.unwrap_err();
    assert!(matches!(
        err,
        BrokerError::ReleaseFailed { status: 404, .. }
    ));

    server_handle.join().unwrap();
}

#[tokio::test]
async fn release_session_errors_without_token() {
    let dir = tempdir().unwrap();
    let client = BrokerClient::new("http://unused.test".to_string(), dir.path().to_path_buf());

    let err = client.release_session("sess-abc123").await.unwrap_err();
    assert!(matches!(err, BrokerError::AuthRequired));
}

// ── list_sessions ─────────────────────────────────────────────────

#[tokio::test]
async fn list_sessions_sends_get_with_auth_and_parses_response() {
    let mock_server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = mock_server.server_addr().to_ip().unwrap().port();
    let broker_url = format!("http://127.0.0.1:{port}");
    let token = future_jwt();

    let expected_token = token.clone();
    let server_handle = std::thread::spawn(move || {
        let request = mock_server.recv().unwrap();
        assert_eq!(request.method(), &tiny_http::Method::Get);
        assert_eq!(request.url(), "/api/sessions");

        let auth_header = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Authorization"))
            .map(|h| h.value.to_string());
        assert_eq!(auth_header, Some(format!("Bearer {expected_token}")));

        let response_body = serde_json::json!([
            {
                "session_id": "sess-1",
                "source": "cli",
                "created_at": "2025-01-27T12:00:00Z",
                "last_active_at": "2025-01-27T14:30:00Z",
                "first_message_preview": "Fix the login bug",
                "status": "idle"
            },
            {
                "session_id": "sess-2",
                "source": "slack",
                "created_at": "2025-01-26T10:00:00Z",
                "last_active_at": "2025-01-26T11:00:00Z",
                "first_message_preview": null,
                "status": "idle"
            }
        ]);
        let response = tiny_http::Response::from_string(response_body.to_string()).with_header(
            "Content-Type: application/json"
                .parse::<tiny_http::Header>()
                .unwrap(),
        );
        request.respond(response).unwrap();
    });

    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: token,
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let sessions = client.list_sessions().await.unwrap();
    assert_eq!(
        sessions,
        vec![
            CloudSessionSummary {
                session_id: "sess-1".to_string(),
                source: "cli".to_string(),
                created_at: "2025-01-27T12:00:00Z".to_string(),
                last_active_at: "2025-01-27T14:30:00Z".to_string(),
                first_message_preview: Some("Fix the login bug".to_string()),
                status: "idle".to_string(),
            },
            CloudSessionSummary {
                session_id: "sess-2".to_string(),
                source: "slack".to_string(),
                created_at: "2025-01-26T10:00:00Z".to_string(),
                last_active_at: "2025-01-26T11:00:00Z".to_string(),
                first_message_preview: None,
                status: "idle".to_string(),
            },
        ]
    );

    server_handle.join().unwrap();
}

#[tokio::test]
async fn list_sessions_returns_token_expired_on_401() {
    let mock_server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = mock_server.server_addr().to_ip().unwrap().port();
    let broker_url = format!("http://127.0.0.1:{port}");
    let token = future_jwt();

    let server_handle = std::thread::spawn(move || {
        let request = mock_server.recv().unwrap();
        let response = tiny_http::Response::from_string("Unauthorized").with_status_code(401);
        request.respond(response).unwrap();
    });

    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: token,
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let err = client.list_sessions().await.unwrap_err();
    assert!(matches!(err, BrokerError::TokenExpired));

    server_handle.join().unwrap();
}

#[tokio::test]
async fn list_sessions_returns_error_on_server_error() {
    let mock_server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = mock_server.server_addr().to_ip().unwrap().port();
    let broker_url = format!("http://127.0.0.1:{port}");
    let token = future_jwt();

    let server_handle = std::thread::spawn(move || {
        let request = mock_server.recv().unwrap();
        let response =
            tiny_http::Response::from_string("Internal Server Error").with_status_code(500);
        request.respond(response).unwrap();
    });

    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: token,
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let err = client.list_sessions().await.unwrap_err();
    assert!(matches!(err, BrokerError::ListFailed { status: 500, .. }));

    server_handle.join().unwrap();
}

#[tokio::test]
async fn list_sessions_returns_empty_vec_when_no_sessions() {
    let mock_server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = mock_server.server_addr().to_ip().unwrap().port();
    let broker_url = format!("http://127.0.0.1:{port}");
    let token = future_jwt();

    let server_handle = std::thread::spawn(move || {
        let request = mock_server.recv().unwrap();
        let response = tiny_http::Response::from_string("[]").with_header(
            "Content-Type: application/json"
                .parse::<tiny_http::Header>()
                .unwrap(),
        );
        request.respond(response).unwrap();
    });

    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: token,
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let sessions = client.list_sessions().await.unwrap();
    assert!(sessions.is_empty());

    server_handle.join().unwrap();
}

#[tokio::test]
async fn list_sessions_errors_without_token() {
    let dir = tempdir().unwrap();
    let client = BrokerClient::new("http://unused.test".to_string(), dir.path().to_path_buf());

    let err = client.list_sessions().await.unwrap_err();
    assert!(matches!(err, BrokerError::AuthRequired));
}

// ── resume_session ────────────────────────────────────────────────

#[tokio::test]
async fn resume_session_sends_post_with_auth_and_parses_response() {
    let mock_server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = mock_server.server_addr().to_ip().unwrap().port();
    let broker_url = format!("http://127.0.0.1:{port}");
    let token = future_jwt();

    let expected_token = token.clone();
    let server_handle = std::thread::spawn(move || {
        let request = mock_server.recv().unwrap();
        assert_eq!(request.method(), &tiny_http::Method::Post);
        assert_eq!(request.url(), "/api/sessions/sess-abc123/resume");

        let auth_header = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Authorization"))
            .map(|h| h.value.to_string());
        assert_eq!(auth_header, Some(format!("Bearer {expected_token}")));

        let response_body = serde_json::json!({
            "session_id": "sess-abc123",
            "ws_url": "wss://broker.test/ws/sess-abc123"
        });
        let response = tiny_http::Response::from_string(response_body.to_string()).with_header(
            "Content-Type: application/json"
                .parse::<tiny_http::Header>()
                .unwrap(),
        );
        request.respond(response).unwrap();
    });

    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: token,
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let info = client.resume_session("sess-abc123").await.unwrap();
    assert_eq!(
        info,
        SessionInfo {
            session_id: "sess-abc123".to_string(),
            ws_url: "wss://broker.test/ws/sess-abc123".to_string(),
        }
    );

    server_handle.join().unwrap();
}

#[tokio::test]
async fn resume_session_returns_token_expired_on_401() {
    let mock_server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = mock_server.server_addr().to_ip().unwrap().port();
    let broker_url = format!("http://127.0.0.1:{port}");
    let token = future_jwt();

    let server_handle = std::thread::spawn(move || {
        let request = mock_server.recv().unwrap();
        let response = tiny_http::Response::from_string("Unauthorized").with_status_code(401);
        request.respond(response).unwrap();
    });

    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: token,
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let err = client.resume_session("sess-abc123").await.unwrap_err();
    assert!(matches!(err, BrokerError::TokenExpired));

    server_handle.join().unwrap();
}

#[tokio::test]
async fn resume_session_returns_error_on_404() {
    let mock_server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = mock_server.server_addr().to_ip().unwrap().port();
    let broker_url = format!("http://127.0.0.1:{port}");
    let token = future_jwt();

    let server_handle = std::thread::spawn(move || {
        let request = mock_server.recv().unwrap();
        let response = tiny_http::Response::from_string("Not Found").with_status_code(404);
        request.respond(response).unwrap();
    });

    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: token,
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let err = client.resume_session("sess-abc123").await.unwrap_err();
    assert!(matches!(err, BrokerError::ResumeFailed { status: 404, .. }));

    server_handle.join().unwrap();
}

#[tokio::test]
async fn resume_session_errors_without_token() {
    let dir = tempdir().unwrap();
    let client = BrokerClient::new("http://unused.test".to_string(), dir.path().to_path_buf());

    let err = client.resume_session("sess-abc123").await.unwrap_err();
    assert!(matches!(err, BrokerError::AuthRequired));
}

#[tokio::test]
async fn list_sessions_returns_token_expired_for_locally_expired_jwt() {
    let dir = tempdir().unwrap();
    let broker_url = "http://unused.test".to_string();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: expired_jwt(),
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let err = client.list_sessions().await.unwrap_err();
    assert!(matches!(err, BrokerError::TokenExpired));
}

#[tokio::test]
async fn resume_session_returns_token_expired_for_locally_expired_jwt() {
    let dir = tempdir().unwrap();
    let broker_url = "http://unused.test".to_string();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: expired_jwt(),
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let err = client.resume_session("sess-abc123").await.unwrap_err();
    assert!(matches!(err, BrokerError::TokenExpired));
}

#[tokio::test]
async fn list_sessions_returns_error_for_malformed_response() {
    let mock_server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = mock_server.server_addr().to_ip().unwrap().port();
    let broker_url = format!("http://127.0.0.1:{port}");
    let token = future_jwt();

    let server_handle = std::thread::spawn(move || {
        let request = mock_server.recv().unwrap();
        let response = tiny_http::Response::from_string("not valid json").with_status_code(200);
        request.respond(response).unwrap();
    });

    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: token,
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let err = client.list_sessions().await.unwrap_err();
    assert!(matches!(err, BrokerError::InvalidResponse(_)));

    server_handle.join().unwrap();
}

#[tokio::test]
async fn resume_session_returns_error_for_malformed_response() {
    let mock_server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = mock_server.server_addr().to_ip().unwrap().port();
    let broker_url = format!("http://127.0.0.1:{port}");
    let token = future_jwt();

    let server_handle = std::thread::spawn(move || {
        let request = mock_server.recv().unwrap();
        let response = tiny_http::Response::from_string("not valid json").with_status_code(200);
        request.respond(response).unwrap();
    });

    let dir = tempdir().unwrap();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: token,
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let err = client.resume_session("sess-abc123").await.unwrap_err();
    assert!(matches!(err, BrokerError::InvalidResponse(_)));

    server_handle.join().unwrap();
}

// ── session_id validation ────────────────────────────────────────

#[test]
fn validate_session_id_accepts_valid_ids() {
    assert!(validate_session_id("sess-abc123").is_ok());
    assert!(validate_session_id("my_session_42").is_ok());
    assert!(validate_session_id("ABC").is_ok());
}

#[test]
fn validate_session_id_rejects_path_traversal() {
    assert!(matches!(
        validate_session_id("../etc/passwd"),
        Err(BrokerError::InvalidSessionId(_))
    ));
    assert!(matches!(
        validate_session_id("sess/../../admin"),
        Err(BrokerError::InvalidSessionId(_))
    ));
}

#[test]
fn validate_session_id_rejects_empty() {
    assert!(matches!(
        validate_session_id(""),
        Err(BrokerError::InvalidSessionId(_))
    ));
}

#[test]
fn validate_session_id_rejects_special_characters() {
    assert!(matches!(
        validate_session_id("sess id"),
        Err(BrokerError::InvalidSessionId(_))
    ));
    assert!(matches!(
        validate_session_id("sess?query=1"),
        Err(BrokerError::InvalidSessionId(_))
    ));
}

#[tokio::test]
async fn resume_session_rejects_malformed_session_id() {
    let dir = tempdir().unwrap();
    let broker_url = "http://unused.test".to_string();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: future_jwt(),
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let err = client.resume_session("../admin").await.unwrap_err();
    assert!(matches!(err, BrokerError::InvalidSessionId(_)));
}

#[tokio::test]
async fn release_session_rejects_malformed_session_id() {
    let dir = tempdir().unwrap();
    let broker_url = "http://unused.test".to_string();
    let creds = CloudCredentials {
        broker_url: broker_url.clone(),
        auth_token: future_jwt(),
    };
    save_credentials(dir.path(), &creds).unwrap();
    let client = BrokerClient::new(broker_url, dir.path().to_path_buf());

    let err = client.release_session("sess/../../x").await.unwrap_err();
    assert!(matches!(err, BrokerError::InvalidSessionId(_)));
}
