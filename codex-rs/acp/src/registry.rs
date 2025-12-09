//! ACP agent registry
//!
//! Provides configuration for ACP agents (subprocess command and args)
//! with embedded provider info to avoid circular dependencies with core.

use anyhow::Result;
use std::time::Duration;

/// Information about an available ACP agent for display in the picker
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpAgentInfo {
    /// Model name used to select this agent (e.g., "mock-model", "gemini-2.5-flash")
    pub model_name: String,
    /// Display name shown in the picker
    pub display_name: String,
    /// Description of the agent
    pub description: String,
    /// Provider slug for this agent
    pub provider_slug: String,
}

/// Get list of all available ACP agents for the agent picker
pub fn list_available_agents() -> Vec<AcpAgentInfo> {
    let mut agents = Vec::new();

    // Mock agents are only available in debug builds (for testing)
    #[cfg(debug_assertions)]
    {
        agents.push(AcpAgentInfo {
            model_name: "mock-model".to_string(),
            display_name: "Mock ACP".to_string(),
            description: "Mock agent for testing".to_string(),
            provider_slug: "mock-acp".to_string(),
        });
        agents.push(AcpAgentInfo {
            model_name: "mock-model-alt".to_string(),
            display_name: "Mock ACP Alt".to_string(),
            description: "Alternate mock agent for testing".to_string(),
            provider_slug: "mock-acp-alt".to_string(),
        });
    }

    // Production agents (always available)
    agents.push(AcpAgentInfo {
        model_name: "claude-acp".to_string(),
        display_name: "Claude".to_string(),
        description: "Anthropic Claude via ACP".to_string(),
        provider_slug: "claude-acp".to_string(),
    });
    agents.push(AcpAgentInfo {
        model_name: "codex-acp".to_string(),
        display_name: "Codex".to_string(),
        description: "OpenAI Codex via ACP".to_string(),
        provider_slug: "codex-acp".to_string(),
    });
    agents.push(AcpAgentInfo {
        model_name: "gemini-acp".to_string(),
        display_name: "Gemini".to_string(),
        description: "Google Gemini via ACP".to_string(),
        provider_slug: "gemini-acp".to_string(),
    });

    agents
}

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

    // Try mock agents first (only available in debug builds)
    #[cfg(debug_assertions)]
    if let Some(config) = get_mock_agent_config(&normalized) {
        return Ok(config);
    }

    // Production agents
    match normalized.as_str() {
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
        "codex-acp" => Ok(AcpAgentConfig {
            provider_slug: "codex-acp".to_string(),
            command: "npx".to_string(),
            args: vec!["@zed-industries/codex-acp".to_string()],
            provider_info: AcpProviderInfo {
                name: "Codex ACP".to_string(),
                ..Default::default()
            },
        }),
        _ => anyhow::bail!("Unknown ACP model: {model_name}"),
    }
}

