//! ACP agent picker component for the TUI.
//!
//! This module provides the agent selection popup for switching between
//! registered ACP agents. The selection is tracked as "pending" until
//! a new prompt is submitted, at which point the agent subprocess is switched.

use codex_acp::AcpAgentConfig;
use codex_acp::get_agent_config;

/// Information about an available ACP agent for display in the picker.
#[derive(Debug, Clone)]
pub struct AcpAgentInfo {
    /// Model identifier (e.g., "claude-4.5", "gemini-2.5-flash")
    pub model: String,
    /// Display name for the UI (e.g., "Claude ACP", "Gemini ACP")
    pub display_name: String,
    /// Short description of the agent
    pub description: String,
    /// Provider slug used for subprocess management
    pub provider_slug: String,
}

impl AcpAgentInfo {
    /// Create agent info from an ACP agent config and model name.
    pub fn from_config(model: &str, config: &AcpAgentConfig) -> Self {
        let description = match config.provider_slug.as_str() {
            "mock-acp" => "Mock agent for testing purposes.".to_string(),
            "gemini-acp" => "Google's experimental thinking model via ACP.".to_string(),
            "claude-acp" => "Anthropic's Claude via Agent Context Protocol.".to_string(),
            _ => format!("{} agent", config.provider_info.name),
        };

        Self {
            model: model.to_string(),
            display_name: config.provider_info.name.clone(),
            description,
            provider_slug: config.provider_slug.clone(),
        }
    }
}

/// Known ACP agent models that can be selected.
/// These correspond to the models registered in the ACP registry.
pub const ACP_AGENT_MODELS: &[&str] = &[
    "mock-model",
    "gemini-2.5-flash",
    "claude-4.5",
];

/// Get a list of all available ACP agents with their info.
pub fn available_acp_agents() -> Vec<AcpAgentInfo> {
    ACP_AGENT_MODELS
        .iter()
        .filter_map(|model| {
            get_agent_config(model)
                .ok()
                .map(|config| AcpAgentInfo::from_config(model, &config))
        })
        .collect()
}

/// Check if a model is an ACP agent model.
pub fn is_acp_agent_model(model: &str) -> bool {
    get_agent_config(model).is_ok()
}

/// Pending agent selection state.
/// This tracks when a user has selected a new agent but hasn't
/// submitted a prompt yet to trigger the switch.
#[derive(Debug, Clone, Default)]
pub struct PendingAgentSelection {
    /// The model to switch to when the next prompt is submitted.
    pub model: Option<String>,
}

impl PendingAgentSelection {
    /// Create a new empty pending selection.
    pub fn new() -> Self {
        Self { model: None }
    }

    /// Set a pending agent selection.
    pub fn set(&mut self, model: String) {
        self.model = Some(model);
    }

    /// Clear the pending selection and return the model if any.
    pub fn take(&mut self) -> Option<String> {
        self.model.take()
    }

    /// Check if there's a pending selection.
    pub fn has_pending(&self) -> bool {
        self.model.is_some()
    }

    /// Get the pending model without clearing it.
    pub fn pending_model(&self) -> Option<&str> {
        self.model.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available_acp_agents() {
        let agents = available_acp_agents();
        // Should have at least the mock agent
        assert!(!agents.is_empty(), "Should have at least one ACP agent");

        // Check that mock agent is present
        let mock = agents.iter().find(|a| a.model == "mock-model");
        assert!(mock.is_some(), "Mock agent should be available");
        if let Some(mock) = mock {
            assert_eq!(mock.display_name, "Mock ACP");
            assert_eq!(mock.provider_slug, "mock-acp");
        }
    }

    #[test]
    fn test_is_acp_agent_model() {
        assert!(is_acp_agent_model("mock-model"));
        assert!(is_acp_agent_model("gemini-2.5-flash"));
        assert!(is_acp_agent_model("claude-4.5"));
        assert!(!is_acp_agent_model("gpt-5.1-codex"));
        assert!(!is_acp_agent_model("unknown-model"));
    }

    #[test]
    fn test_pending_agent_selection() {
        let mut pending = PendingAgentSelection::new();
        assert!(!pending.has_pending());
        assert!(pending.pending_model().is_none());

        pending.set("claude-4.5".to_string());
        assert!(pending.has_pending());
        assert_eq!(pending.pending_model(), Some("claude-4.5"));

        let taken = pending.take();
        assert_eq!(taken, Some("claude-4.5".to_string()));
        assert!(!pending.has_pending());
    }
}
