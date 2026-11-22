//! Sign-in detection and saved API keys for Nori's built-in ACP agents.
//!
//! Detection reports where each agent would find credentials (environment,
//! the agent's own login files, the OS keychain, or a key saved through Nori)
//! without returning or logging secret values. Saved credentials are keyed by
//! model provider (one OpenAI key serves every OpenAI-backed agent), live in a
//! private file under `$NORI_HOME` (or the OS keyring on request), and reach
//! each agent as that agent's own environment variable at launch.

mod detect;
mod launch;
mod store;

pub use detect::AgentAuthStatus;
pub use detect::AuthStatus;
pub use detect::CredentialKind;
pub use detect::CredentialSource;
pub use detect::DetectContext;
pub use detect::DetectedCredential;
pub use detect::KeychainProbe;
pub use detect::Notice;
pub use detect::Platform;
pub use detect::ProviderStatus;
pub use detect::ProvidersReport;
pub use detect::SystemKeychain;
pub use detect::detect_agent;
pub use detect::detect_all;
pub use launch::apply_saved_keys;
pub use launch::inject_saved_keys;
pub use store::Credential;
pub use store::CredentialStore;
pub use store::STORE_BACKEND_ENV;
pub use store::SavedIndex;
pub use store::StoreBackend;

use nori_acp_host::registry::AgentKind;
use serde::Serialize;

/// A model provider whose API key Nori can save once and hand to every agent
/// that talks to it. Ids match the provider ids in Pi's and OpenCode's
/// `auth.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Provider {
    Anthropic,
    OpenAi,
    Google,
    Xai,
    Cursor,
}

impl Provider {
    pub fn all() -> &'static [Provider] {
        &[
            Provider::Anthropic,
            Provider::OpenAi,
            Provider::Google,
            Provider::Xai,
            Provider::Cursor,
        ]
    }

    pub fn id(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenAi => "openai",
            Provider::Google => "google",
            Provider::Xai => "xai",
            Provider::Cursor => "cursor",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Provider::Anthropic => "Anthropic",
            Provider::OpenAi => "OpenAI",
            Provider::Google => "Google Gemini",
            Provider::Xai => "xAI",
            Provider::Cursor => "Cursor",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Provider::all()
            .iter()
            .copied()
            .find(|provider| provider.id() == id)
    }

    /// Parse a provider id, pointing agent names at the provider they use.
    pub fn parse(id: &str) -> anyhow::Result<Self> {
        if let Some(provider) = Provider::from_id(id) {
            return Ok(provider);
        }
        let ids: Vec<_> = Provider::all().iter().map(|p| p.id()).collect();
        let ids = ids.join(", ");
        match AgentKind::from_slug(id).map(|agent| (agent, key_routes(agent))) {
            Some((_, [route])) => anyhow::bail!(
                "{id:?} is an agent; save its key under its provider, `{}`",
                route.provider.id()
            ),
            Some((agent, [])) => anyhow::bail!(
                "{id:?} is an agent that signs in through its own login; {} takes no saved key",
                agent.display_name()
            ),
            Some((agent, _)) => anyhow::bail!(
                "{id:?} is an agent; {} uses the key of each provider it talks to ({ids})",
                agent.display_name()
            ),
            // Unknown input is not echoed: it may be a key pasted in the wrong place.
            None => anyhow::bail!("that is not a provider; use one of {ids}"),
        }
    }
}

impl Provider {
    /// Built-in agents a saved key for this provider reaches, with the
    /// variable each receives it in.
    pub fn agent_routes(self) -> Vec<(AgentKind, &'static str)> {
        AgentKind::all()
            .iter()
            .flat_map(|agent| {
                key_routes(*agent)
                    .iter()
                    .filter(move |route| route.provider == self)
                    .map(|route| (*agent, route.env))
            })
            .collect()
    }

    /// A variable that points this provider's API at another endpoint. When
    /// set, the saved key is withheld so it is never sent to that endpoint.
    pub(crate) fn base_url_env(self) -> Option<&'static str> {
        match self {
            Provider::Anthropic => Some("ANTHROPIC_BASE_URL"),
            Provider::OpenAi => Some("OPENAI_BASE_URL"),
            Provider::Google => Some("GOOGLE_GEMINI_BASE_URL"),
            Provider::Xai | Provider::Cursor => None,
        }
    }
}

impl Serialize for Provider {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.id())
    }
}

/// The variable an agent reads a provider's API key from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyRoute {
    pub provider: Provider,
    pub env: &'static str,
}

/// The providers whose saved key `agent` accepts, and the variable each is
/// passed in. Empty for agents that only sign in through their own login.
pub fn key_routes(agent: AgentKind) -> &'static [KeyRoute] {
    const fn route(provider: Provider, env: &'static str) -> KeyRoute {
        KeyRoute { provider, env }
    }
    const CLAUDE: &[KeyRoute] = &[route(Provider::Anthropic, "ANTHROPIC_API_KEY")];
    const CODEX: &[KeyRoute] = &[route(Provider::OpenAi, "OPENAI_API_KEY")];
    const ANTIGRAVITY: &[KeyRoute] = &[route(Provider::Google, "GEMINI_API_KEY")];
    const GROK: &[KeyRoute] = &[route(Provider::Xai, "XAI_API_KEY")];
    const CURSOR: &[KeyRoute] = &[route(Provider::Cursor, "CURSOR_API_KEY")];
    const OPENCODE: &[KeyRoute] = &[
        route(Provider::Anthropic, "ANTHROPIC_API_KEY"),
        route(Provider::OpenAi, "OPENAI_API_KEY"),
        route(Provider::Google, "GOOGLE_GENERATIVE_AI_API_KEY"),
        route(Provider::Xai, "XAI_API_KEY"),
    ];
    const PI: &[KeyRoute] = &[
        route(Provider::Anthropic, "ANTHROPIC_API_KEY"),
        route(Provider::OpenAi, "OPENAI_API_KEY"),
        route(Provider::Google, "GEMINI_API_KEY"),
        route(Provider::Xai, "XAI_API_KEY"),
    ];
    match agent {
        AgentKind::ClaudeCode => CLAUDE,
        AgentKind::Codex => CODEX,
        AgentKind::Gemini => ANTIGRAVITY,
        AgentKind::GrokBuild => GROK,
        AgentKind::Cursor => CURSOR,
        AgentKind::OpenCode => OPENCODE,
        AgentKind::Pi => PI,
        AgentKind::Hermes => &[],
    }
}

