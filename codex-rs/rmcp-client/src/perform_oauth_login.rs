use std::collections::HashMap;
use std::string::String;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use reqwest::ClientBuilder;
use rmcp::transport::auth::OAuthState;
use tiny_http::Response;
use tiny_http::Server;
use tokio::sync::oneshot;
use tokio::time::timeout;
use urlencoding::decode;

use crate::OAuthCredentialsStoreMode;
use crate::StoredOAuthTokens;
use crate::WrappedOAuthTokenResponse;
use crate::oauth::compute_expires_at_millis;
use crate::save_oauth_tokens;
use crate::utils::apply_default_headers;
use crate::utils::build_default_headers;

struct CallbackServerGuard {
    server: Arc<Server>,
}

impl Drop for CallbackServerGuard {
    fn drop(&mut self) {
        self.server.unblock();
    }
}

/// Handle for an in-progress OAuth login flow that can be cancelled.
///
/// The caller takes ownership of both the `cancel_tx` (to cancel the flow)
/// and the `task` (to await completion). There is no Drop implementation;
/// the caller is responsible for managing both halves.
pub struct OAuthLoginHandle {
    /// Send to cancel the OAuth flow.
    pub cancel_tx: Option<oneshot::Sender<()>>,
    /// The task running the flow.
    pub task: tokio::task::JoinHandle<Result<()>>,
}

/// Start an async MCP OAuth login flow that can be cancelled.
///
/// Opens the browser for the user and waits for the callback. The returned
/// handle can be used to cancel the flow. Call `.task` to await completion.
pub async fn start_oauth_login(
    server_name: String,
    server_url: String,
    store_mode: OAuthCredentialsStoreMode,
    http_headers: Option<HashMap<String, String>>,
    env_http_headers: Option<HashMap<String, String>>,
    scopes: Vec<String>,
) -> Result<OAuthLoginHandle> {
    let server = Arc::new(Server::http("127.0.0.1:0").map_err(|err| anyhow!(err))?);
    let _guard = CallbackServerGuard {
        server: Arc::clone(&server),
    };

    let redirect_uri = match server.server_addr() {
        tiny_http::ListenAddr::IP(std::net::SocketAddr::V4(addr)) => {
            format!("http://{}:{}/callback", addr.ip(), addr.port())
        }
        tiny_http::ListenAddr::IP(std::net::SocketAddr::V6(addr)) => {
            format!("http://[{}]:{}/callback", addr.ip(), addr.port())
        }
        #[cfg(not(target_os = "windows"))]
        _ => return Err(anyhow!("unable to determine callback address")),
    };

    let (callback_tx, callback_rx) = oneshot::channel();
    spawn_callback_server(server, callback_tx);

    let default_headers = build_default_headers(http_headers, env_http_headers)?;
    let http_client = apply_default_headers(ClientBuilder::new(), &default_headers).build()?;

    let mut oauth_state = OAuthState::new(&server_url, Some(http_client)).await?;
    let scope_refs: Vec<&str> = scopes.iter().map(String::as_str).collect();
    oauth_state
        .start_authorization(&scope_refs, &redirect_uri, Some("Codex"))
        .await?;
    let auth_url = oauth_state.get_authorization_url().await?;

    tracing::info!("MCP OAuth: opening browser for {server_name}: {auth_url}");
    if webbrowser::open(&auth_url).is_err() {
        tracing::warn!("MCP OAuth: browser launch failed for {server_name}");
    }

    let (cancel_tx, cancel_rx) = oneshot::channel::<()>();

    let task = tokio::spawn(async move {
        let _guard = _guard; // move guard into the task to keep server alive

        let cancel = async {
            let _ = cancel_rx.await;
        };

        let (code, csrf_state) =
            wait_for_callback_or_cancel(callback_rx, cancel, Duration::from_secs(300)).await?;

        oauth_state
            .handle_callback(&code, &csrf_state)
            .await
            .context("failed to handle OAuth callback")?;

        let (client_id, credentials_opt) = oauth_state
            .get_credentials()
            .await
            .context("failed to retrieve OAuth credentials")?;
        let credentials =
            credentials_opt.ok_or_else(|| anyhow!("OAuth provider did not return credentials"))?;

        let expires_at = compute_expires_at_millis(&credentials);
        let stored = StoredOAuthTokens {
            server_name,
            url: server_url,
            client_id,
            token_response: WrappedOAuthTokenResponse(credentials),
            expires_at,
        };
        save_oauth_tokens(&stored.server_name, &stored, store_mode)?;

        Ok(())
    });

    Ok(OAuthLoginHandle {
        cancel_tx: Some(cancel_tx),
        task,
    })
}

