//! Authenticated product analytics for meaningful Nori agent sessions.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use chrono::SecondsFormat;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use crate::read_install_state;

pub const ANALYTICS_OPT_OUT_ENV: &str = "NORI_NO_ANALYTICS";

const ANALYTICS_URL_ENV: &str = "NORI_ANALYTICS_URL";
const DEFAULT_ANALYTICS_URL: &str = "https://login.norisessions.com/api/analytics/v1/events";
const FIREBASE_TOKEN_URL: &str =
    "https://securetoken.googleapis.com/v1/token?key=AIzaSyC54HqlGrkyANVFKGDQi3LobO5moDOuafk";
const REQUEST_DEADLINE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    Interactive,
    Cloud,
    Exec,
    Acp,
}

impl SessionMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Cloud => "cloud",
            Self::Exec => "exec",
            Self::Acp => "acp",
        }
    }
}

#[derive(Clone)]
pub struct AnalyticsReporter {
    inner: Arc<ReporterInner>,
}

struct ReporterInner {
    mode: SessionMode,
    nori_home: PathBuf,
    config_root: Option<PathBuf>,
    pending: Mutex<Vec<mpsc::Receiver<()>>>,
}

impl std::fmt::Debug for AnalyticsReporter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AnalyticsReporter")
            .field("mode", &self.inner.mode)
            .finish_non_exhaustive()
    }
}

