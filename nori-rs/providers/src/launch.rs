use std::path::Path;
use std::sync::Arc;

use codex_keyring_store::DefaultKeyringStore;
use nori_acp_host::registry::AcpAgentConfig;
use tracing::warn;

use crate::saved::SavedKeyStore;
use crate::saved::StoreMode;
use crate::saved_key_env;

/// Put the saved API key for `config`'s built-in agent into its launch
/// environment. Returns the variable set, if any. Keys already present in the
/// ambient environment or the agent's own config win, and agents bound to a
/// named token provider are left alone.
pub fn apply_saved_key(
    config: &mut AcpAgentConfig,
    store: &SavedKeyStore,
    env: &dyn Fn(&str) -> Option<String>,
) -> anyhow::Result<Option<&'static str>> {
    let Some(agent) = config.builtin_kind else {
        return Ok(None);
    };
    let Some(var) = saved_key_env(agent) else {
        return Ok(None);
    };
    let already_set = env(var).is_some_and(|value| !value.trim().is_empty())
        || config.env.contains_key(var)
        || config.launch.env_remove.iter().any(|name| name == var);
    if config.token_provider.is_some() || already_set {
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
