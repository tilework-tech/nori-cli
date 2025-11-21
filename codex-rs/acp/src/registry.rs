//! ACP agent registry
//!
//! Provides configuration for ACP agents (subprocess command and args)
//! without requiring changes to core ModelProviderInfo struct.

use anyhow::Result;

/// Configuration for an ACP agent subprocess
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpAgentConfig {
    /// Command to execute (binary path or command name)
    pub command: String,
    /// Arguments to pass to the command
    pub args: Vec<String>,
}

/// Get ACP agent configuration for a given provider name
///
/// # Arguments
/// * `provider_name` - The provider identifier (e.g., "mock-acp", "gemini-acp")
///
/// # Returns
/// Configuration with command and args to spawn the agent subprocess
///
/// # Errors
/// Returns error if provider_name is not recognized
pub fn get_agent_config(provider_name: &str) -> Result<AcpAgentConfig> {
    match provider_name {
        "mock-acp" => Ok(AcpAgentConfig {
            command: "mock_acp_agent".to_string(),
            args: vec![],
        }),
        "gemini-acp" => Ok(AcpAgentConfig {
            command: "npx".to_string(),
            args: vec![
                "@google/gemini-cli".to_string(),
                "--experimental-acp".to_string(),
            ],
        }),
        _ => anyhow::bail!("Unknown ACP provider: {provider_name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_mock_acp_agent_config() {
        let config = get_agent_config("mock-acp").expect("Should return mock-acp config");

        assert_eq!(config.command, "mock_acp_agent");
        assert_eq!(config.args, Vec::<String>::new());
    }

    #[test]
    fn test_get_gemini_acp_agent_config() {
        let config = get_agent_config("gemini-acp").expect("Should return gemini-acp config");

        assert_eq!(config.command, "npx");
        assert_eq!(
            config.args,
            vec!["@google/gemini-cli", "--experimental-acp"]
        );
    }

    #[test]
    fn test_get_unknown_provider_returns_error() {
        let result = get_agent_config("unknown-provider");

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("unknown-provider"));
    }
}
