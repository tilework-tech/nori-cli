use std::path::Path;
use std::path::PathBuf;

use nori_acp_host::registry::AgentKind;
use serde::Serialize;
use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;

use crate::KeyRoute;
use crate::Provider;
use crate::store::SavedIndex;
use crate::store::StoreBackend;

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
    pub saved: &'a SavedIndex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProvidersReport {
    /// Where saved credentials live (`NORI_CREDENTIALS_STORE`).
    pub store: StoreBackend,
    /// Why saved credentials could not be read, when they could not. The
    /// rest of the report is still accurate apart from saved keys.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_error: Option<String>,
    pub providers: Vec<ProviderStatus>,
    pub agents: Vec<AgentAuthStatus>,
}

/// A provider whose key can be saved through Nori.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderStatus {
    pub id: Provider,
    pub name: &'static str,
    /// The saved record's type (`{"type": "api_key"}`), or `null`.
    pub saved: Option<SavedRecord>,
    /// Agents a saved key reaches, and the variable each receives it in.
    pub agents: Vec<AgentKeyRoute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SavedRecord {
    #[serde(rename = "type")]
    pub record_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentKeyRoute {
    pub agent: &'static str,
    pub env: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderKeyRoute {
    pub provider: Provider,
    pub env: &'static str,
}

/// Something the app should point out, keyed by a stable `code`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notice {
    /// `api_key_overrides_subscription`: the credential the agent will use is
    /// an API key or token, ranked above a subscription sign-in, so usage is
    /// billed to the API account rather than the plan.
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentAuthStatus {
    pub agent: String,
    pub name: String,
    pub status: AuthStatus,
    /// Found credentials, in the order the agent prefers them. For agents that
    /// talk to one provider the first is the one the agent will use; for
    /// OpenCode and Pi the order holds per `provider`.
    pub credentials: Vec<DetectedCredential>,
    /// Providers whose saved key this agent receives, and the variable each is
    /// passed in. Empty when the agent only signs in through its own login.
    pub saved_keys: Vec<ProviderKeyRoute>,
    pub notices: Vec<Notice>,
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
    /// The model provider the credential is for, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Non-secret detail such as the subscription plan or Codex auth mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// True unless Nori saved the credential and can remove it.
    pub read_only: bool,
    /// How to disconnect it, e.g. `unset OPENAI_API_KEY to disconnect`.
    pub disconnect: String,
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
    Env {
        name: String,
    },
    File {
        path: PathBuf,
    },
    Keychain {
        service: String,
    },
    NoriSaved {
        provider: String,
        env: String,
        store: StoreBackend,
    },
}

pub fn detect_all(ctx: &DetectContext<'_>) -> ProvidersReport {
    let providers = Provider::all()
        .iter()
        .map(|provider| ProviderStatus {
            id: *provider,
            name: provider.display_name(),
            saved: ctx.saved.record_type(*provider).map(|kind| SavedRecord {
                record_type: kind.to_string(),
            }),
            agents: provider
                .agent_routes()
                .into_iter()
                .map(|(agent, env)| AgentKeyRoute {
                    agent: agent.slug(),
                    env,
                })
                .collect(),
        })
        .collect();
    ProvidersReport {
        store: ctx.saved.store(),
        store_error: None,
        providers,
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
    // OpenCode and Pi let their own stored login for a provider win over the
    // environment, so their files come first; the other agents rank the
    // environment above their own login.
    let multi = crate::multi_provider(agent);
    if !multi {
        found.env();
        found.saved();
    }
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
    if multi {
        found.env();
        found.saved();
    }
    let status = match (found.credentials.is_empty(), opaque) {
        (false, _) => AuthStatus::SignedIn,
        (true, true) => AuthStatus::Unknown,
        (true, false) => AuthStatus::SignedOut,
    };
    let notices = if multi {
        Vec::new()
    } else {
        billing_notice(agent, &found.credentials)
            .into_iter()
            .collect()
    };
    AgentAuthStatus {
        agent: agent.slug().to_string(),
        name: agent.display_name().to_string(),
        status,
        credentials: found.credentials,
        saved_keys: crate::key_routes(agent)
            .iter()
            .map(|route| ProviderKeyRoute {
                provider: route.provider,
                env: route.env,
            })
            .collect(),
        notices,
        hint: agent.auth_hint().to_string(),
    }
}

/// Flag an API key or token that the agent will use ahead of a subscription
/// sign-in, which moves billing from the plan to the API account.
fn billing_notice(agent: AgentKind, credentials: &[DetectedCredential]) -> Option<Notice> {
    let first = credentials.first()?;
    if !matches!(first.kind, CredentialKind::ApiKey | CredentialKind::Token) {
        return None;
    }
    let subscription = credentials
        .iter()
        .find(|credential| credential.kind == CredentialKind::Subscription)?;
    let used = match &first.source {
        CredentialSource::Env { name } => format!("${name}"),
        CredentialSource::NoriSaved { provider, .. } => format!("the saved {provider} API key"),
        CredentialSource::File { path } => path.display().to_string(),
        CredentialSource::Keychain { service } => format!("keychain item {service:?}"),
    };
    let plan = match &subscription.detail {
        Some(plan) => format!("{plan} subscription"),
        None => "subscription".to_string(),
    };
    Some(Notice {
        code: "api_key_overrides_subscription",
        message: format!(
            "{} uses {used} instead of your {plan} sign-in, so usage is billed to that API \
             account, not the subscription",
            agent.display_name()
        ),
    })
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
        let provider = match crate::key_routes(self.agent) {
            [only] => Some(only.provider.id().to_string()),
            _ => None,
        };
        self.push_for(provider, kind, source, detail);
    }

    fn push_for(
        &mut self,
        provider: Option<String>,
        kind: CredentialKind,
        source: CredentialSource,
        detail: Option<&str>,
    ) {
        let (read_only, disconnect) = match &source {
            CredentialSource::Env { name } => (true, format!("unset {name} to disconnect")),
            CredentialSource::NoriSaved { provider, .. } => (
                false,
                format!("run `nori providers remove-key {provider}` to disconnect"),
            ),
            CredentialSource::File { .. } | CredentialSource::Keychain { .. } => (
                true,
                format!("sign out in {} to disconnect", self.agent.display_name()),
            ),
        };
        self.credentials.push(DetectedCredential {
            kind,
            source,
            provider,
            detail: detail.map(str::to_string),
            read_only,
            disconnect,
        });
    }

    fn env(&mut self) {
        for credential in crate::env_credentials(self.agent) {
            if credential.is_set(self.ctx.env) {
                self.push_for(
                    Some(credential.provider.id().to_string()),
                    credential.kind,
                    CredentialSource::Env {
                        name: credential.name.to_string(),
                    },
                    None,
                );
            }
        }
    }

    fn env_path(&self, name: &str) -> Option<PathBuf> {
        (self.ctx.env)(name)
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
    }

    fn saved(&mut self) {
        for KeyRoute { provider, env } in crate::key_routes(self.agent) {
            let kind = match self.ctx.saved.record_type(*provider) {
                Some("api_key") => CredentialKind::ApiKey,
                // Record types this build cannot use are not passed at launch.
                _ => continue,
            };
            self.push_for(
                Some(provider.id().to_string()),
                kind,
                CredentialSource::NoriSaved {
                    provider: provider.id().to_string(),
                    env: env.to_string(),
                    store: self.ctx.saved.store(),
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
            self.push_for(
                Some(provider.clone()),
                kind,
                CredentialSource::File { path: path.clone() },
                None,
            );
        }
    }
}

/// Parse a credential file, treating unreadable or malformed files as absent.
/// The parsed value stays in memory only long enough to pick non-secret fields.
fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}
