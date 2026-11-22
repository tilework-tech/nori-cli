use std::path::Path;
use std::sync::Arc;

use codex_keyring_store::DefaultKeyringStore;
use nori_acp_host::registry::AcpAgentConfig;
use tracing::warn;

use crate::store::CredentialStore;
use crate::store::StoreBackend;

/// Put the saved API key of each provider `config`'s built-in agent talks to
/// into its launch environment, in that agent's variable for the provider.
/// Returns the variables set. A provider is skipped when any credential
/// variable the agent honours for it is set (in the ambient environment or the
/// agent's own config), when the provider's base-URL variable points it at
/// another endpoint, or when its variable is in `launch.env_remove`; agents bound
/// to a named token provider are left alone entirely.
pub fn apply_saved_keys(
    config: &mut AcpAgentConfig,
    store: &CredentialStore,
    env: &dyn Fn(&str) -> Option<String>,
) -> anyhow::Result<Vec<&'static str>> {
    let Some(agent) = config.builtin_kind else {
        return Ok(Vec::new());
    };
    let routes = crate::key_routes(agent);
    if routes.is_empty() || config.token_provider.is_some() {
        return Ok(Vec::new());
    }
    let configured = |name: &str| config.env.get(name).cloned().or_else(|| env(name));
    let wanted: Vec<_> = routes
        .iter()
        .filter(|route| {
            let explicit = crate::env_credentials(agent).iter().any(|credential| {
                credential.provider == route.provider && credential.is_set(&configured)
            });
            let removed = config
                .launch
                .env_remove
                .iter()
                .any(|name| name == route.env);
            let redirected = route
                .provider
                .base_url_env()
                .is_some_and(|name| configured(name).is_some_and(|value| !value.trim().is_empty()));
            !explicit && !redirected && !removed
        })
        .collect();
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let saved = store.all()?;
    let mut applied = Vec::new();
    for route in wanted {
        if let Some(credential) = saved.get(&route.provider) {
            config
                .env
                .insert(route.env.to_string(), credential.key().to_string());
            applied.push(route.env);
        }
    }
    Ok(applied)
}

/// [`apply_saved_keys`] against the store `NORI_CREDENTIALS_STORE` selects
/// under `nori_home`, logging (without values) instead of failing the launch
/// when the store is unreadable.
pub fn inject_saved_keys(
    config: &mut AcpAgentConfig,
    nori_home: &Path,
    env: &dyn Fn(&str) -> Option<String>,
) {
    if config
        .builtin_kind
        .is_none_or(|agent| crate::key_routes(agent).is_empty())
    {
        return;
    }
    let result = StoreBackend::from_env(env).and_then(|backend| {
        let store = CredentialStore::new(nori_home, backend, Arc::new(DefaultKeyringStore));
        apply_saved_keys(config, &store, env)
    });
    if let Err(error) = result {
        warn!(
            "launching {} without saved API keys: {error:#}",
            config.display_name
        );
    }
}
