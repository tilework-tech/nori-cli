//! Anthropic OAuth login server for Claude Code authentication.
//!
//! This module provides OAuth browser-based authentication for Anthropic's Claude Code,
//! similar to how `server.rs` handles OpenAI authentication.

use std::io::Cursor;
use std::io::Read;
use std::io::Write;
use std::io::{self};
use std::net::SocketAddr;
use std::net::TcpStream;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::pkce::PkceCodes;
use crate::pkce::generate_pkce;
use base64::Engine;
use rand::RngCore;
use tiny_http::Header;
use tiny_http::Request;
use tiny_http::Response;
use tiny_http::Server;
use tiny_http::StatusCode;

/// Anthropic OAuth issuer URL
pub const ANTHROPIC_ISSUER: &str = "https://console.anthropic.com";
/// Default port for the local OAuth callback server
const DEFAULT_PORT: u16 = 1456;
/// Anthropic OAuth client ID for Claude Code CLI
pub const ANTHROPIC_CLIENT_ID: &str = "9d3c964e-d6f1-41b7-bff4-b29f0ce41766";

/// Options for running the Anthropic login server
#[derive(Debug, Clone)]
pub struct AnthropicServerOptions {
    /// Path to store authentication credentials
    pub config_dir: PathBuf,
    /// OAuth client ID
    pub client_id: String,
    /// OAuth issuer URL
    pub issuer: String,
    /// Port for the local callback server
    pub port: u16,
    /// Whether to automatically open the browser
    pub open_browser: bool,
}

impl AnthropicServerOptions {
    /// Create new options with defaults for Claude Code
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            config_dir,
            client_id: ANTHROPIC_CLIENT_ID.to_string(),
            issuer: ANTHROPIC_ISSUER.to_string(),
            port: DEFAULT_PORT,
            open_browser: true,
        }
    }
}

/// Anthropic login server handle
pub struct AnthropicLoginServer {
    /// The OAuth authorization URL to open in browser
    pub auth_url: String,
    /// The actual port the server is listening on
    pub actual_port: u16,
    /// Handle to the server task
    server_handle: tokio::task::JoinHandle<io::Result<()>>,
    /// Handle to cancel the server
    shutdown_handle: AnthropicShutdownHandle,
}

impl AnthropicLoginServer {
    /// Block until the login flow completes
    pub async fn block_until_done(self) -> io::Result<()> {
        self.server_handle
            .await
            .map_err(|err| io::Error::other(format!("login server thread panicked: {err:?}")))?
    }

    /// Cancel the login flow
    pub fn cancel(&self) {
        self.shutdown_handle.shutdown();
    }

    /// Get a handle for cancelling the server
    pub fn cancel_handle(&self) -> AnthropicShutdownHandle {
        self.shutdown_handle.clone()
    }
}

/// Handle to cancel the Anthropic login server
#[derive(Clone, Debug)]
pub struct AnthropicShutdownHandle {
    shutdown_notify: Arc<tokio::sync::Notify>,
}

impl AnthropicShutdownHandle {
    /// Trigger server shutdown
    pub fn shutdown(&self) {
        self.shutdown_notify.notify_waiters();
    }
}

/// Run the Anthropic OAuth login server
pub fn run_anthropic_login_server(opts: AnthropicServerOptions) -> io::Result<AnthropicLoginServer> {
    let pkce = generate_pkce();
    let state = generate_state();

    let server = bind_server(opts.port)?;
    let actual_port = match server.server_addr().to_ip() {
        Some(addr) => addr.port(),
        None => {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "Unable to determine the server port",
            ));
        }
    };
    let server = Arc::new(server);

    let redirect_uri = format!("http://localhost:{actual_port}/callback");
    let auth_url = build_authorize_url(&opts.issuer, &opts.client_id, &redirect_uri, &pkce, &state);

    if opts.open_browser {
        let _ = webbrowser::open(&auth_url);
    }

    // Map blocking reads from server.recv() to an async channel
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Request>(16);
    let _server_handle = {
        let server = server.clone();
        thread::spawn(move || -> io::Result<()> {
            while let Ok(request) = server.recv() {
                tx.blocking_send(request).map_err(|e| {
                    eprintln!("Failed to send request to channel: {e}");
                    io::Error::other("Failed to send request to channel")
                })?;
            }
            Ok(())
        })
    };

    let shutdown_notify = Arc::new(tokio::sync::Notify::new());
    let server_handle = {
        let shutdown_notify = shutdown_notify.clone();
        let server = server;
        tokio::spawn(async move {
            let result = loop {
                tokio::select! {
                    _ = shutdown_notify.notified() => {
                        break Err(io::Error::other("Login was not completed"));
                    }
                    maybe_req = rx.recv() => {
                        let Some(req) = maybe_req else {
                            break Err(io::Error::other("Login was not completed"));
                        };

                        let url_raw = req.url().to_string();
                        let response =
                            process_request(&url_raw, &opts, &redirect_uri, &pkce, actual_port, &state).await;

                        let exit_result = match response {
                            HandledRequest::Response(response) => {
                                let _ = tokio::task::spawn_blocking(move || req.respond(response)).await;
                                None
                            }
                            HandledRequest::ResponseAndExit {
                                headers,
                                body,
                                result,
                            } => {
                                let _ = tokio::task::spawn_blocking(move || {
                                    send_response_with_disconnect(req, headers, body)
                                })
                                .await;
                                Some(result)
                            }
                            HandledRequest::RedirectWithHeader(header) => {
                                let redirect = Response::empty(302).with_header(header);
                                let _ = tokio::task::spawn_blocking(move || req.respond(redirect)).await;
                                None
                            }
                        };

                        if let Some(result) = exit_result {
                            break result;
                        }
                    }
                }
            };

            server.unblock();
            result
        })
    };

    Ok(AnthropicLoginServer {
        auth_url,
        actual_port,
        server_handle,
        shutdown_handle: AnthropicShutdownHandle { shutdown_notify },
    })
}

