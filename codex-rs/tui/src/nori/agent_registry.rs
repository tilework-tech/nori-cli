//! ACP agent registry with multi-model support per agent.
//!
//! This module provides a structured registry of ACP agents that can be used
//! by the TUI to display available agents and their model variants.

use codex_acp::{get_agent_config, AcpAgentConfig};

/// An ACP agent with potentially multiple model variants.
#[derive(Debug, Clone)]
pub struct AcpAgent {
    /// Unique identifier for the agent (e.g., "claude-acp")
    pub id: &'static str,
    /// Display name shown in the UI (e.g., "Claude")
    pub display_name: &'static str,
    /// Short description of the agent
    pub description: &'static str,
    /// Available model variants for this agent
    pub models: Vec<AcpModelVariant>,
}

/// A specific model variant within an ACP agent.
#[derive(Debug, Clone)]
pub struct AcpModelVariant {
    /// Model slug used for backend configuration (e.g., "claude-4.5")
    pub model_slug: &'static str,
    /// Display name shown in the UI (e.g., "Claude Sonnet 4.5")
    pub display_name: &'static str,
    /// Whether this is the default model for the agent
    pub is_default: bool,
}

/// Returns all registered ACP agents.
///
/// Each agent may have multiple model variants that the user can choose from.
pub fn get_acp_agents() -> Vec<AcpAgent> {
    vec![
        AcpAgent {
            id: "claude-acp",
            display_name: "Claude",
            description: "Anthropic's Claude via Agent Client Protocol",
            models: vec![AcpModelVariant {
                model_slug: "claude-4.5",
                display_name: "Claude Sonnet 4.5",
                is_default: true,
            }],
        },
        AcpAgent {
            id: "gemini-acp",
            display_name: "Gemini",
            description: "Google's Gemini via Agent Client Protocol",
            models: vec![AcpModelVariant {
                model_slug: "gemini-2.5-flash",
                display_name: "Gemini 2.5 Flash",
                is_default: true,
            }],
        },
        AcpAgent {
            id: "mock-acp",
            display_name: "Mock ACP Agent",
            description: "Test agent for development and testing",
            models: vec![AcpModelVariant {
                model_slug: "mock-model",
                display_name: "Mock Model",
                is_default: true,
            }],
        },
    ]
}

/// Returns all ACP agents that should be shown in the picker UI.
///
/// This filters out agents that are only intended for testing/development
/// unless running in a development environment.
pub fn get_visible_acp_agents() -> Vec<AcpAgent> {
    let show_mock = std::env::var("NORI_SHOW_MOCK_AGENT").is_ok() || cfg!(debug_assertions);

    get_acp_agents()
        .into_iter()
        .filter(|agent| {
            if agent.id == "mock-acp" {
                show_mock
            } else {
                true
            }
        })
        .collect()
}

/// Resolves a model slug to its ACP agent configuration.
///
/// This wraps `codex_acp::get_agent_config` for consistency.
pub fn resolve_agent_config(model_slug: &str) -> Option<AcpAgentConfig> {
    get_agent_config(model_slug).ok()
}

/// Finds the ACP agent that contains the given model slug.
pub fn find_agent_for_model(model_slug: &str) -> Option<AcpAgent> {
    get_acp_agents()
        .into_iter()
        .find(|agent| agent.models.iter().any(|m| m.model_slug == model_slug))
}

/// Returns the default model variant for an agent.
pub fn get_default_model(agent: &AcpAgent) -> Option<&AcpModelVariant> {
    agent
        .models
        .iter()
        .find(|m| m.is_default)
        .or_else(|| agent.models.first())
}

/// Checks if a model slug corresponds to a registered ACP agent.
pub fn is_acp_model(model_slug: &str) -> bool {
    resolve_agent_config(model_slug).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_acp_agents_returns_agents() {
        let agents = get_acp_agents();
        assert!(!agents.is_empty());
        assert!(agents.iter().any(|a| a.id == "claude-acp"));
        assert!(agents.iter().any(|a| a.id == "gemini-acp"));
    }

    #[test]
    fn test_find_agent_for_model() {
        let agent = find_agent_for_model("claude-4.5");
        assert!(agent.is_some());
        assert_eq!(agent.unwrap().id, "claude-acp");
    }

    #[test]
    fn test_find_agent_for_unknown_model() {
        let agent = find_agent_for_model("unknown-model");
        assert!(agent.is_none());
    }

    #[test]
    fn test_get_default_model() {
        let agents = get_acp_agents();
        let claude = agents.iter().find(|a| a.id == "claude-acp").unwrap();
        let default = get_default_model(claude);
        assert!(default.is_some());
        assert!(default.unwrap().is_default);
    }

    #[test]
    fn test_is_acp_model() {
        assert!(is_acp_model("mock-model"));
        assert!(is_acp_model("claude-4.5"));
        assert!(is_acp_model("gemini-2.5-flash"));
        assert!(!is_acp_model("gpt-5.1-codex"));
    }
}
