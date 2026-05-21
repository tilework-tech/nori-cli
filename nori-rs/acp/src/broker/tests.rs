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

// ── Callback URL token extraction ───────────────────────────────────

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

// ── BrokerClient construction ───────────────────────────────────────

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
        let request = mock_server.recv().unwrap();
        assert_eq!(request.method(), &tiny_http::Method::Post);
        assert!(request.url().contains("/api/sessions/acquire"));

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

    let info = client.acquire_session().await.unwrap();
    assert_eq!(info.session_id, "sess-abc123");
    assert_eq!(info.ws_url, "wss://broker.test/ws/sess-abc123");

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
