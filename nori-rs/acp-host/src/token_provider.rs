//! Local Pi launch metadata for the Subconscious preset. Nori config owns
//! connection validation and the shared harness compatibility contract.
use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use nori_config::token_providers::Preset;
use nori_config::token_providers::Protocol;
use nori_config::token_providers::TokenProvider;
use serde_json::json;

use crate::registry::AcpAgentConfig;
use crate::registry::AgentKind;

pub use nori_config::token_providers::PresetModel;

/// A known token provider.
#[derive(Debug, PartialEq, Eq)]
pub struct TokenProviderPreset {
    /// Broker preset id, also the default provider id in config.
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    /// Environment variable suggested for the key.
    pub api_key_env: &'static str,
    pub default_model: &'static str,
    pub models: &'static [PresetModel],
}

static SUBCONSCIOUS: TokenProviderPreset = TokenProviderPreset {
    id: "subconscious",
    name: "Subconscious",
    base_url: "https://api.subconscious.dev/v1",
    api_key_env: "SUBCONSCIOUS_API_KEY",
    default_model: nori_config::token_providers::SUBCONSCIOUS_DEFAULT_MODEL,
    models: nori_config::token_providers::SUBCONSCIOUS_MODELS,
};

/// Look up a preset by id.
pub fn preset(id: &str) -> Option<&'static TokenProviderPreset> {
    (SUBCONSCIOUS.id == id).then_some(&SUBCONSCIOUS)
}

/// Whether this connection can use the local Pi adapter and its known model catalog.
pub fn supports_pi_connection(binding: &TokenProvider) -> bool {
    matches!(binding.preset, Preset::Subconscious | Preset::Custom)
        && binding.validate().is_ok()
        && binding.protocol_for("pi").ok() == Some(Protocol::OpenaiCompletions)
        && SUBCONSCIOUS
            .models
            .iter()
            .any(|model| model.id == binding.model)
}

/// Presets offered when the actual registered executable is the built-in Pi agent.
pub fn compatible_presets(agent_name: &str) -> Vec<&'static TokenProviderPreset> {
    if crate::registry::get_agent_config(agent_name)
        .is_ok_and(|config| config.builtin_kind == Some(AgentKind::Pi))
        && supports_pi_connection(&SUBCONSCIOUS.connection(
            SUBCONSCIOUS.id,
            SUBCONSCIOUS.api_key_env,
            SUBCONSCIOUS.default_model,
        ))
    {
        vec![&SUBCONSCIOUS]
    } else {
        Vec::new()
    }
}

impl TokenProviderPreset {
    /// Construct an editable named connection with the preset's endpoint and wire model.
    pub fn connection(&self, name: &str, env_key: &str, model: &str) -> TokenProvider {
        TokenProvider {
            name: name.to_owned(),
            preset: Preset::Subconscious,
            protocol: Protocol::OpenaiCompletions,
            base_url: self.base_url.to_owned(),
            env_key: env_key.to_owned(),
            model: model.to_owned(),
        }
    }
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
    let Some(binding) = config.token_provider.as_ref() else {
        return Ok(());
    };
    if config.builtin_kind != Some(AgentKind::Pi) {
        return Ok(());
    }
    anyhow::ensure!(
        supports_pi_connection(binding),
        "Pi requires a compatible Subconscious connection and catalog model"
    );
    crate::token_launch::credential(binding, lookup)?;
    let models: Vec<_> = SUBCONSCIOUS
        .models
        .iter()
        .map(|model| {
            json!({
                "id": model.id,
                "name": model.name,
                "contextWindow": model.context_window,
                "maxTokens": model.max_output_tokens,
                "compat": {
                    "sendSessionAffinityHeaders": true,
                    "sessionAffinityFormat": "openai-nosession"
                }
            })
        })
        .collect();
    let models_json = json!({
        "providers": {
            binding.name.as_str(): {
                "baseUrl": binding.base_url,
                "api": Protocol::OpenaiCompletions.as_str(),
                "apiKey": format!("${}", binding.env_key),
                "headers": {"x-subconscious-client": "pi"},
                "models": models,
            }
        }
    });
    let settings_json = json!({
        "defaultProvider": binding.name,
        "defaultModel": binding.model,
        "quietStartup": true,
    });

    let root = nori_home.join("token-providers");
    let directory = root.join(format!("{}-pi-acp", binding.name));
    std::fs::create_dir_all(&directory)
        .with_context(|| format!("failed to create {}", directory.display()))?;
    restrict(&root, 0o700)?;
    restrict(&directory, 0o700)?;
    write_private(
        &directory.join("models.json"),
        &serde_json::to_string_pretty(&models_json)?,
    )?;
    write_private(
        &directory.join("settings.json"),
        &serde_json::to_string_pretty(&settings_json)?,
    )?;
    config.env.insert(
        "PI_CODING_AGENT_DIR".to_string(),
        directory.to_string_lossy().into_owned(),
    );
    Ok(())
}

/// Write through a private temporary file and rename it into place, so a Pi
/// starting at the same moment never reads a partial file.
fn write_private(path: &Path, contents: &str) -> Result<()> {
    use std::io::Write as _;
    let temporary = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    file.write_all(contents.as_bytes())
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    restrict(&temporary, 0o600)?;
    std::fs::rename(&temporary, path)
        .with_context(|| format!("failed to replace {}", path.display()))
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("failed to restrict {}", path.display()))
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}