enum HandledRequest {
    Response(Response<Cursor<Vec<u8>>>),
    RedirectWithHeader(Header),
    ResponseAndExit {
        headers: Vec<Header>,
        body: Vec<u8>,
        result: io::Result<()>,
    },
}

async fn process_request(
    url_raw: &str,
    opts: &AnthropicServerOptions,
    redirect_uri: &str,
    pkce: &PkceCodes,
    actual_port: u16,
    state: &str,
) -> HandledRequest {
    let parsed_url = match url::Url::parse(&format!("http://localhost{url_raw}")) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("URL parse error: {e}");
            return HandledRequest::Response(
                Response::from_string("Bad Request").with_status_code(400),
            );
        }
    };
    let path = parsed_url.path().to_string();

    match path.as_str() {
        "/callback" => {
            let params: std::collections::HashMap<String, String> =
                parsed_url.query_pairs().into_owned().collect();

            // Check for errors first
            if let Some(error) = params.get("error") {
                let error_desc = params
                    .get("error_description")
                    .cloned()
                    .unwrap_or_else(|| "Authentication failed".to_string());
                return HandledRequest::ResponseAndExit {
                    headers: match Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"text/html; charset=utf-8"[..],
                    ) {
                        Ok(header) => vec![header],
                        Err(_) => Vec::new(),
                    },
                    body: format!(
                        "<html><body><h1>Authentication Failed</h1><p>{}: {}</p></body></html>",
                        error, error_desc
                    )
                    .into_bytes(),
                    result: Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!("{}: {}", error, error_desc),
                    )),
                };
            }

            // Verify state
            if params.get("state").map(String::as_str) != Some(state) {
                return HandledRequest::Response(
                    Response::from_string("State mismatch").with_status_code(400),
                );
            }

            let code = match params.get("code") {
                Some(c) if !c.is_empty() => c.clone(),
                _ => {
                    return HandledRequest::Response(
                        Response::from_string("Missing authorization code").with_status_code(400),
                    );
                }
            };

            match exchange_code_for_tokens(&opts.issuer, &opts.client_id, redirect_uri, pkce, &code)
                .await
            {
                Ok(tokens) => {
                    // Persist the tokens
                    if let Err(err) =
                        persist_anthropic_tokens(&opts.config_dir, &tokens.access_token).await
                    {
                        eprintln!("Persist error: {err}");
                        return HandledRequest::Response(
                            Response::from_string(format!("Unable to persist auth: {err}"))
                                .with_status_code(500),
                        );
                    }

                    let success_url = format!("http://localhost:{actual_port}/success");
                    match tiny_http::Header::from_bytes(&b"Location"[..], success_url.as_bytes()) {
                        Ok(header) => HandledRequest::RedirectWithHeader(header),
                        Err(_) => HandledRequest::Response(
                            Response::from_string("Internal Server Error").with_status_code(500),
                        ),
                    }
                }
                Err(err) => {
                    eprintln!("Token exchange error: {err}");
                    HandledRequest::Response(
                        Response::from_string(format!("Token exchange failed: {err}"))
                            .with_status_code(500),
                    )
                }
            }
        }
        "/success" => {
            let body = include_str!("assets/anthropic_success.html");
            HandledRequest::ResponseAndExit {
                headers: match Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"text/html; charset=utf-8"[..],
                ) {
                    Ok(header) => vec![header],
                    Err(_) => Vec::new(),
                },
                body: body.as_bytes().to_vec(),
                result: Ok(()),
            }
        }
        "/cancel" => HandledRequest::ResponseAndExit {
            headers: Vec::new(),
            body: b"Login cancelled".to_vec(),
            result: Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Login cancelled",
            )),
        },
        _ => HandledRequest::Response(Response::from_string("Not Found").with_status_code(404)),
    }
}

