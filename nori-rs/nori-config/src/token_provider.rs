//! Named token providers: API endpoints an agent reaches with a key the user
//! keeps in an environment variable.

use serde::Deserialize;
use serde::Serialize;

/// One `[token_providers.<id>]` entry. The table key is the provider id that
/// agents bind to through `[agent_token_providers]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenProviderToml {
    /// Catalog preset this provider is built from, for example `subconscious`.
    pub preset: String,
    /// Name of the environment variable that holds the API key. The key
    /// itself is never written to config.
    pub api_key_env: String,
    /// Replaces the preset's endpoint, for example a regional or proxy URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}
