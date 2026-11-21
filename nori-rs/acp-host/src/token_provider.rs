//! Token providers: API endpoints that serve models to a harness with a key
//! the user keeps in an environment variable.
//!
//! The vocabulary matches the Sessions broker: a preset names a provider and
//! its endpoint, a protocol names the wire API, and each preset lists the
//! harnesses (by broker harness id) that can speak one of its protocols. This
//! CLI currently configures Pi only; other harnesses resolve to an error
//! rather than silently launching without the provider.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use nori_config::TokenProviderToml;
use serde_json::json;

use crate::registry::AcpAgentConfig;
use crate::registry::AgentKind;
use crate::registry::resolve_agent_slug;

/// Wire API a harness uses to reach a token provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenProtocol {
    OpenAiCompletions,
    OpenAiResponses,
    AnthropicMessages,
}

impl TokenProtocol {
    /// Identifier shared by the broker and Pi's `models.json` `api` field.
    pub fn id(self) -> &'static str {
        match self {
            TokenProtocol::OpenAiCompletions => "openai-completions",
            TokenProtocol::OpenAiResponses => "openai-responses",
            TokenProtocol::AnthropicMessages => "anthropic-messages",
        }
    }
}

/// A harness, by broker harness id, and the protocol it uses for a preset.
#[derive(Debug, PartialEq, Eq)]
pub struct HarnessProtocol {
    pub harness: &'static str,
    pub protocol: TokenProtocol,
}

/// A model a preset serves.
#[derive(Debug, PartialEq, Eq)]
pub struct PresetModel {
    /// Provider model id, sent verbatim on the wire.
    pub id: &'static str,
    pub name: &'static str,
    pub context_window: i64,
    pub max_output_tokens: i64,
}

/// A header that tells the provider which harness is calling.
#[derive(Debug, PartialEq, Eq)]
pub struct ClientHeader {
    pub name: &'static str,
    /// Header value per broker harness id.
    pub values: &'static [(&'static str, &'static str)],
}

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
    pub harnesses: &'static [HarnessProtocol],
    pub client_header: Option<ClientHeader>,
    /// Ask the harness to send session-affinity headers so the provider keeps
    /// one conversation on one backend.
    pub session_affinity: bool,
}

const SUBCONSCIOUS_CONTEXT_WINDOW: i64 = 5_000_000;
const SUBCONSCIOUS_MAX_OUTPUT_TOKENS: i64 = 65_536;

const fn subconscious_model(id: &'static str, name: &'static str) -> PresetModel {
    PresetModel {
        id,
        name,
        context_window: SUBCONSCIOUS_CONTEXT_WINDOW,
        max_output_tokens: SUBCONSCIOUS_MAX_OUTPUT_TOKENS,
    }
}

static PRESETS: &[TokenProviderPreset] = &[TokenProviderPreset {
    id: "subconscious",
    name: "Subconscious",
    base_url: "https://api.subconscious.dev/v1",
    api_key_env: "SUBCONSCIOUS_API_KEY",
    default_model: "subconscious/glm-5.3-marathon",
    models: &[
        subconscious_model("subconscious/glm-5.3-marathon", "GLM-5.3 Marathon"),
        subconscious_model("subconscious/glm-5.2", "GLM-5.2"),
        subconscious_model("subconscious/tim-qwen3.6-27b", "Tim-Qwen3.6-27b"),
        subconscious_model(
            "subconscious/deepseek-v4-flash-marathon",
            "DeepSeek-V4-Flash Marathon",
        ),
        subconscious_model(
            "subconscious/deepseek-v4.1-flash-marathon",
            "DeepSeek-V4.1-Flash Marathon",
        ),
    ],
    // Mirrors the broker's compatibility table; this CLI configures `pi-acp`.
    harnesses: &[
        HarnessProtocol {
            harness: "pi-acp",
            protocol: TokenProtocol::OpenAiCompletions,
        },
        HarnessProtocol {
            harness: "opencode",
            protocol: TokenProtocol::OpenAiCompletions,
        },
        HarnessProtocol {
            harness: "codex",
            protocol: TokenProtocol::OpenAiResponses,
        },
        HarnessProtocol {
            harness: "claude",
            protocol: TokenProtocol::AnthropicMessages,
        },
    ],
    client_header: Some(ClientHeader {
        name: "x-subconscious-client",
        values: &[
            ("pi-acp", "pi"),
            ("opencode", "opencode"),
            ("codex", "codex"),
            ("claude", "claude_code"),
        ],
    }),
    session_affinity: true,
}];