/// Get mock agent configuration (only available in debug builds)
#[cfg(debug_assertions)]
fn get_mock_agent_config(normalized: &str) -> Option<AcpAgentConfig> {
    match normalized {
        "mock-model" => {
            // Resolve path to mock_acp_agent binary.
            //
            // Priority:
            // 1. MOCK_ACP_AGENT_BIN environment variable (set by CI)
            // 2. Relative to current executable (for local development)
            let exe_path = if let Ok(env_path) = std::env::var("MOCK_ACP_AGENT_BIN") {
                tracing::debug!("Mock ACP agent path from MOCK_ACP_AGENT_BIN: {}", env_path);
                std::path::PathBuf::from(env_path)
            } else {
                // Fall back to resolving relative to current executable.
                // This handles both:
                // - Running as `codex` binary: target/{profile}/codex -> target/{profile}/
                // - Running as test binary: target/{profile}/deps/test -> target/{profile}/
                let mock_path = std::env::current_exe()
                    .ok()
                    .and_then(|p| {
                        p.parent().map(|parent| {
                            // Check if we're in a "deps" directory (test binary context)
                            let in_deps_dir = parent
                                .file_name()
                                .map(|name| name == "deps")
                                .unwrap_or(false);

                            if in_deps_dir {
                                parent.parent().map(|p| p.join("mock_acp_agent"))
                            } else {
                                Some(parent.join("mock_acp_agent"))
                            }
                        })
                    })
                    .flatten()
                    .unwrap_or_else(|| std::path::PathBuf::from("mock_acp_agent"));
                tracing::debug!("Mock ACP agent path resolved to: {}", mock_path.display());
                mock_path
            };

            Some(AcpAgentConfig {
                provider_slug: "mock-acp".to_string(),
                command: exe_path.to_string_lossy().to_string(),
                args: vec![],
                provider_info: AcpProviderInfo {
                    name: "Mock ACP".to_string(),
                    ..Default::default()
                },
            })
        }
        "mock-model-alt" => {
            // Alternate mock model for E2E testing agent switching.
            // Uses the same binary as mock-model but with a different provider_slug
            // to verify that different agent configurations spawn separate subprocesses.
            let exe_path = if let Ok(env_path) = std::env::var("MOCK_ACP_AGENT_BIN") {
                tracing::debug!("Mock ACP agent path from MOCK_ACP_AGENT_BIN: {}", env_path);
                std::path::PathBuf::from(env_path)
            } else {
                let mock_path = std::env::current_exe()
                    .ok()
                    .and_then(|p| {
                        p.parent().map(|parent| {
                            let in_deps_dir = parent
                                .file_name()
                                .map(|name| name == "deps")
                                .unwrap_or(false);

                            if in_deps_dir {
                                parent.parent().map(|p| p.join("mock_acp_agent"))
                            } else {
                                Some(parent.join("mock_acp_agent"))
                            }
                        })
                    })
                    .flatten()
                    .unwrap_or_else(|| std::path::PathBuf::from("mock_acp_agent"));
                tracing::debug!("Mock ACP agent path resolved to: {}", mock_path.display());
                mock_path
            };

            Some(AcpAgentConfig {
                provider_slug: "mock-acp-alt".to_string(),
                command: exe_path.to_string_lossy().to_string(),
                args: vec![],
                provider_info: AcpProviderInfo {
                    name: "Mock ACP Alt".to_string(),
                    ..Default::default()
                },
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(debug_assertions)]
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
    #[cfg(debug_assertions)]
    fn test_get_mock_model_alt_config() {
        let config =
            get_agent_config("mock-model-alt").expect("Should return config for mock-model-alt");

        assert_eq!(config.provider_slug, "mock-acp-alt");
        assert!(
            config.command.contains("mock_acp_agent"),
            "Command should contain 'mock_acp_agent', got: {}",
            config.command
        );
        assert_eq!(config.args, Vec::<String>::new());
        assert_eq!(config.provider_info.name, "Mock ACP Alt");
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
    fn test_get_codex_model_config() {
        let config = get_agent_config("codex-acp").expect("Should return config for codex-acp");

        assert_eq!(config.provider_slug, "codex-acp");
        assert_eq!(config.command, "npx");
        assert_eq!(config.args, vec!["@zed-industries/codex-acp"]);
        assert_eq!(config.provider_info.name, "Codex ACP");
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

        // Should still reject unknown models
        let unknown_result = get_agent_config("unknown-model-xyz");
        assert!(unknown_result.is_err(), "Unknown model should return error");
        let err_msg = unknown_result.unwrap_err().to_string();
        assert!(
            err_msg.contains("unknown-model-xyz"),
            "Error message should contain original input"
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_list_available_agents_debug_build() {
        let agents = list_available_agents();
        // Debug build should have 5 agents: mock, mock-alt, claude, codex, gemini
        assert_eq!(agents.len(), 5, "Debug build should have 5 agents");

        let names: Vec<&str> = agents.iter().map(|a| a.display_name.as_str()).collect();
        assert!(names.contains(&"Mock ACP"), "Should have Mock ACP");
        assert!(names.contains(&"Mock ACP Alt"), "Should have Mock ACP Alt");
        assert!(names.contains(&"Claude"), "Should have Claude");
        assert!(names.contains(&"Codex"), "Should have Codex");
        assert!(names.contains(&"Gemini"), "Should have Gemini");
    }

    #[test]
    fn test_list_available_agents_contains_production_agents() {
        let agents = list_available_agents();
        let names: Vec<&str> = agents.iter().map(|a| a.display_name.as_str()).collect();

        // Production agents should always be present
        assert!(names.contains(&"Claude"), "Should have Claude");
        assert!(names.contains(&"Codex"), "Should have Codex");
        assert!(names.contains(&"Gemini"), "Should have Gemini");
    }
}
