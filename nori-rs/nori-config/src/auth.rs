//! Per-agent choice between the agent's own login and a saved API key.

use serde::Deserialize;

use crate::types::NoriConfig;

/// `[auth.<agent-slug>]` in config.toml.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentAuthPreference {
    /// Pass the API key saved through `nori auth set-key` to this agent at
    /// launch. Off by default, so the agent's own login (such as a
    /// subscription) is used.
    pub prefer_api_key: bool,
    /// The user accepted that preferring the API key bills usage to the API
    /// account instead of a detected subscription. Remembered so the warning
    /// is shown once.
    pub api_billing_confirmed: bool,
}

impl NoriConfig {
    /// The auth preference for a canonical agent slug.
    pub fn auth_preference(&self, agent: &str) -> AgentAuthPreference {
        self.auth.get(agent).copied().unwrap_or_default()
    }
}
