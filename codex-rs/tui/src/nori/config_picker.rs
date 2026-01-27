//! Config picker component for Nori TUI settings.
//!
//! This module provides the UI for modifying TUI configuration settings
//! that are persisted to ~/.nori/cli/config.toml.

use codex_acp::config::NoriConfig;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;

/// Create selection view parameters for the config picker.
///
/// # Arguments
/// * `config` - The current Nori configuration
/// * `app_event_tx` - The app event sender for triggering config change events
pub fn config_picker_params(
    config: &NoriConfig,
    app_event_tx: AppEventSender,
) -> SelectionViewParams {
    let vertical_footer_enabled = config.vertical_footer;

    let items: Vec<SelectionItem> = vec![build_toggle_item(
        "Vertical Footer",
        "Stack footer segments vertically instead of horizontally",
        vertical_footer_enabled,
        {
            let tx = app_event_tx;
            let new_value = !vertical_footer_enabled;
            move || {
                tx.send(AppEvent::SetConfigVerticalFooter(new_value));
            }
        },
    )];

    SelectionViewParams {
        title: Some("Configuration".to_string()),
        subtitle: Some("Toggle TUI settings (changes saved to config.toml)".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        ..Default::default()
    }
}

/// Build a toggle-style selection item.
fn build_toggle_item<F>(
    name: &str,
    description: &str,
    is_enabled: bool,
    on_toggle: F,
) -> SelectionItem
where
    F: Fn() + Send + Sync + 'static,
{
    let status = if is_enabled { "on" } else { "off" };
    let display_name = format!("{name} ({status})");

    let actions: Vec<SelectionAction> = vec![Box::new(move |_tx| {
        on_toggle();
    })];

    SelectionItem {
        name: display_name,
        description: Some(description.to_string()),
        is_current: is_enabled,
        actions,
        dismiss_on_select: true,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use std::path::PathBuf;
    use tokio::sync::mpsc::unbounded_channel;

    fn make_test_config(vertical_footer: bool) -> NoriConfig {
        NoriConfig {
            agent: "claude-code".to_string(),
            model: "claude-code".to_string(),
            sandbox_mode: codex_protocol::config_types::SandboxMode::WorkspaceWrite,
            approval_policy: codex_acp::config::ApprovalPolicy::OnRequest,
            history_persistence: codex_acp::config::HistoryPersistence::SaveAll,
            animations: true,
            notifications: true,
            vertical_footer,
            nori_home: PathBuf::from("/tmp/test-nori"),
            cwd: PathBuf::from("/tmp"),
            mcp_servers: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn config_picker_returns_one_item() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx);

        assert_eq!(params.items.len(), 1);
        assert!(params.title.is_some());
        assert!(params.title.unwrap().contains("Configuration"));
    }

    #[test]
    fn config_picker_shows_current_state_on() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(true);

        let params = config_picker_params(&config, tx);

        assert!(params.items[0].name.contains("(on)"));
    }

    #[test]
    fn config_picker_shows_current_state_off() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx);

        assert!(params.items[0].name.contains("(off)"));
    }

    #[test]
    fn config_picker_vertical_footer_action_sends_correct_event() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx.clone());

        // Trigger the vertical footer toggle action (first item)
        let vertical_footer_item = &params.items[0];
        assert!(vertical_footer_item.name.contains("Vertical Footer"));
        for action in &vertical_footer_item.actions {
            action(&tx);
        }

        // Verify the event was sent with the toggled value
        let event = rx.try_recv().expect("should receive event");
        match event {
            AppEvent::SetConfigVerticalFooter(value) => {
                // Was false, should toggle to true
                assert!(value, "vertical_footer was off, should toggle to on");
            }
            _ => panic!("expected SetConfigVerticalFooter event"),
        }
    }
}
