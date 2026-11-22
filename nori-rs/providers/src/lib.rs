//! Sign-in detection and saved API keys for Nori's built-in ACP agents.
//!
//! Detection reports where each agent would find credentials (environment,
//! the agent's own login files, the OS keychain, or a key saved through Nori)
//! without returning or logging secret values. Saved keys live in the OS
//! keychain, or in a 0600 file when no keychain is available, and reach the
//! agent as an environment variable at launch.

mod detect;
mod launch;
mod saved;

pub use detect::AgentAuthStatus;
pub use detect::AuthStatus;
pub use detect::CredentialKind;
pub use detect::CredentialSource;
pub use detect::DetectContext;
pub use detect::DetectedCredential;
pub use detect::KeychainProbe;
pub use detect::Platform;
pub use detect::ProvidersReport;
pub use detect::SystemKeychain;
pub use detect::detect_agent;
pub use detect::detect_all;
pub use launch::apply_saved_key;
pub use launch::inject_saved_key;
pub use saved::STORE_MODE_ENV;
pub use saved::SavedKeyIndex;
pub use saved::SavedKeyStore;
pub use saved::StoreLocation;
pub use saved::StoreMode;

use nori_acp_host::registry::AgentKind;

/// The environment variable a saved API key is passed in for `agent`, or
/// `None` when the agent only signs in through its own login or profiles.
pub fn saved_key_env(agent: AgentKind) -> Option<&'static str> {
    match agent {
        AgentKind::ClaudeCode => Some("ANTHROPIC_API_KEY"),
        AgentKind::Codex => Some("OPENAI_API_KEY"),
        AgentKind::Gemini => Some("GEMINI_API_KEY"),
        AgentKind::GrokBuild => Some("XAI_API_KEY"),
        AgentKind::Cursor => Some("CURSOR_API_KEY"),
        AgentKind::OpenCode | AgentKind::Hermes | AgentKind::Pi => None,
    }
}

/// A credential an agent reads from its environment.
pub(crate) struct EnvCredential {
    pub name: &'static str,
    pub kind: detect::CredentialKind,
    /// Counts only when set to `1` or `true`, like `CLAUDE_CODE_USE_BEDROCK`.
    pub flag: bool,
}

/// Environment credentials each agent honours, in the agent's precedence
/// order. Any of them being set means the user chose a credential explicitly,
/// so a saved key is neither injected nor reported ahead of them.
pub(crate) fn env_credentials(agent: AgentKind) -> &'static [EnvCredential] {
    use detect::CredentialKind::ApiKey;
    use detect::CredentialKind::Cloud;
    use detect::CredentialKind::Subscription;
    use detect::CredentialKind::Token;
    const fn var(name: &'static str, kind: detect::CredentialKind) -> EnvCredential {
        EnvCredential {
            name,
            kind,
            flag: false,
        }
    }
    const fn flag(name: &'static str) -> EnvCredential {
        EnvCredential {
            name,
            kind: Cloud,
            flag: true,
        }
    }
    const CLAUDE: &[EnvCredential] = &[
        flag("CLAUDE_CODE_USE_BEDROCK"),
        flag("CLAUDE_CODE_USE_VERTEX"),
        flag("CLAUDE_CODE_USE_FOUNDRY"),
        var("ANTHROPIC_AUTH_TOKEN", Token),
        var("ANTHROPIC_API_KEY", ApiKey),
        var("CLAUDE_CODE_OAUTH_TOKEN", Subscription),
    ];
    const CODEX: &[EnvCredential] = &[var("CODEX_API_KEY", ApiKey), var("OPENAI_API_KEY", ApiKey)];
    const ANTIGRAVITY: &[EnvCredential] = &[
        flag("GOOGLE_GENAI_USE_VERTEXAI"),
        flag("AGY_ADC_AUTH"),
        var("GEMINI_API_KEY", ApiKey),
        var("GOOGLE_API_KEY", ApiKey),
    ];
    const GROK: &[EnvCredential] = &[var("XAI_API_KEY", ApiKey)];
    const CURSOR: &[EnvCredential] = &[var("CURSOR_API_KEY", ApiKey)];
    match agent {
        AgentKind::ClaudeCode => CLAUDE,
        AgentKind::Codex => CODEX,
        AgentKind::Gemini => ANTIGRAVITY,
        AgentKind::GrokBuild => GROK,
        AgentKind::Cursor => CURSOR,
        AgentKind::OpenCode | AgentKind::Hermes | AgentKind::Pi => &[],
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
