//! ACP agent registry
//!
//! Provides configuration for ACP agents (subprocess command and args)
//! with embedded provider info to avoid circular dependencies with core.

use anyhow::Result;
use std::time::Duration;

/// Default idle timeout for ACP streaming (5 minutes)
const DEFAULT_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Provider information embedded in ACP agent config.
/// This mirrors relevant fields from `ModelProviderInfo` to avoid circular dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpProviderInfo {
    /// Friendly display name (e.g., "Gemini ACP", "Mock ACP")
    pub name: String,
    /// Maximum number of request retries
    pub request_max_retries: u64,
    /// Maximum number of stream reconnection attempts
    pub stream_max_retries: u64,
    /// Idle timeout for streaming responses
    pub stream_idle_timeout: Duration,
}

impl Default for AcpProviderInfo {
    fn default() -> Self {
        Self {
            name: "ACP".to_string(),
            request_max_retries: 1,
            stream_max_retries: 1,
            stream_idle_timeout: DEFAULT_STREAM_IDLE_TIMEOUT,
        }
    }
}

/// Configuration for an ACP agent subprocess
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpAgentConfig {
    /// Provider identifier (e.g., "mock-acp", "gemini-acp")
    /// Used to determine when subprocess can be reused vs needs replacement
    pub provider_slug: String,
    /// Command to execute (binary path or command name)
    pub command: String,
    /// Arguments to pass to the command
    pub args: Vec<String>,
    /// Provider information for this ACP agent
    pub provider_info: AcpProviderInfo,
}

/// Get ACP agent configuration for a given model name
///
/// # Arguments
/// * `model_name` - The model identifier (e.g., "mock-model", "gemini-flash-2.5")
///   Names are normalized to lowercase for case-insensitive matching.
///
/// # Returns
/// Configuration with provider_slug, command and args to spawn the agent subprocess
///
/// # Errors
/// Returns error if model_name is not recognized
pub fn get_agent_config(model_name: &str) -> Result<AcpAgentConfig> {
    // Normalize model name: lowercase
    let normalized = model_name.to_lowercase();

    match normalized.as_str() {
        "mock-model" => {
            // Use full path to mock_acp_agent binary from target directory
            // This handles both debug and release builds
            let exe_path = match std::env::current_exe() {
                Ok(p) => {
                    let mock_path = p
                        .parent()
                        .map(|parent| parent.join("mock_acp_agent"))
                        .unwrap_or_else(|| std::path::PathBuf::from("mock_acp_agent"));
                    tracing::debug!("Mock ACP agent path resolved to: {}", mock_path.display());
                    mock_path
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to get current_exe for mock-model: {}, falling back to 'mock_acp_agent'",
                        e
                    );
                    std::path::PathBuf::from("mock_acp_agent")
                }
            };

            Ok(AcpAgentConfig {
                provider_slug: "mock-acp".to_string(),
                command: exe_path.to_string_lossy().to_string(),
                args: vec![],
                provider_info: AcpProviderInfo {
                    name: "Mock ACP".to_string(),
                    ..Default::default()
                },
            })
        }
        "gemini-2.5-flash" | "gemini-acp" => Ok(AcpAgentConfig {
            provider_slug: "gemini-acp".to_string(),
            command: "npx".to_string(),
            args: vec![
                "@google/gemini-cli".to_string(),
                "--experimental-acp".to_string(),
            ],
            provider_info: AcpProviderInfo {
                name: "Gemini ACP".to_string(),
                ..Default::default()
            },
        }),
        "claude-4.5" | "claude-acp" => Ok(AcpAgentConfig {
            provider_slug: "claude-acp".to_string(),
            command: "npx".to_string(),
            args: vec!["@zed-industries/claude-code-acp".to_string()],
            provider_info: AcpProviderInfo {
                name: "Claude ACP".to_string(),
                ..Default::default()
            },
        }),
        _ => anyhow::bail!("Unknown ACP model: {model_name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_mock_model_config() {
        let config = get_agent_config("mock-model").expect("Should return config for mock-model");

        assert_eq!(config.provider_slug, "mock-acp");
        assert!(
            config.command.contains("mock_acp_agent"),
            "Command should contain 'mock_acp_agent', got: {}",
            config.command
        );
        assert_eq!(config.args, Vec::<String>::new());
        assert_eq!(config.provider_info.name, "Mock ACP");
        assert_eq!(config.provider_info.request_max_retries, 1);
        assert_eq!(config.provider_info.stream_max_retries, 1);
    }

    #[test]
    fn test_get_gemini_model_config() {
        let config = get_agent_config("gemini-2.5-flash")
            .expect("Should return config for gemini-2.5-flash");

        assert_eq!(config.provider_slug, "gemini-acp");
        assert_eq!(config.command, "npx");
        assert_eq!(
            config.args,
            vec!["@google/gemini-cli", "--experimental-acp"]
        );
        assert_eq!(config.provider_info.name, "Gemini ACP");
    }

    #[test]
    fn test_get_claude_model_config() {
        let config = get_agent_config("claude-acp").expect("Should return config for claude-acp");

        assert_eq!(config.provider_slug, "claude-acp");
        assert_eq!(config.command, "npx");
        assert_eq!(config.args, vec!["@zed-industries/claude-code-acp"]);
        assert_eq!(config.provider_info.name, "Claude ACP");
    }

    #[test]
    fn test_get_unknown_model_returns_error() {
        let result = get_agent_config("unknown-model-xyz");

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("unknown-model-xyz"));
    }

    #[test]
    fn test_get_agent_config_normalizes_model_names() {
        // Should work with lowercase model names
        assert!(
            get_agent_config("gemini-2.5-flash").is_ok(),
            "Lowercase 'gemini-2.5-flash' should work"
        );
        assert!(
            get_agent_config("mock-model").is_ok(),
            "Lowercase 'mock-model' should work"
        );

        // Should work with mixed case (normalized to lowercase)
        let gemini_result = get_agent_config("Gemini-2.5-Flash");
        assert!(
            gemini_result.is_ok(),
            "Mixed case 'Gemini-2.5-Flash' should work"
        );
        assert_eq!(
            gemini_result.unwrap().provider_slug,
            "gemini-acp",
            "Should resolve to gemini-acp provider"
        );

        let mock_result = get_agent_config("Mock-Model");
        assert!(mock_result.is_ok(), "Mixed case 'Mock-Model' should work");
        assert_eq!(
            mock_result.unwrap().provider_slug,
            "mock-acp",
            "Should resolve to mock-acp provider"
        );

        // Should still reject unknown models
        let unknown_result = get_agent_config("unknown-model-xyz");
        assert!(unknown_result.is_err(), "Unknown model should return error");
        let err_msg = unknown_result.unwrap_err().to_string();
        assert!(
            err_msg.contains("unknown-model-xyz"),
            "Error message should contain original input"
        );
    }
}
