use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;

use serde::Deserializer;
use serde::de::Error as SerdeError;

use strum_macros::Display;
use strum_macros::EnumIter;
use ts_rs::TS;

/// See https://platform.openai.com/docs/guides/reasoning?api-mode=responses#get-started-with-reasoning
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Display,
    JsonSchema,
    TS,
    EnumIter,
    Hash,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ReasoningEffort {
    None,
    Minimal,
    Low,
    #[default]
    Medium,
    High,
    XHigh,
}

/// A summary of the reasoning performed by the model. This can be useful for
/// debugging and understanding the model's reasoning process.
/// See https://platform.openai.com/docs/guides/reasoning?api-mode=responses#reasoning-summaries
#[derive(
    Debug, Serialize, Deserialize, Default, Clone, Copy, PartialEq, Eq, Display, JsonSchema, TS,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ReasoningSummary {
    #[default]
    Auto,
    Concise,
    Detailed,
    /// Option to disable reasoning summaries.
    None,
}

/// Controls output length/detail on GPT-5 models via the Responses API.
/// Serialized with lowercase values to match the OpenAI API.
#[derive(
    Hash,
    Debug,
    Serialize,
    Deserialize,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Display,
    JsonSchema,
    TS,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Verbosity {
    Low,
    #[default]
    Medium,
    High,
}

#[derive(
    Deserialize, Debug, Clone, Copy, PartialEq, Default, Serialize, Display, JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum SandboxMode {
    #[serde(rename = "read-only")]
    #[default]
    ReadOnly,

    #[serde(rename = "workspace-write")]
    WorkspaceWrite,

    #[serde(rename = "danger-full-access")]
    DangerFullAccess,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Display, JsonSchema, TS)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ForcedLoginMethod {
    Chatgpt,
    Api,
}

