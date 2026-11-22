//! Translate a named API connection into the private profile read by Pi.
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use nori_config::token_providers::TokenProvider;
use serde_json::json;

use crate::registry::AcpAgentConfig;
use crate::registry::AgentKind;

use crate::launch::OwnedRuntime;
use crate::launch::private_dir;
use crate::launch::private_file;

/// Metadata for applying a credential reference to a single server process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundConnection {
    pub connection: TokenProvider,
    pub child_key: String,
}

/// Return the ACP model identity that corresponds to the connection's wire model.
pub fn bound_model_id(agent: &str, binding: &TokenProvider) -> String {
    if agent == "pi" {
        format!("{}/{}", binding.name, binding.model)
    } else {
        binding.model.clone()
    }
}

/// Prepare Pi's private profile before starting a bound agent. Native Pi and
/// the other launch adapters do not need a generated profile.
pub fn prepare_pi_profile(
    config: &mut AcpAgentConfig,
    nori_home: &Path,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<()> {
    let root = nori_home.join(".pi-launches");
    let Some(binding) = config
        .token_provider
        .as_ref()
        .filter(|_| config.builtin_kind == Some(AgentKind::Pi))
    else {
        if let Err(error) = OwnedRuntime::sweep_existing(&root) {
            tracing::warn!("could not collect abandoned ACP runtime directories: {error}");
        }
        return Ok(());
    };
    let binding = &binding.connection;
    crate::token_launch::credential(binding, lookup)?;
    // The Messages SDK appends /v1/messages itself.
    let base_url = if binding.protocol == nori_config::token_providers::Protocol::AnthropicMessages
    {
        binding
            .base_url
            .trim_end_matches('/')
            .strip_suffix("/v1")
            .unwrap_or(&binding.base_url)
    } else {
        &binding.base_url
    };
    let models_json = json!({
        "providers": {
            binding.name.as_str(): {
                "baseUrl": base_url,
                "authHeader": binding.bearer_messages(),
                "api": binding.protocol.as_str(),
                "apiKey": format!("${}", binding.env_key),
                "models": [{"id": binding.model, "name": binding.model}],
            }
        }
    });
    let home = std::path::absolute(nori_home)?;
    std::fs::create_dir_all(&home)?;
    let runtime = OwnedRuntime::create(&home.join(".pi-launches"))?;
    let root = home.join("token-providers");
    let binding_dir = root.join(format!("{}-pi-acp", binding.name));
    let history = binding_dir.join("sessions");
    for dir in [&root, &binding_dir, &history] {
        private_dir(dir)?;
    }
    let settings_json = json!({
        "defaultProvider": binding.name,
        "defaultModel": binding.model,
        "quietStartup": true,
        "sessionDir": history,
    });

    for (name, value) in [
        ("models.json", models_json),
        ("settings.json", settings_json),
    ] {
        private_file()
            .create_new(true)
            .open(runtime.path().join(name))?
            .write_all(&serde_json::to_vec_pretty(&value)?)?;
    }
    config.env.insert(
        "PI_CODING_AGENT_DIR".to_string(),
        runtime.path().to_string_lossy().into_owned(),
    );
    config.launch.runtime_dir = Some(runtime.path().to_owned());
    config.owned_runtime = Some(std::sync::Arc::new(runtime));
    Ok(())
}