pub async fn perform_oauth_login(
    server_name: &str,
    server_url: &str,
    store_mode: OAuthCredentialsStoreMode,
    http_headers: Option<HashMap<String, String>>,
    env_http_headers: Option<HashMap<String, String>>,
    scopes: &[String],
) -> Result<()> {
    let server = Arc::new(Server::http("127.0.0.1:0").map_err(|err| anyhow!(err))?);
    let guard = CallbackServerGuard {
        server: Arc::clone(&server),
    };

    let redirect_uri = match server.server_addr() {
        tiny_http::ListenAddr::IP(std::net::SocketAddr::V4(addr)) => {
            format!("http://{}:{}/callback", addr.ip(), addr.port())
        }
        tiny_http::ListenAddr::IP(std::net::SocketAddr::V6(addr)) => {
            format!("http://[{}]:{}/callback", addr.ip(), addr.port())
        }
        #[cfg(not(target_os = "windows"))]
        _ => return Err(anyhow!("unable to determine callback address")),
    };

    let (tx, rx) = oneshot::channel();
    spawn_callback_server(server, tx);

    let default_headers = build_default_headers(http_headers, env_http_headers)?;
    let http_client = apply_default_headers(ClientBuilder::new(), &default_headers).build()?;

    let mut oauth_state = OAuthState::new(server_url, Some(http_client)).await?;
    let scope_refs: Vec<&str> = scopes.iter().map(String::as_str).collect();
    oauth_state
        .start_authorization(&scope_refs, &redirect_uri, Some("Codex"))
        .await?;
    let auth_url = oauth_state.get_authorization_url().await?;

    println!("Authorize `{server_name}` by opening this URL in your browser:\n{auth_url}\n");

    if webbrowser::open(&auth_url).is_err() {
        println!("(Browser launch failed; please copy the URL above manually.)");
    }

    println!("Press Enter to cancel...\n");

    let cancel = async {
        let mut buf = String::new();
        let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
        let _ = tokio::io::AsyncBufReadExt::read_line(&mut stdin, &mut buf).await;
    };

    let (code, csrf_state) =
        wait_for_callback_or_cancel(rx, cancel, Duration::from_secs(300)).await?;

    oauth_state
        .handle_callback(&code, &csrf_state)
        .await
        .context("failed to handle OAuth callback")?;

    let (client_id, credentials_opt) = oauth_state
        .get_credentials()
        .await
        .context("failed to retrieve OAuth credentials")?;
    let credentials =
        credentials_opt.ok_or_else(|| anyhow!("OAuth provider did not return credentials"))?;

    let expires_at = compute_expires_at_millis(&credentials);
    let stored = StoredOAuthTokens {
        server_name: server_name.to_string(),
        url: server_url.to_string(),
        client_id,
        token_response: WrappedOAuthTokenResponse(credentials),
        expires_at,
    };
    save_oauth_tokens(server_name, &stored, store_mode)?;

    drop(guard);
    Ok(())
}

fn spawn_callback_server(server: Arc<Server>, tx: oneshot::Sender<(String, String)>) {
    tokio::task::spawn_blocking(move || {
        while let Ok(request) = server.recv() {
            let path = request.url().to_string();
            if let Some(OauthCallbackResult { code, state }) = parse_oauth_callback(&path) {
                let response =
                    Response::from_string("Authentication complete. You may close this window.");
                if let Err(err) = request.respond(response) {
                    eprintln!("Failed to respond to OAuth callback: {err}");
                }
                if let Err(err) = tx.send((code, state)) {
                    eprintln!("Failed to send OAuth callback: {err:?}");
                }
                break;
            } else {
                let response =
                    Response::from_string("Invalid OAuth callback").with_status_code(400);
                if let Err(err) = request.respond(response) {
                    eprintln!("Failed to respond to OAuth callback: {err}");
                }
            }
        }
    });
}

struct OauthCallbackResult {
    code: String,
    state: String,
}

fn parse_oauth_callback(path: &str) -> Option<OauthCallbackResult> {
    let (route, query) = path.split_once('?')?;
    if route != "/callback" {
        return None;
    }

    let mut code = None;
    let mut state = None;

    for pair in query.split('&') {
        let (key, value) = pair.split_once('=')?;
        let decoded = decode(value).ok()?.into_owned();
        match key {
            "code" => code = Some(decoded),
            "state" => state = Some(decoded),
            _ => {}
        }
    }

    Some(OauthCallbackResult {
        code: code?,
        state: state?,
    })
}

/// Wait for the OAuth callback or a cancellation signal, with a timeout.
/// Returns the (code, csrf_state) pair from the callback, or an error if
/// cancelled or timed out.
async fn wait_for_callback_or_cancel(
    rx: oneshot::Receiver<(String, String)>,
    cancel: impl std::future::Future<Output = ()>,
    timeout_duration: Duration,
) -> Result<(String, String)> {
    tokio::select! {
        // Prefer callback over cancel when both are ready simultaneously.
        biased;
        result = timeout(timeout_duration, rx) => {
            result
                .context("timed out waiting for OAuth callback")?
                .context("OAuth callback was cancelled")
        }
        _ = cancel => {
            Err(anyhow!("OAuth login cancelled by user"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn cancel_signal_aborts_callback_wait() {
        let (_callback_tx, callback_rx) = oneshot::channel::<(String, String)>();

        // Cancel resolves immediately.
        let cancel = std::future::ready(());

        let result =
            wait_for_callback_or_cancel(callback_rx, cancel, Duration::from_secs(300)).await;

        assert_eq!(
            result.unwrap_err().to_string(),
            "OAuth login cancelled by user"
        );
    }

    #[tokio::test]
    async fn callback_returns_code_and_state() {
        let (callback_tx, callback_rx) = oneshot::channel::<(String, String)>();

        callback_tx
            .send(("auth_code_123".to_string(), "csrf_state_abc".to_string()))
            .unwrap();

        // Cancel never resolves.
        let cancel = std::future::pending();

        let result =
            wait_for_callback_or_cancel(callback_rx, cancel, Duration::from_secs(300)).await;

        let (code, state) = result.unwrap();
        assert_eq!(code, "auth_code_123");
        assert_eq!(state, "csrf_state_abc");
    }

    #[tokio::test]
    async fn timeout_returns_error_when_no_callback_or_cancel() {
        let (_callback_tx, callback_rx) = oneshot::channel::<(String, String)>();

        // Cancel never resolves.
        let cancel = std::future::pending();

        let result =
            wait_for_callback_or_cancel(callback_rx, cancel, Duration::from_millis(10)).await;

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("timed out waiting for OAuth callback"),
        );
    }
}
