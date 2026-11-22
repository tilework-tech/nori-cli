use std::path::Path;
use std::sync::Arc;

use codex_keyring_store::DefaultKeyringStore;
use nori_acp_host::registry::AcpAgentConfig;
use tracing::warn;

use crate::saved::SavedKeyStore;
use crate::saved::StoreMode;
use crate::saved_key_env;

/// Put the saved API key for `config`'s built-in agent into its launch
/// environment. Returns the variable set, if any. Any credential variable the
/// agent honours, whether in the ambient environment or the agent's own
/// config, wins over a saved key, and agents bound to a named token provider
/// or told to drop the variable are left alone.
pub fn apply_saved_key(
    config: &mut AcpAgentConfig,
    store: &SavedKeyStore,
    env: &dyn Fn(&str) -> Option<String>,
) -> anyhow::Result<Option<&'static str>> {
    let Some((agent, var)) = target(config) else {
        return Ok(None);
    };
    let configured = |name: &str| config.env.get(name).cloned().or_else(|| env(name));
    let explicit = crate::env_credentials(agent)
        .iter()
        .any(|credential| credential.is_set(&configured));
    let removed = config.launch.env_remove.iter().any(|name| name == var);
    if config.token_provider.is_some() || explicit || removed {
        return Ok(None);
    }
    let Some(key) = store.load(agent)? else {
        return Ok(None);
    };
    config.env.insert(var.to_string(), key);
    Ok(Some(var))
}

/// [`apply_saved_key`] against the system keychain and `$NORI_HOME`, logging
/// (without the value) instead of failing the launch when the store is unreadable.
pub fn inject_saved_key(
    config: &mut AcpAgentConfig,
    nori_home: &Path,
    env: &dyn Fn(&str) -> Option<String>,
) {
    if target(config).is_none() {
        return;
    }
    let result = StoreMode::from_env(env).and_then(|mode| {
        let store = SavedKeyStore::new(nori_home, mode, Arc::new(DefaultKeyringStore));
        apply_saved_key(config, &store, env)
    });
    if let Err(error) = result {
        warn!(
            "launching {} without its saved API key: {error:#}",
            config.display_name
        );
    }
}

fn target(config: &AcpAgentConfig) -> Option<(nori_acp_host::registry::AgentKind, &'static str)> {
    let agent = config.builtin_kind?;
    Some((agent, saved_key_env(agent)?))
}
