use std::path::Path;
use std::sync::Arc;

use codex_keyring_store::DefaultKeyringStore;
use nori_acp_host::registry::AgentKind;

use crate::AgentAuthStatus;
use crate::DetectContext;
use crate::Platform;
use crate::Provider;
use crate::SystemKeychain;
use crate::store::CredentialStore;
use crate::store::StoreBackend;

/// One agent's sign-in on this machine, and the providers it talks to that
/// have a key saved through Nori.
#[derive(Debug, Clone)]
pub struct LocalAgentAuth {
    pub status: AgentAuthStatus,
    pub saved_providers: Vec<Provider>,
}

/// Detect `agent` against the real environment and keychain, the login files
/// under `home`, and the keys saved under `nori_home` (in the store
/// `NORI_CREDENTIALS_STORE` selects; an unreadable store counts as empty).
/// May block on the OS keyring.
pub fn detect_local_agent(
    agent: AgentKind,
    home: &Path,
    nori_home: &Path,
    prefer_api_key: bool,
) -> anyhow::Result<LocalAgentAuth> {
    let env = |key: &str| std::env::var(key).ok();
    let store = CredentialStore::new(
        nori_home,
        StoreBackend::from_env(&env)?,
        Arc::new(DefaultKeyringStore),
    );
    // A broken store must not block choosing between login and API key.
    let saved = store.index().unwrap_or_else(|error| {
        tracing::warn!("saved API keys are unavailable: {error:#}");
        crate::SavedIndex::default()
    });
    let prefer = |candidate: AgentKind| candidate == agent && prefer_api_key;
    let status = crate::detect_agent(
        agent,
        &DetectContext {
            home,
            env: &env,
            platform: Platform::current(),
            keychain: &SystemKeychain,
            saved: &saved,
            prefer_api_key: &prefer,
        },
    );
    let saved_providers = crate::key_routes(agent)
        .iter()
        .map(|route| route.provider)
        .filter(|provider| saved.record_type(*provider).is_some())
        .collect();
    Ok(LocalAgentAuth {
        status,
        saved_providers,
    })
}
