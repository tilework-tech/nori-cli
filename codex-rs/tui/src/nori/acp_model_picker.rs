//! ACP model picker component for the TUI.
//!
//! This module provides a model selection popup for ACP mode that shows
//! all models as disabled (not selectable) since the agent-context-protocol
//! calls to switch the model are not yet supported.
//!
//! Users should use the `/agent` command to switch between ACP agents instead.

use crate::bottom_pane::popup_consts::standard_popup_hint_line;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;

/// Information about a model option in the ACP model picker.
#[derive(Debug, Clone)]
pub struct AcpModelOption {
    /// Model identifier
    pub model: String,
    /// Display name for the UI
    pub display_name: String,
    /// Description shown in the picker
    pub description: String,
    /// Whether this is the currently active model
    pub is_current: bool,
}

/// Create selection view parameters for the ACP model picker.
///
/// All options are shown as disabled since model switching via ACP
/// is not yet supported. Users should use `/agent` instead.
pub fn create_acp_model_picker_params(
    current_model: &str,
    options: Vec<AcpModelOption>,
) -> SelectionViewParams {
    let items: Vec<SelectionItem> = options
        .into_iter()
        .map(|opt| {
            let is_current = opt.model == current_model || opt.is_current;
            let name = if is_current {
                format!("{} (current)", opt.display_name)
            } else {
                format!("{} (disabled)", opt.display_name)
            };

            SelectionItem {
                name,
                description: Some(opt.description),
                selected_description: if is_current {
                    None
                } else {
                    Some("Model switching is not yet supported in ACP mode. Use /agent to switch agents.".to_string())
                },
                is_current,
                // No actions since model switching is disabled
                actions: vec![],
                dismiss_on_select: true,
                ..Default::default()
            }
        })
        .collect();

    SelectionViewParams {
        title: Some("Model Selection (ACP Mode)".to_string()),
        subtitle: Some(
            "Model switching is disabled in ACP mode. Use /agent to switch between agents."
                .to_string(),
        ),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        ..Default::default()
    }
}

/// Get the default ACP model options based on what's available in the registry.
pub fn default_acp_model_options(current_model: &str) -> Vec<AcpModelOption> {
    use super::acp_agent_picker::available_acp_agents;

    available_acp_agents()
        .into_iter()
        .map(|agent| AcpModelOption {
            model: agent.model.clone(),
            display_name: agent.display_name,
            description: agent.description,
            is_current: agent.model == current_model,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_acp_model_picker_params() {
        let options = vec![
            AcpModelOption {
                model: "mock-model".to_string(),
                display_name: "Mock ACP".to_string(),
                description: "Mock agent for testing.".to_string(),
                is_current: true,
            },
            AcpModelOption {
                model: "claude-4.5".to_string(),
                display_name: "Claude ACP".to_string(),
                description: "Anthropic's Claude.".to_string(),
                is_current: false,
            },
        ];

        let params = create_acp_model_picker_params("mock-model", options);

        assert_eq!(params.title, Some("Model Selection (ACP Mode)".to_string()));
        assert!(params.subtitle.is_some());
        assert_eq!(params.items.len(), 2);

        // First item should be current
        assert!(params.items[0].is_current);
        assert!(params.items[0].name.contains("(current)"));

        // Second item should be disabled
        assert!(!params.items[1].is_current);
        assert!(params.items[1].name.contains("(disabled)"));
        assert!(params.items[1].selected_description.is_some());
    }

    #[test]
    fn test_default_acp_model_options() {
        let options = default_acp_model_options("mock-model");
        assert!(!options.is_empty());

        // Check that current model is marked
        let current = options.iter().find(|o| o.model == "mock-model");
        assert!(current.is_some());
        assert!(current.unwrap().is_current);
    }
}
