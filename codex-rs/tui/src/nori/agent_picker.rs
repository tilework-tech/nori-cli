//! Agent picker component for ACP mode.
//!
//! This module provides the UI for selecting between available ACP agents.
//! Agent selection is tracked as "pending" and the actual switch happens
//! on the next prompt submission to avoid disrupting active prompt turns.

use codex_acp::AcpAgentInfo;
use codex_acp::list_available_agents;
use ratatui::text::Line;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;

/// Information about a pending agent selection.
/// This struct is stored in the App to track which agent should be switched to
/// when the user submits their next prompt.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PendingAgentSelection {
    /// The model name of the selected agent (e.g., "mock-model", "gemini-2.5-flash")
    pub model_name: String,
    /// The display name for the status indicator
    pub display_name: String,
}

/// Create selection view parameters for the agent picker.
///
/// # Arguments
/// * `current_model` - The currently active model name
/// * `app_event_tx` - The app event sender for triggering selection events
pub fn agent_picker_params(
    current_model: &str,
    _app_event_tx: AppEventSender,
) -> SelectionViewParams {
    let available_agents = list_available_agents();
    let current_normalized = current_model.to_lowercase();

    let items: Vec<SelectionItem> = available_agents
        .into_iter()
        .map(|agent| {
            let is_current = agent.model_name.to_lowercase() == current_normalized;
            let model_name = agent.model_name.clone();
            let display_name = agent.display_name.clone();

            // Create action that sends the pending agent selection event
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(AppEvent::SetPendingAgent {
                    model_name: model_name.clone(),
                    display_name: display_name.clone(),
                });
            })];

            SelectionItem {
                name: agent.display_name,
                description: Some(agent.description),
                is_current,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        })
        .collect();

    SelectionViewParams {
        title: Some("Select Agent".to_string()),
        subtitle: Some("Agent will switch on next prompt submission".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        ..Default::default()
    }
}

/// State for ACP model selection, passed to the model picker.
#[derive(Debug, Clone)]
pub struct AcpModelState {
    /// Available models from the agent
    pub available_models: Vec<crate::app_event::AcpModelInfo>,
    /// Currently selected model ID
    pub current_model_id: String,
}

/// Create selection view parameters for the model picker in ACP mode.
///
/// If `model_state` is `Some`, shows the actual available models from the agent.
/// If `None`, shows a disabled message indicating model switching is not supported.
pub fn acp_model_picker_params(model_state: Option<&AcpModelState>) -> SelectionViewParams {
    match model_state {
        Some(state) if !state.available_models.is_empty() => {
            // Agent supports model switching - show actual models
            let items: Vec<SelectionItem> = state
                .available_models
                .iter()
                .map(|model| {
                    let is_current = model.id == state.current_model_id;
                    let model_id = model.id.clone();
                    let display_name = model.display_name.clone();

                    // Create action that sends the model switch event
                    let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                        tx.send(crate::app_event::AppEvent::SetAcpModel {
                            model_id: model_id.clone(),
                            display_name: display_name.clone(),
                        });
                    })];

                    SelectionItem {
                        name: model.display_name.clone(),
                        description: Some(model.id.clone()),
                        is_current,
                        actions,
                        dismiss_on_select: true,
                        ..Default::default()
                    }
                })
                .collect();

            SelectionViewParams {
                title: Some("Select Model".to_string()),
                subtitle: Some("Switch to a different model in this agent".to_string()),
                footer_hint: Some(standard_popup_hint_line()),
                items,
                ..Default::default()
            }
        }
        _ => {
            // Agent does not support model switching
            let items: Vec<SelectionItem> = vec![SelectionItem {
                name: "Model switching not available".to_string(),
                description: Some("This agent does not support model switching".to_string()),
                is_current: false,
                actions: vec![],
                dismiss_on_select: true,
                ..Default::default()
            }];

            SelectionViewParams {
                title: Some("Select Model".to_string()),
                subtitle: Some("Not available for this agent".to_string()),
                footer_hint: Some(Line::from("Press esc to dismiss.")),
                items,
                ..Default::default()
            }
        }
    }
}

/// Get information about an agent by model name
#[allow(dead_code)]
pub fn get_agent_info(model_name: &str) -> Option<AcpAgentInfo> {
    let normalized = model_name.to_lowercase();
    list_available_agents()
        .into_iter()
        .find(|agent| agent.model_name.to_lowercase() == normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn test_agent_picker_params_lists_available_agents() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params = agent_picker_params("mock-model", tx);

        assert!(params.title.is_some());
        assert!(params.title.unwrap().contains("Select Agent"));
        assert!(!params.items.is_empty());

        // Should have mock-model as current
        let mock_agent = params.items.iter().find(|i| i.name == "Mock ACP");
        assert!(mock_agent.is_some());
        assert!(mock_agent.unwrap().is_current);
    }

    #[test]
    fn test_acp_model_picker_shows_disabled_when_no_models() {
        let params = acp_model_picker_params(None);

        assert!(params.title.is_some());
        assert!(params.subtitle.is_some());
        assert!(params.subtitle.unwrap().contains("Not available"));
    }

    #[test]
    fn test_acp_model_picker_shows_models_when_available() {
        use crate::app_event::AcpModelInfo;

        let state = AcpModelState {
            available_models: vec![
                AcpModelInfo {
                    id: "model-a".to_string(),
                    display_name: "Model A".to_string(),
                },
                AcpModelInfo {
                    id: "model-b".to_string(),
                    display_name: "Model B".to_string(),
                },
            ],
            current_model_id: "model-a".to_string(),
        };
        let params = acp_model_picker_params(Some(&state));

        assert!(params.title.is_some());
        assert!(params.subtitle.unwrap().contains("Switch"));
        assert_eq!(params.items.len(), 2);

        // First model should be current
        assert!(params.items[0].is_current);
        assert!(!params.items[1].is_current);
    }

    #[test]
    fn test_get_agent_info() {
        let info = get_agent_info("mock-model");
        assert!(info.is_some());
        assert_eq!(info.unwrap().display_name, "Mock ACP");

        let info = get_agent_info("Mock-Model"); // Case insensitive
        assert!(info.is_some());

        let info = get_agent_info("unknown-agent");
        assert!(info.is_none());
    }
}
