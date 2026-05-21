use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

const CLOUD_AUTH_FILE: &str = "cloud-auth.json";

#[derive(Debug, Clone)]
pub struct CloudConnectionInfo {
    pub ws_url: String,
    pub auth_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudCredentials {
    pub broker_url: String,
    pub auth_token: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SessionInfo {
    pub session_id: String,
    pub ws_url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    #[error("authentication required")]
    AuthRequired,

    #[error("token expired")]
    TokenExpired,

    #[error("session acquisition failed: HTTP {status}: {body}")]
    AcquireFailed { status: u16, body: String },

    #[error("network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("invalid token: {0}")]
    InvalidToken(String),
}

pub struct BrokerClient {
    broker_url: String,
    auth_token: Option<String>,
    nori_home: PathBuf,
    http: reqwest::Client,
}

impl BrokerClient {
    pub fn new(broker_url: String, nori_home: PathBuf) -> Self {
        let creds = load_credentials(&nori_home);
        let auth_token = creds
            .filter(|c| c.broker_url == broker_url)
            .map(|c| c.auth_token);

        Self {
            broker_url,
            auth_token,
            nori_home,
            http: reqwest::Client::new(),
        }
    }

    pub fn auth_token(&self) -> Option<&str> {
        self.auth_token.as_deref()
    }

    pub fn has_valid_token(&self) -> bool {
        self.auth_token
            .as_deref()
            .is_some_and(|t| !is_token_expired(t))
    }

    pub async fn acquire_session(&self) -> std::result::Result<SessionInfo, BrokerError> {
        let token = self
            .auth_token
            .as_deref()
            .ok_or(BrokerError::AuthRequired)?;

        if is_token_expired(token) {
            return Err(BrokerError::TokenExpired);
        }

        let url = format!("{}/api/sessions/acquire", self.broker_url);
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status == 401 {
            return Err(BrokerError::TokenExpired);
        }
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(BrokerError::AcquireFailed { status, body });
        }

        let info: SessionInfo = resp
            .json()
            .await
            .map_err(|e| BrokerError::InvalidToken(format!("invalid response: {e}")))?;
        Ok(info)
    }

    pub async fn authenticate(&mut self) -> Result<()> {
        let server = tiny_http::Server::http("127.0.0.1:0")
            .map_err(|e| anyhow::anyhow!("failed to bind local server: {e}"))?;
        let port = server
            .server_addr()
            .to_ip()
            .ok_or_else(|| anyhow::anyhow!("server did not bind to an IP address"))?
            .port();

        let auth_url = format!(
            "{}/auth/cli?redirect_uri=http://localhost:{port}/callback",
            self.broker_url
        );

        if webbrowser::open(&auth_url).is_err() {
            eprintln!("Could not open browser. Please visit:\n{auth_url}");
        }

        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        std::thread::spawn(move || {
            while let Ok(request) = server.recv() {
                let url = request.url().to_string();
                if url.starts_with("/callback") {
                    let token = extract_token_from_callback(&url);
                    let html = if token.is_some() {
                        "<html><body><h1>Authentication successful!</h1><p>You can close this tab.</p></body></html>"
                    } else {
                        "<html><body><h1>Authentication failed</h1><p>No token received.</p></body></html>"
                    };
                    let mut response = tiny_http::Response::from_string(html);
                    if let Ok(header) = "Content-Type: text/html".parse::<tiny_http::Header>() {
                        response = response.with_header(header);
                    }
                    let _ = request.respond(response);
                    if let Some(t) = token {
                        let _ = tx.send(t);
                    }
                    server.unblock();
                    break;
                } else {
                    let _ = request.respond(
                        tiny_http::Response::from_string("Not found").with_status_code(404),
                    );
                }
            }
        });

        let token = rx
            .await
            .map_err(|_| anyhow::anyhow!("authentication callback was not received"))?;

        self.auth_token = Some(token.clone());
        save_credentials(
            &self.nori_home,
            &CloudCredentials {
                broker_url: self.broker_url.clone(),
                auth_token: token,
            },
        )?;

        Ok(())
    }
}

pub fn is_token_expired(token: &str) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return true;
    }

    let payload =
        match base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, parts[1]) {
            Ok(bytes) => bytes,
            Err(_) => return true,
        };

    let claims: serde_json::Value = match serde_json::from_slice(&payload) {
        Ok(v) => v,
        Err(_) => return true,
    };

    let exp = match claims.get("exp").and_then(serde_json::Value::as_i64) {
        Some(e) => e,
        None => return true,
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    exp <= now
}

pub fn load_credentials(nori_home: &Path) -> Option<CloudCredentials> {
    let path = nori_home.join(CLOUD_AUTH_FILE);
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn save_credentials(nori_home: &Path, creds: &CloudCredentials) -> Result<()> {
    std::fs::create_dir_all(nori_home)?;
    let path = nori_home.join(CLOUD_AUTH_FILE);
    let json = serde_json::to_string_pretty(creds)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn extract_token_from_callback(url_path: &str) -> Option<String> {
    let full = format!("http://localhost{url_path}");
    let parsed = url::Url::parse(&full).ok()?;
    parsed
        .query_pairs()
        .find(|(k, _)| k == "token")
        .map(|(_, v)| v.into_owned())
}

#[cfg(test)]
mod tests;
