//! ACP agent picker UI component.
//!
//! Provides functions to build selection view parameters for the ACP agent
//! and model selection flow. This follows the same two-stage pattern as
//! the HTTP model picker (agent selection → model variant selection).

use crate::app_event::AppEvent;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;
use crate::bottom_pane::{SelectionAction, SelectionItem, SelectionViewParams};

use super::agent_registry::{get_default_model, get_visible_acp_agents, AcpAgent, AcpModelVariant};

/// Builds selection view parameters for the ACP agent picker (stage 1).
///
/// This creates a list of available ACP agents. When the user selects an agent,
/// it either:
/// - Opens the model variant picker if the agent has multiple models
/// - Directly applies the default model if the agent has only one model
pub fn build_agent_selection(current_model: &str) -> SelectionViewParams {
    let agents = get_visible_acp_agents();

    let items: Vec<SelectionItem> = agents
        .into_iter()
        .map(|agent| {
            let is_current = agent
                .models
                .iter()
                .any(|m| m.model_slug == current_model);

            let has_single_model = agent.models.len() == 1;
            let agent_for_action = agent.clone();

            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                if has_single_model {
                    // Single model: apply directly
                    if let Some(model) = agent_for_action.models.first() {
                        apply_acp_model_selection(tx, model.model_slug);
                    }
                } else {
                    // Multiple models: open model picker
                    tx.send(AppEvent::OpenAcpModelPopup {
                        agent: agent_for_action.clone(),
                    });
                }
            })];

            SelectionItem {
                name: agent.display_name.to_string(),
                description: Some(agent.description.to_string()),
                is_current,
                actions,
                dismiss_on_select: has_single_model,
                ..Default::default()
            }
        })
        .collect();

    SelectionViewParams {
        title: Some("Select Agent".to_string()),
        subtitle: Some("Choose an ACP agent to use for this session".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        ..Default::default()
    }
}

/// Builds selection view parameters for the ACP model variant picker (stage 2).
///
/// This creates a list of available model variants for a specific agent.
/// When the user selects a model, it is applied immediately.
pub fn build_model_selection(agent: &AcpAgent, current_model: &str) -> SelectionViewParams {
    let default_model = get_default_model(agent);

    let items: Vec<SelectionItem> = agent
        .models
        .iter()
        .map(|model| {
            let is_current = model.model_slug == current_model;
            let is_default = default_model.map(|d| d.model_slug == model.model_slug).unwrap_or(false);

            let mut name = model.display_name.to_string();
            if is_default {
                name.push_str(" (default)");
            }

            let model_slug = model.model_slug;
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                apply_acp_model_selection(tx, model_slug);
            })];

            SelectionItem {
                name,
                description: None,
                is_current,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        })
        .collect();

    let initial_selected_idx = agent
        .models
        .iter()
        .position(|m| m.model_slug == current_model)
        .or_else(|| agent.models.iter().position(|m| m.is_default));

    SelectionViewParams {
        title: Some(format!("Select {} Model", agent.display_name)),
        subtitle: Some(format!("Choose a model variant for {}", agent.display_name)),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        initial_selected_idx,
        ..Default::default()
    }
}

/// Helper function to apply an ACP model selection.
///
/// This sends the necessary events to update the model configuration
/// and persist the selection.
fn apply_acp_model_selection(tx: &crate::app_event_sender::AppEventSender, model_slug: &str) {
    use codex_core::protocol::Op;

    // Update the turn context with the new model
    tx.send(AppEvent::CodexOp(Op::OverrideTurnContext {
        cwd: None,
        approval_policy: None,
        sandbox_policy: None,
        model: Some(model_slug.to_string()),
        effort: None, // ACP agents don't use reasoning effort
        summary: None,
    }));

    // Update the UI model display
    tx.send(AppEvent::UpdateModel(model_slug.to_string()));

    // Persist the selection (without reasoning effort for ACP)
    tx.send(AppEvent::PersistModelSelection {
        model: model_slug.to_string(),
        effort: None,
    });

    tracing::info!("Selected ACP model: {}", model_slug);
}

/// Directly applies an ACP agent's default model.
///
/// This is useful when the agent is known and we want to skip the picker UI.
pub fn apply_agent_default(
    tx: &crate::app_event_sender::AppEventSender,
    agent: &AcpAgent,
) -> Option<&'static str> {
    if let Some(model) = get_default_model(agent) {
        apply_acp_model_selection(tx, model.model_slug);
        Some(model.model_slug)
    } else {
        None
    }
}

/// Returns an `AcpModelVariant` reference for UI display purposes.
pub fn find_current_model_variant<'a>(
    agent: &'a AcpAgent,
    model_slug: &str,
) -> Option<&'a AcpModelVariant> {
    agent.models.iter().find(|m| m.model_slug == model_slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_agent_selection_has_items() {
        let params = build_agent_selection("unknown-model");
        assert!(!params.items.is_empty());
        assert_eq!(params.title, Some("Select Agent".to_string()));
    }

    #[test]
    fn test_build_agent_selection_marks_current() {
        let params = build_agent_selection("claude-4.5");
        let claude_item = params
            .items
            .iter()
            .find(|item| item.name == "Claude")
            .expect("Claude agent should be in list");
        assert!(claude_item.is_current);
    }

    #[test]
    fn test_build_model_selection_for_agent() {
        use super::super::agent_registry::get_acp_agents;

        let agents = get_acp_agents();
        let claude = agents.iter().find(|a| a.id == "claude-acp").unwrap();

        let params = build_model_selection(claude, "claude-4.5");
        assert!(!params.items.is_empty());
        assert!(params.title.unwrap().contains("Claude"));
    }
}