impl AnalyticsReporter {
    pub fn new(mode: SessionMode, nori_home: PathBuf) -> Self {
        Self {
            inner: Arc::new(ReporterInner {
                mode,
                nori_home,
                config_root: dirs::home_dir(),
                pending: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn attach(
        &self,
        handle: nori_harness::runtime::HarnessHandle,
    ) -> nori_harness::runtime::HarnessHandle {
        let reporter = self.clone();
        handle.with_first_prompt_started_callback(move || reporter.capture_session_started())
    }

    pub fn flush(&self) {
        let receivers = self
            .inner
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain(..)
            .collect::<Vec<_>>();
        let deadline = Instant::now() + REQUEST_DEADLINE;
        for receiver in receivers {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || receiver.recv_timeout(remaining).is_err() {
                break;
            }
        }
    }

    fn capture_session_started(&self) {
        let Some(endpoint) = analytics_endpoint() else {
            return;
        };
        let mode = self.inner.mode;
        let nori_home = self.inner.nori_home.clone();
        let config_root = self.inner.config_root.clone();
        let (completion_tx, completion_rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("nori-analytics".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                if let Ok(runtime) = runtime {
                    let _ = runtime.block_on(async {
                        tokio::time::timeout(
                            REQUEST_DEADLINE,
                            send_session_started(
                                mode,
                                &nori_home,
                                config_root.as_deref(),
                                &endpoint,
                            ),
                        )
                        .await
                    });
                }
                let _ = completion_tx.send(());
            });
        if spawned.is_ok() {
            self.inner
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(completion_rx);
        }
    }
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
    auth: Option<AuthSection>,
    username: Option<String>,
    #[serde(rename = "refreshToken")]
    refresh_token: Option<String>,
}

impl ConfigFile {
    fn effective_auth(self) -> Option<AuthSection> {
        self.auth.or_else(|| {
            (self.username.is_some() || self.refresh_token.is_some()).then_some(AuthSection {
                username: self.username,
                id_token: None,
                id_token_expires_at: None,
                refresh_token: self.refresh_token,
            })
        })
    }
}

#[derive(Debug, Deserialize)]
struct AuthSection {
    username: Option<String>,
    #[serde(rename = "idToken")]
    id_token: Option<String>,
    #[serde(rename = "idTokenExpiresAt")]
    id_token_expires_at: Option<i64>,
    #[serde(rename = "refreshToken")]
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    id_token: String,
}

#[derive(Debug, Serialize)]
struct AnalyticsEnvelope<'a> {
    schema_version: i64,
    event: &'static str,
    activity_id: Uuid,
    occurred_at: String,
    product: &'static str,
    surface: &'static str,
    app_version: &'static str,
    properties: SessionProperties<'a>,
}

#[derive(Debug, Serialize)]
struct SessionProperties<'a> {
    session_mode: &'a str,
}

async fn send_session_started(
    mode: SessionMode,
    nori_home: &Path,
    config_root: Option<&Path>,
    endpoint: &str,
) -> anyhow::Result<()> {
    if should_skip_analytics(nori_home) {
        return Ok(());
    }
    let Some(config_root) = config_root else {
        return Ok(());
    };
    let raw = tokio::fs::read_to_string(config_root.join(".nori-config.json")).await?;
    let Some(auth) = serde_json::from_str::<ConfigFile>(&raw)?.effective_auth() else {
        return Ok(());
    };
    let username = auth.username.as_deref().map(str::trim).unwrap_or_default();
    if !is_human_username(username) {
        return Ok(());
    }

    let client = reqwest13::Client::builder()
        .timeout(REQUEST_DEADLINE)
        .build()?;
    let token = match fresh_id_token(&auth) {
        Some(token) => token.to_string(),
        None => {
            let Some(refresh_token) = auth
                .refresh_token
                .as_deref()
                .filter(|token| !token.is_empty())
            else {
                return Ok(());
            };
            refresh_id_token(&client, refresh_token, FIREBASE_TOKEN_URL).await?
        }
    };
    let envelope = AnalyticsEnvelope {
        schema_version: 1,
        event: "nori_agent_session_started",
        activity_id: Uuid::new_v4(),
        occurred_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        product: "nori",
        surface: "cli",
        app_version: crate::CLI_VERSION,
        properties: SessionProperties {
            session_mode: mode.as_str(),
        },
    };
    let _ = client
        .post(endpoint)
        .bearer_auth(token)
        .json(&envelope)
        .send()
        .await?;
    Ok(())
}

fn analytics_endpoint() -> Option<String> {
    if let Ok(endpoint) = std::env::var(ANALYTICS_URL_ENV)
        && !endpoint.is_empty()
    {
        return Some(endpoint);
    }
    if cfg!(debug_assertions) {
        None
    } else {
        Some(DEFAULT_ANALYTICS_URL.to_string())
    }
}

fn should_skip_analytics(nori_home: &Path) -> bool {
    if std::env::var(ANALYTICS_OPT_OUT_ENV).as_deref() == Ok("1") {
        return true;
    }
    if read_install_state(nori_home).is_some_and(|state| state.opt_out) {
        return true;
    }
    is_ci_env() && std::env::var_os(ANALYTICS_URL_ENV).is_none()
}

fn is_human_username(username: &str) -> bool {
    username.contains('@') && !username.to_ascii_lowercase().starts_with("nori-service:")
}

fn fresh_id_token(auth: &AuthSection) -> Option<&str> {
    let expires_at = auth.id_token_expires_at?;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_millis();
    let now = i64::try_from(now).ok()?;
    (expires_at > now)
        .then_some(auth.id_token.as_deref())
        .flatten()
        .filter(|token| !token.is_empty())
}

async fn refresh_id_token(
    client: &reqwest13::Client,
    refresh_token: &str,
    endpoint: &str,
) -> anyhow::Result<String> {
    let response = client
        .post(endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await?
        .error_for_status()?
        .json::<RefreshResponse>()
        .await?;
    Ok(response.id_token)
}

pub fn is_ci_env() -> bool {
    std::env::var("CI")
        .map(|value| value != "0" && !value.is_empty())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::io::Write;
    use std::net::TcpListener;

    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn human_identity_gate_excludes_service_and_malformed_names() {
        assert!(is_human_username("person@example.com"));
        assert!(!is_human_username("nori-service:acme"));
        assert!(!is_human_username("not-an-email"));
    }

    #[test]
    fn config_prefers_nested_auth_credentials() {
        let config = serde_json::from_value::<ConfigFile>(serde_json::json!({
            "auth": {
                "username": "nested@example.com",
                "refreshToken": "nested-refresh"
            },
            "username": "legacy@example.com",
            "refreshToken": "legacy-refresh"
        }))
        .expect("parse config");

        let auth = config.effective_auth().expect("effective auth");
        assert_eq!(auth.username.as_deref(), Some("nested@example.com"));
        assert_eq!(auth.refresh_token.as_deref(), Some("nested-refresh"));
    }

    #[test]
    fn config_accepts_legacy_top_level_credentials() {
        let config = serde_json::from_value::<ConfigFile>(serde_json::json!({
            "username": "legacy@example.com",
            "refreshToken": "legacy-refresh"
        }))
        .expect("parse config");

        let auth = config.effective_auth().expect("effective auth");
        assert_eq!(auth.username.as_deref(), Some("legacy@example.com"));
        assert_eq!(auth.refresh_token.as_deref(), Some("legacy-refresh"));
        assert_eq!(auth.id_token.as_deref(), None);
        assert_eq!(auth.id_token_expires_at, None);
    }

    #[tokio::test]
    async fn refresh_token_exchange_returns_a_firebase_id_token() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind token endpoint");
        let endpoint = format!("http://{}", listener.local_addr().expect("token address"));
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept token request");
            let mut request = vec![0_u8; 4096];
            let count = stream.read(&mut request).expect("read token request");
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.starts_with("POST / HTTP/1.1"));
            assert!(request.contains("grant_type=refresh_token&refresh_token=refresh-value"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 30\r\nConnection: close\r\n\r\n{\"id_token\":\"refreshed-token\"}",
                )
                .expect("write token response");
        });

        let client = reqwest13::Client::new();
        let token = refresh_id_token(&client, "refresh-value", &endpoint)
            .await
            .expect("refresh ID token");
        assert_eq!(token, "refreshed-token");
        server.join().expect("token server");
    }
}
