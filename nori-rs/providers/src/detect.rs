use std::path::Path;
use std::path::PathBuf;

use nori_acp_host::registry::AgentKind;
use serde::Serialize;
use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;

use crate::saved::SavedKeyIndex;
use crate::saved::StoreLocation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Linux,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }
}

/// Answers whether a macOS keychain generic-password item exists, without
/// reading its secret.
pub trait KeychainProbe {
    fn has_generic_password(&self, service: &str) -> bool;
}

/// Probes the login keychain with `security find-generic-password`, which
/// prints only item attributes unless asked for the password.
#[derive(Debug, Default)]
pub struct SystemKeychain;

impl KeychainProbe for SystemKeychain {
    fn has_generic_password(&self, service: &str) -> bool {
        cfg!(target_os = "macos")
            && std::process::Command::new("/usr/bin/security")
                .args(["find-generic-password", "-s", service])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
    }
}

/// Everything detection reads, so callers can point it at a fixture home.
pub struct DetectContext<'a> {
    pub home: &'a Path,
    pub env: &'a dyn Fn(&str) -> Option<String>,
    pub platform: Platform,
    pub keychain: &'a dyn KeychainProbe,
    pub saved: &'a SavedKeyIndex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProvidersReport {
    pub agents: Vec<AgentAuthStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentAuthStatus {
    pub agent: String,
    pub name: String,
    pub status: AuthStatus,
    /// Found credentials, in the order the agent prefers them; the first one
    /// is the one the agent will use.
    pub credentials: Vec<DetectedCredential>,
    /// The variable a key saved through Nori is passed in, if the agent takes one.
    pub saved_key_env: Option<&'static str>,
    pub hint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthStatus {
    SignedIn,
    SignedOut,
    /// Nothing Nori can see, but the agent keeps its own sign-in somewhere
    /// Nori does not inspect.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DetectedCredential {
    pub kind: CredentialKind,
    pub source: CredentialSource,
    /// Non-secret detail such as the subscription plan or a provider name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    /// A consumer subscription sign-in (Claude.ai, ChatGPT, ...).
    Subscription,
    ApiKey,
    /// Cloud-provider routing such as Bedrock or Vertex AI.
    Cloud,
    /// A bearer token supplied directly.
    Token,
    /// The agent's own stored sign-in whose type Nori cannot see.
    Login,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CredentialSource {
    Env { name: String },
    File { path: PathBuf },
    Keychain { service: String },
    NoriSaved { env: String, store: StoreLocation },
}

pub fn detect_all(ctx: &DetectContext<'_>) -> ProvidersReport {
    ProvidersReport {
        agents: AgentKind::all()
            .iter()
            .map(|agent| detect_agent(*agent, ctx))
            .collect(),
    }
}

pub fn detect_agent(agent: AgentKind, ctx: &DetectContext<'_>) -> AgentAuthStatus {
    let mut found = Found {
        ctx,
        agent,
        credentials: Vec::new(),
    };
    // Agents that keep a sign-in Nori cannot inspect report `Unknown` rather
    // than `SignedOut` when nothing visible turns up.
    for credential in crate::env_credentials(agent) {
        if credential.is_set(ctx.env) {
            found.push(
                credential.kind,
                CredentialSource::Env {
                    name: credential.name.to_string(),
                },
                None,
            );
        }
    }
    found.saved();
    // Agents that keep a sign-in Nori cannot inspect report `Unknown` rather
    // than `SignedOut` when nothing visible turns up.
    let opaque = match agent {
        AgentKind::ClaudeCode => {
            claude(&mut found);
            false
        }
        AgentKind::Codex => codex(&mut found),
        AgentKind::OpenCode => {
            let data = found
                .env_path("XDG_DATA_HOME")
                .unwrap_or_else(|| ctx.home.join(".local/share"));
            found.provider_map(data.join("opencode/auth.json"));
            true
        }
        AgentKind::Pi => {
            found.provider_map(ctx.home.join(".pi/agent/auth.json"));
            true
        }
        AgentKind::Gemini | AgentKind::GrokBuild | AgentKind::Cursor | AgentKind::Hermes => true,
    };
    let status = match (found.credentials.is_empty(), opaque) {
        (false, _) => AuthStatus::SignedIn,
        (true, true) => AuthStatus::Unknown,
        (true, false) => AuthStatus::SignedOut,
    };
    AgentAuthStatus {
        agent: agent.slug().to_string(),
        name: agent.display_name().to_string(),
        status,
        credentials: found.credentials,
        saved_key_env: crate::saved_key_env(agent),
        hint: agent.auth_hint().to_string(),
    }
}

/// Claude Code's own `/login` credentials, after the environment.
fn claude(found: &mut Found<'_, '_>) {
    let ctx = found.ctx;
    let custom_dir = found.env_path("CLAUDE_CONFIG_DIR");
    if ctx.platform == Platform::MacOs {
        let service = match &custom_dir {
            Some(dir) => {
                let digest = Sha256::digest(dir.to_string_lossy().as_bytes());
                let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
                format!("Claude Code-credentials-{}", &hex[..8])
            }
            None => "Claude Code-credentials".to_string(),
        };
        found.keychain(&service, CredentialKind::Subscription);
    }
    let config_dir = custom_dir
        .clone()
        .unwrap_or_else(|| ctx.home.join(".claude"));
    let credentials_path = config_dir.join(".credentials.json");
    if let Some(oauth) = read_json(&credentials_path)
        .as_ref()
        .and_then(|json| json.get("claudeAiOauth"))
        .filter(|oauth| oauth.is_object())
    {
        let plan = oauth.get("subscriptionType").and_then(Value::as_str);
        found.push(
            CredentialKind::Subscription,
            CredentialSource::File {
                path: credentials_path,
            },
            plan,
        );
    }
    let global_config = custom_dir
        .unwrap_or_else(|| ctx.home.to_path_buf())
        .join(".claude.json");
    if read_json(&global_config)
        .as_ref()
        .and_then(|json| json.get("primaryApiKey"))
        .and_then(Value::as_str)
        .is_some_and(|key| !key.is_empty())
    {
        found.push(
            CredentialKind::ApiKey,
            CredentialSource::File {
                path: global_config,
            },
            None,
        );
    }
}

/// Codex's `auth.json` or the keychain item its `cli_auth_credentials_store`
/// setting selects. Nori launches Codex without `CODEX_HOME`, so only
/// `~/.codex` counts. Returns whether a keychain login might exist that Nori
/// cannot see.
fn codex(found: &mut Found<'_, '_>) -> bool {
    let ctx = found.ctx;
    let codex_home = ctx.home.join(".codex");
    let auth_path = codex_home.join("auth.json");
    if let Some(auth) = read_json(&auth_path).filter(Value::is_object) {
        let mode = auth.get("auth_mode").and_then(Value::as_str);
        let has_key = auth
            .get("OPENAI_API_KEY")
            .and_then(Value::as_str)
            .is_some_and(|key| !key.is_empty());
        let has_tokens = auth.get("tokens").is_some_and(Value::is_object);
        let credential = match mode {
            Some("apikey") => Some((CredentialKind::ApiKey, None)),
            Some(mode @ ("chatgpt" | "chatgptAuthTokens")) => {
                Some((CredentialKind::Subscription, Some(mode)))
            }
            Some(mode) => Some((CredentialKind::Token, Some(mode))),
            None if has_key => Some((CredentialKind::ApiKey, None)),
            None if has_tokens => Some((CredentialKind::Subscription, Some("chatgpt"))),
            None => None,
        };
        if let Some((kind, detail)) = credential {
            found.push(kind, CredentialSource::File { path: auth_path }, detail);
        }
    }
    let store = std::fs::read_to_string(codex_home.join("config.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|table| {
            table
                .get("cli_auth_credentials_store")
                .and_then(toml::Value::as_str)
                .map(str::to_string)
        });
    let uses_keyring = matches!(store.as_deref(), Some("keyring" | "auto"));
    match ctx.platform {
        Platform::MacOs if uses_keyring => {
            found.keychain("Codex Auth", CredentialKind::Login);
            false
        }
        Platform::MacOs => false,
        Platform::Linux => uses_keyring,
    }
}

struct Found<'c, 'a> {
    ctx: &'c DetectContext<'a>,
    agent: AgentKind,
    credentials: Vec<DetectedCredential>,
}

impl Found<'_, '_> {
    fn push(&mut self, kind: CredentialKind, source: CredentialSource, detail: Option<&str>) {
        self.credentials.push(DetectedCredential {
            kind,
            source,
            detail: detail.map(str::to_string),
        });
    }

    fn env_path(&self, name: &str) -> Option<PathBuf> {
        (self.ctx.env)(name)
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
    }

    fn saved(&mut self) {
        if let (Some(env), Some(store)) = (
            crate::saved_key_env(self.agent),
            self.ctx.saved.location(self.agent),
        ) {
            self.push(
                CredentialKind::ApiKey,
                CredentialSource::NoriSaved {
                    env: env.to_string(),
                    store,
                },
                None,
            );
        }
    }

    fn keychain(&mut self, service: &str, kind: CredentialKind) {
        if self.ctx.keychain.has_generic_password(service) {
            self.push(
                kind,
                CredentialSource::Keychain {
                    service: service.to_string(),
                },
                None,
            );
        }
    }

    /// An `auth.json` mapping provider ids to `{"type": ...}` entries, as
    /// OpenCode and Pi write them. Reports each provider, never its secret.
    fn provider_map(&mut self, path: PathBuf) {
        let Some(Value::Object(providers)) = read_json(&path) else {
            return;
        };
        let mut entries: Vec<_> = providers.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (provider, entry) in entries {
            let kind = match entry.get("type").and_then(Value::as_str) {
                Some("oauth") => CredentialKind::Subscription,
                Some("api" | "api_key") => CredentialKind::ApiKey,
                Some("wellknown") => CredentialKind::Token,
                _ => CredentialKind::Login,
            };
            self.push(
                kind,
                CredentialSource::File { path: path.clone() },
                Some(provider),
            );
        }
    }
}

/// Parse a credential file, treating unreadable or malformed files as absent.
/// The parsed value stays in memory only long enough to pick non-secret fields.
fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}
