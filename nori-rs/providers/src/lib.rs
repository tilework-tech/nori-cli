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