fn send_response_with_disconnect(
    req: Request,
    mut headers: Vec<Header>,
    body: Vec<u8>,
) -> io::Result<()> {
    let status = StatusCode(200);
    let mut writer = req.into_writer();
    let reason = status.default_reason_phrase();
    write!(writer, "HTTP/1.1 {} {}\r\n", status.0, reason)?;
    headers.retain(|h| !h.field.equiv("Connection"));
    if let Ok(close_header) = Header::from_bytes(&b"Connection"[..], &b"close"[..]) {
        headers.push(close_header);
    }

    let content_length_value = format!("{}", body.len());
    if let Ok(content_length_header) =
        Header::from_bytes(&b"Content-Length"[..], content_length_value.as_bytes())
    {
        headers.push(content_length_header);
    }

    for header in headers {
        write!(
            writer,
            "{}: {}\r\n",
            header.field.as_str(),
            header.value.as_str()
        )?;
    }

    writer.write_all(b"\r\n")?;
    writer.write_all(&body)?;
    writer.flush()
}

fn build_authorize_url(
    issuer: &str,
    client_id: &str,
    redirect_uri: &str,
    pkce: &PkceCodes,
    state: &str,
) -> String {
    let query = vec![
        ("response_type".to_string(), "code".to_string()),
        ("client_id".to_string(), client_id.to_string()),
        ("redirect_uri".to_string(), redirect_uri.to_string()),
        ("scope".to_string(), "openid profile email".to_string()),
        (
            "code_challenge".to_string(),
            pkce.code_challenge.to_string(),
        ),
        ("code_challenge_method".to_string(), "S256".to_string()),
        ("state".to_string(), state.to_string()),
    ];
    let qs = query
        .into_iter()
        .map(|(k, v)| format!("{k}={}", urlencoding::encode(&v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{issuer}/oauth/authorize?{qs}")
}

fn generate_state() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn send_cancel_request(port: u16) -> io::Result<()> {
    let addr: SocketAddr = format!("127.0.0.1:{port}")
        .parse()
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;

    stream.write_all(b"GET /cancel HTTP/1.1\r\n")?;
    stream.write_all(format!("Host: 127.0.0.1:{port}\r\n").as_bytes())?;
    stream.write_all(b"Connection: close\r\n\r\n")?;

    let mut buf = [0u8; 64];
    let _ = stream.read(&mut buf);
    Ok(())
}

fn bind_server(port: u16) -> io::Result<Server> {
    let bind_address = format!("127.0.0.1:{port}");
    let mut cancel_attempted = false;
    let mut attempts = 0;
    const MAX_ATTEMPTS: u32 = 10;
    const RETRY_DELAY: Duration = Duration::from_millis(200);

    loop {
        match Server::http(&bind_address) {
            Ok(server) => return Ok(server),
            Err(err) => {
                attempts += 1;
                let is_addr_in_use = err
                    .downcast_ref::<io::Error>()
                    .map(|io_err| io_err.kind() == io::ErrorKind::AddrInUse)
                    .unwrap_or(false);

                if is_addr_in_use {
                    if !cancel_attempted {
                        cancel_attempted = true;
                        if let Err(cancel_err) = send_cancel_request(port) {
                            eprintln!("Failed to cancel previous login server: {cancel_err}");
                        }
                    }

                    thread::sleep(RETRY_DELAY);

                    if attempts >= MAX_ATTEMPTS {
                        return Err(io::Error::new(
                            io::ErrorKind::AddrInUse,
                            format!("Port {bind_address} is already in use"),
                        ));
                    }

                    continue;
                }

                return Err(io::Error::other(err));
            }
        }
    }
}

struct ExchangedTokens {
    access_token: String,
    #[allow(dead_code)]
    refresh_token: Option<String>,
}

async fn exchange_code_for_tokens(
    issuer: &str,
    client_id: &str,
    redirect_uri: &str,
    pkce: &PkceCodes,
    code: &str,
) -> io::Result<ExchangedTokens> {
    #[derive(serde::Deserialize)]
    struct TokenResponse {
        access_token: String,
        refresh_token: Option<String>,
    }

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{issuer}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
            urlencoding::encode(code),
            urlencoding::encode(redirect_uri),
            urlencoding::encode(client_id),
            urlencoding::encode(&pkce.code_verifier)
        ))
        .send()
        .await
        .map_err(io::Error::other)?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(io::Error::other(format!(
            "token endpoint returned status {}: {}",
            status, body
        )));
    }

    let tokens: TokenResponse = resp.json().await.map_err(io::Error::other)?;
    Ok(ExchangedTokens {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
    })
}

async fn persist_anthropic_tokens(config_dir: &Path, access_token: &str) -> io::Result<()> {
    // Claude Code stores credentials in ~/.claude/credentials.json
    // We'll store the API key that was obtained via OAuth
    let claude_dir = config_dir.join(".claude");
    tokio::fs::create_dir_all(&claude_dir).await?;

    let credentials_file = claude_dir.join("credentials.json");
    let credentials = serde_json::json!({
        "claudeAiOauth": {
            "accessToken": access_token,
            "expiresAt": null
        }
    });

    tokio::fs::write(
        &credentials_file,
        serde_json::to_string_pretty(&credentials).map_err(io::Error::other)?,
    )
    .await
}