/// Represents the trust level for a project directory.
/// This determines the approval policy and sandbox mode applied.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Display, JsonSchema, TS)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum TrustLevel {
    Trusted,
    Untrusted,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct McpServerConfig {
    #[serde(flatten)]
    pub transport: McpServerTransportConfig,

    /// When `false`, Codex skips initializing this MCP server.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Startup timeout in seconds for initializing MCP server & initially listing tools.
    #[serde(
        default,
        with = "option_duration_secs",
        skip_serializing_if = "Option::is_none"
    )]
    pub startup_timeout_sec: Option<Duration>,

    /// Default timeout for MCP tool calls initiated via this server.
    #[serde(default, with = "option_duration_secs")]
    pub tool_timeout_sec: Option<Duration>,

    /// Explicit allow-list of tools exposed from this server. When set, only these tools will be registered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_tools: Option<Vec<String>>,

    /// Explicit deny-list of tools. These tools will be removed after applying `enabled_tools`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_tools: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for McpServerConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize, Clone)]
        struct RawMcpServerConfig {
            // stdio
            command: Option<String>,
            #[serde(default)]
            args: Option<Vec<String>>,
            #[serde(default)]
            env: Option<HashMap<String, String>>,
            #[serde(default)]
            env_vars: Option<Vec<String>>,
            http_headers: Option<HashMap<String, String>>,
            #[serde(default)]
            env_http_headers: Option<HashMap<String, String>>,

            // streamable_http
            url: Option<String>,
            bearer_token: Option<String>,
            bearer_token_env_var: Option<String>,
            #[serde(default)]
            client_id: Option<String>,
            #[serde(default)]
            client_secret_env_var: Option<String>,

            // shared
            #[serde(default)]
            startup_timeout_sec: Option<f64>,
            #[serde(default)]
            startup_timeout_ms: Option<u64>,
            #[serde(default, with = "option_duration_secs")]
            tool_timeout_sec: Option<Duration>,
            #[serde(default)]
            enabled: Option<bool>,
            #[serde(default)]
            enabled_tools: Option<Vec<String>>,
            #[serde(default)]
            disabled_tools: Option<Vec<String>>,
        }

        let mut raw = RawMcpServerConfig::deserialize(deserializer)?;

        let startup_timeout_sec = match (raw.startup_timeout_sec, raw.startup_timeout_ms) {
            (Some(sec), _) => {
                let duration = Duration::try_from_secs_f64(sec).map_err(SerdeError::custom)?;
                Some(duration)
            }
            (None, Some(ms)) => Some(Duration::from_millis(ms)),
            (None, None) => None,
        };
        let tool_timeout_sec = raw.tool_timeout_sec;
        let enabled = raw.enabled.unwrap_or_else(default_enabled);
        let enabled_tools = raw.enabled_tools.clone();
        let disabled_tools = raw.disabled_tools.clone();

        fn throw_if_set<E, T>(transport: &str, field: &str, value: Option<&T>) -> Result<(), E>
        where
            E: SerdeError,
        {
            if value.is_none() {
                return Ok(());
            }
            Err(E::custom(format!(
                "{field} is not supported for {transport}",
            )))
        }

        let transport = if let Some(command) = raw.command.clone() {
            throw_if_set("stdio", "url", raw.url.as_ref())?;
            throw_if_set(
                "stdio",
                "bearer_token_env_var",
                raw.bearer_token_env_var.as_ref(),
            )?;
            throw_if_set("stdio", "bearer_token", raw.bearer_token.as_ref())?;
            throw_if_set("stdio", "http_headers", raw.http_headers.as_ref())?;
            throw_if_set("stdio", "env_http_headers", raw.env_http_headers.as_ref())?;
            throw_if_set("stdio", "client_id", raw.client_id.as_ref())?;
            throw_if_set(
                "stdio",
                "client_secret_env_var",
                raw.client_secret_env_var.as_ref(),
            )?;
            McpServerTransportConfig::Stdio {
                command,
                args: raw.args.clone().unwrap_or_default(),
                env: raw.env.clone(),
                env_vars: raw.env_vars.clone().unwrap_or_default(),
            }
        } else if let Some(url) = raw.url.clone() {
            throw_if_set("streamable_http", "args", raw.args.as_ref())?;
            throw_if_set("streamable_http", "env", raw.env.as_ref())?;
            throw_if_set("streamable_http", "env_vars", raw.env_vars.as_ref())?;
            throw_if_set("streamable_http", "bearer_token", raw.bearer_token.as_ref())?;
            McpServerTransportConfig::StreamableHttp {
                url,
                bearer_token_env_var: raw.bearer_token_env_var.clone(),
                http_headers: raw.http_headers.clone(),
                env_http_headers: raw.env_http_headers.take(),
                client_id: raw.client_id.clone(),
                client_secret_env_var: raw.client_secret_env_var.clone(),
            }
        } else {
            return Err(SerdeError::custom("invalid transport"));
        };

        Ok(Self {
            transport,
            startup_timeout_sec,
            tool_timeout_sec,
            enabled,
            enabled_tools,
            disabled_tools,
        })
    }
}

const fn default_enabled() -> bool {
    true
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged, deny_unknown_fields, rename_all = "snake_case")]
pub enum McpServerTransportConfig {
    /// https://modelcontextprotocol.io/specification/2025-06-18/basic/transports#stdio
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        env: Option<HashMap<String, String>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        env_vars: Vec<String>,
    },
    /// https://modelcontextprotocol.io/specification/2025-06-18/basic/transports#streamable-http
    StreamableHttp {
        url: String,
        /// Name of the environment variable to read for an HTTP bearer token.
        /// When set, requests will include the token via `Authorization: Bearer <token>`.
        /// The actual secret value must be provided via the environment.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bearer_token_env_var: Option<String>,
        /// Additional HTTP headers to include in requests to this server.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        http_headers: Option<HashMap<String, String>>,
        /// HTTP headers where the value is sourced from an environment variable.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        env_http_headers: Option<HashMap<String, String>>,
        /// Pre-registered OAuth client ID for servers that do not support
        /// dynamic client registration (e.g., Slack).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client_id: Option<String>,
        /// Name of the environment variable holding the OAuth client secret.
        /// Used together with `client_id` for servers that require a
        /// confidential OAuth client.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client_secret_env_var: Option<String>,
    },
}

mod option_duration_secs {
    use serde::Deserialize;
    use serde::Deserializer;
    use serde::Serializer;
    use std::time::Duration;

    pub fn serialize<S>(value: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(duration) => serializer.serialize_some(&duration.as_secs_f64()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = Option::<f64>::deserialize(deserializer)?;
        secs.map(|secs| Duration::try_from_secs_f64(secs).map_err(serde::de::Error::custom))
            .transpose()
    }
}