/// Look up a preset by id.
pub fn preset(id: &str) -> Option<&'static TokenProviderPreset> {
    PRESETS.iter().find(|preset| preset.id == id)
}

/// Broker harness id for a built-in agent this CLI can configure with a token
/// provider.
fn configurable_harness(agent: AgentKind) -> Option<&'static str> {
    match agent {
        AgentKind::Pi => Some("pi-acp"),
        AgentKind::ClaudeCode | AgentKind::Codex | AgentKind::Gemini | AgentKind::GrokBuild => None,
    }
}

fn harness_protocol(
    preset: &'static TokenProviderPreset,
    harness: &str,
) -> Option<&'static HarnessProtocol> {
    preset
        .harnesses
        .iter()
        .find(|entry| entry.harness == harness)
}

/// Presets the named agent can launch with in this CLI. A custom agent that
/// reuses a built-in slug is not offered any.
pub fn compatible_presets(agent_name: &str) -> Vec<&'static TokenProviderPreset> {
    let Some(harness) = crate::registry::get_agent_config(agent_name)
        .ok()
        .and_then(|config| config.builtin_kind)
        .and_then(configurable_harness)
    else {
        return Vec::new();
    };
    PRESETS
        .iter()
        .filter(|preset| harness_protocol(preset, harness).is_some())
        .collect()
}

/// An agent's resolved token provider binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenProviderSelection {
    pub agent: AgentKind,
    /// Provider id from `[token_providers.<id>]`.
    pub provider_id: String,
    pub preset: &'static TokenProviderPreset,
    pub api_key_env: String,
    pub base_url: String,
    pub model: &'static PresetModel,
}

/// Find the token provider an agent is bound to.
///
/// Returns `Ok(None)` when the agent has no binding, so it uses its own login
/// and settings. A binding that cannot be honored is an error: the agent is
/// never started without the provider the user chose.
///
/// `builtin` is the registered agent's built-in kind, `None` for a custom
/// agent. `model` is the saved model for the agent in its ACP form
/// (`<provider id>/<model id>`) or a bare catalog id.
pub fn resolve_token_provider(
    agent_name: &str,
    builtin: Option<AgentKind>,
    providers: &BTreeMap<String, TokenProviderToml>,
    bindings: &BTreeMap<String, String>,
    model: Option<&str>,
) -> Result<Option<TokenProviderSelection>> {
    let canonical = resolve_agent_slug(agent_name);
    let Some(provider_id) = bindings
        .iter()
        .find(|(agent, _)| resolve_agent_slug(agent) == canonical)
        .map(|(_, provider)| provider)
    else {
        return Ok(None);
    };
    let Some(agent) = builtin else {
        bail!(
            "The custom agent `{canonical}` cannot use token provider `{provider_id}`. Remove it from [agent_token_providers]."
        );
    };
    let safe_id = !provider_id.is_empty()
        && provider_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    if !safe_id {
        bail!("Token provider id `{provider_id}` may contain only letters, digits, `-` and `_`.");
    }
    let display = agent.display_name();
    let Some(provider) = providers.get(provider_id) else {
        bail!(
            "{display} is set to token provider `{provider_id}`, but [token_providers.{provider_id}] is not configured. Choose a provider with /agent."
        );
    };
    let Some(preset) = preset(&provider.preset) else {
        bail!(
            "Token provider `{provider_id}` uses unknown preset `{}`.",
            provider.preset
        );
    };
    let supported = configurable_harness(agent)
        .and_then(|harness| harness_protocol(preset, harness))
        .is_some();
    if !supported {
        bail!(
            "{} is not available for {display} in this CLI yet.",
            preset.name
        );
    }
    let model_id = model.unwrap_or(preset.default_model);
    let catalog_entry = |id: &str| preset.models.iter().find(|entry| entry.id == id);
    let found = catalog_entry(model_id).or_else(|| {
        model_id
            .strip_prefix(&format!("{provider_id}/"))
            .and_then(catalog_entry)
    });
    let Some(model) = found else {
        bail!(
            "Model `{model_id}` is not offered by {}. Choose one with /agent.",
            preset.name
        );
    };
    Ok(Some(TokenProviderSelection {
        agent,
        provider_id: provider_id.clone(),
        preset,
        api_key_env: provider.api_key_env.clone(),
        base_url: provider
            .base_url
            .clone()
            .unwrap_or_else(|| preset.base_url.to_string()),
        model,
    }))
}

