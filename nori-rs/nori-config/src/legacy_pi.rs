//! Read named Pi provider tables written before Nori shared connections across agents.
use std::collections::BTreeMap;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use serde::Deserialize;
use toml_edit::ArrayOfTables;
use toml_edit::DocumentMut;
use toml_edit::Item;

use crate::AgentConfigToml;
use crate::token_providers::Preset;
use crate::token_providers::Protocol;
use crate::token_providers::SUBCONSCIOUS_DEFAULT_MODEL;
use crate::token_providers::SUBCONSCIOUS_MODELS;
use crate::token_providers::TokenProvider;

#[derive(Deserialize)]
struct LegacyConfig {
    #[serde(default)]
    agents: Vec<AgentConfigToml>,
    token_providers: BTreeMap<String, LegacyProvider>,
    #[serde(default)]
    agent_token_providers: BTreeMap<String, String>,
    #[serde(default)]
    default_models: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyProvider {
    preset: String,
    api_key_env: String,
    base_url: Option<String>,
}

fn is_pi(slug: &str) -> bool {
    slug.eq_ignore_ascii_case("pi") || slug.eq_ignore_ascii_case("pi-acp")
}

/// A legacy picker saved `<connection>/<wire model>`. Handwritten entries also
/// accepted the wire model itself; it may start with the connection's name.
fn wire_model<'a>(model: &'a str, connection: &str) -> Option<&'a str> {
    if SUBCONSCIOUS_MODELS.iter().any(|entry| entry.id == model) {
        Some(model)
    } else {
        model.strip_prefix(connection)?.strip_prefix('/')
    }
}

pub(crate) fn normalize(document: &mut DocumentMut) -> Result<()> {
    if !document
        .get("token_providers")
        .is_some_and(Item::is_table_like)
    {
        return Ok(());
    }
    let saved: LegacyConfig = toml::from_str(&document.to_string()).map_err(|_| {
        anyhow::anyhow!(
            "invalid legacy Pi token provider; expected preset, api_key_env and optional base_url"
        )
    })?;
    // The registry checks exact custom slugs before resolving "pi-acp" to "pi".
    let shadowed = |slug: &str| {
        saved.agents.iter().any(|agent| {
            agent.slug == "pi" || (agent.slug == "pi-acp" && slug.eq_ignore_ascii_case("pi-acp"))
        })
    };
    let belongs_to_pi = |slug: &str| is_pi(slug) && !shadowed(slug);
    ensure!(
        !saved
            .agent_token_providers
            .keys()
            .any(|slug| is_pi(slug) && shadowed(slug)),
        "legacy Pi token provider binding targets a custom agent; remove its [agent_token_providers] entry"
    );
    let mut bindings = saved
        .agent_token_providers
        .iter()
        .filter(|(slug, _)| belongs_to_pi(slug));
    let bound = bindings.next().map(|(_, name)| name);
    ensure!(
        bindings.all(|(_, name)| Some(name) == bound),
        "conflicting Pi token provider bindings"
    );
    let selected = saved
        .default_models
        .get("pi")
        .filter(|_| belongs_to_pi("pi"))
        .or_else(|| {
            saved
                .default_models
                .iter()
                .find(|(slug, _)| belongs_to_pi(slug))
                .map(|(_, model)| model)
        });

    let standard_url = Preset::Subconscious
        .source()?
        .base_url
        .as_deref()
        .context("Subconscious endpoint missing from the compatibility contract")?;
    let mut entries = ArrayOfTables::new();
    for (name, provider) in &saved.token_providers {
        ensure!(
            provider.preset == "subconscious",
            "unknown legacy Pi token provider preset"
        );
        let base_url = provider.base_url.as_deref().unwrap_or(standard_url);
        let model = if Some(name) == bound {
            selected.map(|model| wire_model(model, name).unwrap_or(model))
        } else {
            None
        };
        let connection = TokenProvider {
            name: name.clone(),
            preset: if base_url.trim_end_matches('/') == standard_url {
                Preset::Subconscious
            } else {
                Preset::Custom
            },
            protocol: Protocol::OpenaiCompletions,
            base_url: base_url.to_owned(),
            env_key: provider.api_key_env.clone(),
            model: model.unwrap_or(SUBCONSCIOUS_DEFAULT_MODEL).to_owned(),
        };
        connection.validate()?;
        entries.push(
            toml::to_string(&connection)?
                .parse::<DocumentMut>()?
                .into_table(),
        );
    }

    document["token_providers"] = Item::ArrayOfTables(entries);
    if let Some(name) = bound {
        let aliases = document["agent_token_providers"]
            .as_table_like_mut()
            .context("invalid Pi bindings")?;
        for key in saved
            .agent_token_providers
            .keys()
            .filter(|key| belongs_to_pi(key) && *key != "pi")
        {
            aliases.remove(key);
        }
        aliases.insert("pi", toml_edit::value(name.as_str()));
        if let Some(defaults) = document
            .get_mut("default_models")
            .and_then(Item::as_table_like_mut)
        {
            for (key, model) in saved.default_models {
                if belongs_to_pi(&key) && wire_model(&model, name).is_some() {
                    defaults.remove(&key);
                }
            }
        }
    }
    Ok(())
}