/// Whether `agent` talks to more than one provider, so its credentials are
/// ranked per provider rather than as one list.
pub(crate) fn multi_provider(agent: AgentKind) -> bool {
    key_routes(agent).len() > 1
}

/// A credential an agent reads from its environment.
pub(crate) struct EnvCredential {
    pub name: &'static str,
    pub provider: Provider,
    pub kind: detect::CredentialKind,
    /// Counts only when set to `1` or `true`, like `CLAUDE_CODE_USE_BEDROCK`.
    pub flag: bool,
}

/// Environment credentials each agent honours, in the agent's precedence
/// order. Any of them being set for a provider means the user chose that
/// provider's credential explicitly, so its saved key is not injected.
pub(crate) fn env_credentials(agent: AgentKind) -> &'static [EnvCredential] {
    use Provider::Anthropic;
    use Provider::Google;
    use Provider::OpenAi;
    use Provider::Xai;
    use detect::CredentialKind::ApiKey;
    use detect::CredentialKind::Cloud;
    use detect::CredentialKind::Subscription;
    use detect::CredentialKind::Token;
    const fn var(
        name: &'static str,
        provider: Provider,
        kind: detect::CredentialKind,
    ) -> EnvCredential {
        EnvCredential {
            name,
            provider,
            kind,
            flag: false,
        }
    }
    const fn flag(name: &'static str, provider: Provider) -> EnvCredential {
        EnvCredential {
            name,
            provider,
            kind: Cloud,
            flag: true,
        }
    }
    const CLAUDE: &[EnvCredential] = &[
        flag("CLAUDE_CODE_USE_BEDROCK", Anthropic),
        flag("CLAUDE_CODE_USE_VERTEX", Anthropic),
        flag("CLAUDE_CODE_USE_FOUNDRY", Anthropic),
        var("ANTHROPIC_AUTH_TOKEN", Anthropic, Token),
        var("ANTHROPIC_API_KEY", Anthropic, ApiKey),
        var("CLAUDE_CODE_OAUTH_TOKEN", Anthropic, Subscription),
    ];
    const CODEX: &[EnvCredential] = &[
        var("CODEX_API_KEY", OpenAi, ApiKey),
        var("OPENAI_API_KEY", OpenAi, ApiKey),
    ];
    const ANTIGRAVITY: &[EnvCredential] = &[
        flag("GOOGLE_GENAI_USE_VERTEXAI", Google),
        flag("AGY_ADC_AUTH", Google),
        var("GEMINI_API_KEY", Google, ApiKey),
        var("GOOGLE_API_KEY", Google, ApiKey),
    ];
    const GROK: &[EnvCredential] = &[var("XAI_API_KEY", Xai, ApiKey)];
    const CURSOR: &[EnvCredential] = &[var("CURSOR_API_KEY", Provider::Cursor, ApiKey)];
    const OPENCODE: &[EnvCredential] = &[
        var("ANTHROPIC_API_KEY", Anthropic, ApiKey),
        var("OPENAI_API_KEY", OpenAi, ApiKey),
        var("GOOGLE_GENERATIVE_AI_API_KEY", Google, ApiKey),
        var("GOOGLE_API_KEY", Google, ApiKey),
        var("GEMINI_API_KEY", Google, ApiKey),
        var("XAI_API_KEY", Xai, ApiKey),
    ];
    const PI: &[EnvCredential] = &[
        var("ANTHROPIC_AUTH_TOKEN", Anthropic, Token),
        var("ANTHROPIC_OAUTH_TOKEN", Anthropic, Subscription),
        var("ANTHROPIC_API_KEY", Anthropic, ApiKey),
        var("OPENAI_API_KEY", OpenAi, ApiKey),
        var("GEMINI_API_KEY", Google, ApiKey),
        var("XAI_API_KEY", Xai, ApiKey),
    ];
    match agent {
        AgentKind::ClaudeCode => CLAUDE,
        AgentKind::Codex => CODEX,
        AgentKind::Gemini => ANTIGRAVITY,
        AgentKind::GrokBuild => GROK,
        AgentKind::Cursor => CURSOR,
        AgentKind::OpenCode => OPENCODE,
        AgentKind::Pi => PI,
        AgentKind::Hermes => &[],
    }
}

impl EnvCredential {
    pub(crate) fn is_set(&self, env: &dyn Fn(&str) -> Option<String>) -> bool {
        env(self.name).is_some_and(|value| {
            let value = value.trim();
            if self.flag {
                value == "1" || value.eq_ignore_ascii_case("true")
            } else {
                !value.is_empty()
            }
        })
    }
}