/// Prepare an agent launch for its token provider.
///
/// Fails before anything is written when the key variable is missing. The
/// generated files name the variable and never contain the key; the agent
/// reads the key from the environment it inherits.
pub fn apply_token_provider(
    config: &mut AcpAgentConfig,
    selection: &TokenProviderSelection,
    nori_home: &Path,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<()> {
    let variable = &selection.api_key_env;
    if env(variable).is_none_or(|value| value.trim().is_empty()) {
        bail!(
            "{} needs an API key for {}. Set {variable} in the environment that starts Nori, or choose another provider with /agent.",
            selection.preset.name,
            selection.agent.display_name()
        );
    }
    match selection.agent {
        AgentKind::Pi => write_pi_agent_dir(config, selection, nori_home),
        AgentKind::ClaudeCode | AgentKind::Codex | AgentKind::Gemini | AgentKind::GrokBuild => {
            bail!(
                "{} is not available for {} in this CLI yet.",
                selection.preset.name,
                selection.agent.display_name()
            )
        }
    }
}

/// Pi reads providers and its starting model from `PI_CODING_AGENT_DIR`.
fn write_pi_agent_dir(
    config: &mut AcpAgentConfig,
    selection: &TokenProviderSelection,
    nori_home: &Path,
) -> Result<()> {
    let harness = "pi-acp";
    let preset = selection.preset;
    let protocol = harness_protocol(preset, harness)
        .context("Pi has no protocol for this provider")?
        .protocol;
    let headers: serde_json::Map<String, serde_json::Value> = preset
        .client_header
        .iter()
        .filter_map(|header| {
            header
                .values
                .iter()
                .find(|(name, _)| *name == harness)
                .map(|(_, value)| (header.name.to_string(), json!(value)))
        })
        .collect();
    let models: Vec<serde_json::Value> = preset
        .models
        .iter()
        .map(|model| {
            let mut entry = json!({
                "id": model.id,
                "name": model.name,
                "contextWindow": model.context_window,
                "maxTokens": model.max_output_tokens,
            });
            if preset.session_affinity {
                entry["compat"] = json!({
                    "sendSessionAffinityHeaders": true,
                    "sessionAffinityFormat": "openai-nosession",
                });
            }
            entry
        })
        .collect();
    let models_json = json!({
        "providers": {
            selection.provider_id.as_str(): {
                "baseUrl": selection.base_url,
                "api": protocol.id(),
                "apiKey": format!("${}", selection.api_key_env),
                "headers": headers,
                "models": models,
            }
        }
    });
    let settings_json = json!({
        "defaultProvider": selection.provider_id,
        "defaultModel": selection.model.id,
        "quietStartup": true,
    });

    let root = nori_home.join("token-providers");
    let directory = root.join(format!("{}-{harness}", selection.provider_id));
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
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
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
