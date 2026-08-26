//! Agent picker component for ACP mode.
//!
//! This module provides the UI for selecting between available ACP agents.
//! Selecting an agent immediately starts preparation of a live candidate.

use nori_harness::AcpAgentInfo;
use nori_harness::list_available_agents;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;

/// Create selection view parameters for the agent picker.
///
/// # Arguments
/// * `current_agent` - The currently active agent name
/// * `app_event_tx` - The app event sender for triggering selection events
pub fn agent_picker_params(
    current_agent: &str,
    _app_event_tx: AppEventSender,
    recording_enabled: bool,
) -> SelectionViewParams {
    let available_agents = list_available_agents();
    let current_normalized = current_agent.to_lowercase();

    let items: Vec<SelectionItem> = available_agents
        .into_iter()
        .map(|agent| {
            let is_current = agent.agent_name.to_lowercase() == current_normalized;
            let agent_name = agent.agent_name.clone();
            let display_name = agent.display_name.clone();

            // Start preparing the selected agent as soon as the picker closes.
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(AppEvent::PrepareAgentCandidate {
                    agent_name: agent_name.clone(),
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
        subtitle: Some(
            "Creates new conversation with selected agent (history not preserved)".to_string(),
        ),
        footer_hint: Some(standard_popup_hint_line()),
        footer_hint_right: Some(recording_footer_hint(recording_enabled)),
        on_shift_tab: Some(Box::new(move |tx| {
            tx.send(AppEvent::SetConfigAcpWireRecording(!recording_enabled));
        })),
        items,
        ..Default::default()
    }
    .picker()
}

fn recording_footer_hint(enabled: bool) -> Line<'static> {
    let (symbol, status, action) = if enabled {
        ("●", "on", "disable")
    } else {
        ("○", "off", "enable")
    };
    let symbol_span: Span<'static> = if enabled { symbol.red() } else { symbol.into() };

    Line::from(vec![
        "Recording: ".dim(),
        symbol_span,
        format!(" {status}  Shift-Tab to {action}").dim(),
    ])
}

/// Create selection view parameters for the model picker in ACP mode.
/// Shows models as disabled with a message to use /agent instead.
///
/// This is the fallback when model state is not available.
pub fn acp_model_picker_params() -> SelectionViewParams {
    // In ACP mode, we show a message that models are not directly selectable
    // and users should use /agent instead
    let items: Vec<SelectionItem> = vec![SelectionItem {
        name: "Model switching disabled in ACP mode".to_string(),
        description: Some("Use /agent to switch between ACP agents".to_string()),
        is_current: false,
        actions: vec![],
        dismiss_on_select: true,
        ..Default::default()
    }];

    SelectionViewParams {
        title: Some("Select Model".to_string()),
        subtitle: Some("Not available in ACP mode - use /agent instead".to_string()),
        footer_hint: Some(Line::from(
            "Press esc to dismiss, or use /agent to switch agents.",
        )),
        items,
        ..Default::default()
    }
    .picker()
}

/// Get information about an agent by agent name
#[allow(dead_code)]
pub fn get_agent_info(agent_name: &str) -> Option<AcpAgentInfo> {
    let normalized = agent_name.to_lowercase();
    list_available_agents()
        .into_iter()
        .find(|agent| agent.agent_name.to_lowercase() == normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use ratatui::style::Color;
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn test_agent_picker_params_lists_available_agents() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params = agent_picker_params("mock-model", tx, false);

        assert!(params.title.is_some());
        assert!(params.title.unwrap().contains("Select Agent"));
        assert!(!params.items.is_empty());

        // Should have mock-model as current
        let mock_agent = params.items.iter().find(|i| i.name == "Mock ACP");
        assert!(mock_agent.is_some());
        assert!(mock_agent.unwrap().is_current);
    }

    #[test]
    fn agent_picker_footer_shows_recording_disabled_and_shift_tab_toggles_on() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params = agent_picker_params("mock-model", tx.clone(), false);

        let footer = params.footer_hint_right.as_ref().expect("recording footer");
        assert_eq!(footer.to_string(), "Recording: ○ off  Shift-Tab to enable");

        let action = params.on_shift_tab.as_ref().expect("shift-tab action");
        action(&tx);
        let event = rx.try_recv().expect("shift-tab event");
        assert!(
            matches!(event, AppEvent::SetConfigAcpWireRecording(true)),
            "expected SetConfigAcpWireRecording(true), got: {event:?}"
        );
    }

    #[test]
    fn recording_footer_uses_red_filled_dot_when_enabled() {
        let footer = recording_footer_hint(true);
        assert_eq!(footer.to_string(), "Recording: ● on  Shift-Tab to disable");
        let dot = footer
            .spans
            .iter()
            .find(|span| span.content == "●")
            .expect("filled recording dot");
        assert_eq!(dot.style.fg, Some(Color::Red));
    }

    #[test]
    fn test_acp_model_picker_shows_disabled() {
        let params = acp_model_picker_params();

        assert!(params.title.is_some());
        assert!(params.subtitle.is_some());
        assert!(params.subtitle.unwrap().contains("Not available"));
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
