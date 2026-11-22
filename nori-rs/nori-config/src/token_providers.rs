//! Named API connections. A resolved binding contains only metadata and an environment reference.
use std::collections::HashMap;

use anyhow::Result;
use anyhow::ensure;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::de::Error;
use url::Url;

use crate::harness_compat::Environment;
use crate::harness_compat::Pairing;
use crate::harness_compat::Status;
use crate::harness_compat::{self};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Protocol {
    OpenaiCompletions,
    OpenaiResponses,
    AnthropicMessages,
}

impl Protocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenaiCompletions => "openai-completions",
            Self::OpenaiResponses => "openai-responses",
            Self::AnthropicMessages => "anthropic-messages",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Openrouter,
    Subconscious,
    Fireworks,
    Cloudflare,
    Custom,
}

impl Preset {
    fn source_id(self) -> &'static str {
        match self {
            Self::Openrouter => "token:openrouter",
            Self::Subconscious => "token:subconscious",
            Self::Fireworks => "token:fireworks",
            Self::Cloudflare => "token:cloudflare",
            Self::Custom => "token:custom",
        }
    }

    pub(crate) fn source(self) -> Result<&'static harness_compat::Source> {
        harness_compat::embedded()?
            .source(self.source_id())
            .ok_or_else(|| anyhow::anyhow!("token preset missing from harness-compat"))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TokenProvider {
    pub name: String,
    pub preset: Preset,
    pub protocol: Protocol,
    pub base_url: String,
    pub env_key: String,
    pub model: String,
}

pub(crate) fn deserialize_entries<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Vec<TokenProvider>, D::Error> {
    Vec::deserialize(d).map_err(|_| D::Error::custom("invalid token_providers entry; expected name, preset, protocol, base_url, env_key and model (API key references only)"))
}

pub(crate) fn deserialize_bindings<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<HashMap<String, String>, D::Error> {
    HashMap::deserialize(d).map_err(|_| {
        D::Error::custom("agent_token_providers must map agent slugs to provider names")
    })
}

impl TokenProvider {
    /// Check connection metadata and the preset's endpoint and protocol constraints.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.is_empty()
                && self
                    .name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_')),
            "invalid token provider name"
        );
        let mut env = self.env_key.bytes();
        ensure!(
            env.next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
                && env.all(|c| c.is_ascii_alphanumeric() || c == b'_'),
            "env_key must name an environment variable"
        );
        ensure!(
            !self.model.is_empty()
                && !self
                    .model
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control()),
            "token provider requires a model ID"
        );
        ensure!(
            Url::parse(&self.base_url).is_ok_and(|url| matches!(url.scheme(), "http" | "https")
                && url.has_host()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none())
                && !self
                    .base_url
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control()),
            "base_url must be an HTTP(S) endpoint without credentials, query or fragment"
        );
        let endpoint = self.base_url.trim_end_matches('/');
        let source = self.preset.source()?;
        let matches_preset = match self.preset {
            Preset::Custom => true,
            Preset::Openrouter | Preset::Fireworks | Preset::Subconscious => {
                source.base_url.as_deref() == Some(endpoint)
            }
            Preset::Cloudflare => endpoint
                .strip_prefix("https://api.cloudflare.com/client/v4/accounts/")
                .and_then(|path| path.strip_suffix("/ai/v1"))
                .is_some_and(|id| id.len() == 32 && id.bytes().all(|c| c.is_ascii_hexdigit())),
        };
        ensure!(
            matches_preset,
            "base_url does not match the named gateway; use the custom preset for other endpoints"
        );
        ensure!(
            source.protocols.contains(&self.protocol),
            "declared protocol is incompatible with the named gateway"
        );
        Ok(())
    }

    /// These gateways authenticate Messages requests using an Authorization bearer token.
    pub fn bearer_messages(&self) -> bool {
        self.protocol == Protocol::AnthropicMessages
            && self
                .preset
                .source()
                .is_ok_and(|source| source.messages_auth.as_deref() == Some("bearer"))
    }

    /// Resolve a saved local binding. Bindings are keyed by canonical CLI slug so
    /// an alias cannot route around an explicit selection or shadow a custom command.
    pub fn protocol_for(&self, agent: &str) -> Result<Protocol> {
        let contract = harness_compat::embedded()?;
        let decision = contract.evaluate(Pairing {
            harness: agent,
            source: self.preset.source_id(),
            environment: Environment::Local,
            protocol: Some(self.protocol),
            model: Some(&self.model),
            exact: false,
            trust_saved_protocol: false,
        });
        ensure!(
            decision.status != Status::Excluded,
            "incompatible token provider: {}",
            decision.reason.unwrap_or("protocol-mismatch")
        );
        ensure!(
            contract.cli_slug(agent) == Some(agent),
            "use the canonical agent slug for a named token provider binding"
        );
        decision.protocol.ok_or_else(|| {
            anyhow::anyhow!("token provider has no concrete protocol for this agent")
        })
    }
}

pub(crate) fn resolve_bindings(
    entries: &[TokenProvider],
    bindings: HashMap<String, String>,
) -> Result<HashMap<String, TokenProvider>> {
    let mut providers = HashMap::new();
    for entry in entries {
        entry.validate()?;
        ensure!(
            providers.insert(entry.name.as_str(), entry).is_none(),
            "duplicate token provider name"
        );
    }
    bindings
        .into_iter()
        .map(|(agent, name)| {
            let mut binding = providers
                .get(name.as_str())
                .copied()
                .ok_or_else(|| {
                    anyhow::anyhow!("agent_token_providers references an undefined provider")
                })?
                .clone();
            binding.protocol = binding.protocol_for(&agent)?;
            Ok((agent, binding))
        })
        .collect()
}
