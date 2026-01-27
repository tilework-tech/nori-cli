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
    let notification_timeout = config.notification_timeout;

    let items: Vec<SelectionItem> = vec![
        build_toggle_item(
            "Vertical Footer",
            "Stack footer segments vertically instead of horizontally",
            vertical_footer_enabled,
            {
                let tx = app_event_tx.clone();
                let new_value = !vertical_footer_enabled;
                move || {
                    tx.send(AppEvent::SetConfigVerticalFooter(new_value));
                }
            },
        ),
        build_cycle_item(
            "Notification Timeout",
            "Duration for desktop notifications (select to cycle)",
            &notification_timeout.to_string(),
            {
                let tx = app_event_tx;
                let next_value = notification_timeout.next();
                move || {
                    tx.send(AppEvent::SetConfigNotificationTimeout(next_value));
                }
            },
        ),
    ];

    SelectionViewParams {
        title: Some("Configuration".to_string()),
        subtitle: Some("Toggle TUI settings (changes saved to config.toml)".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        ..Default::default()
    }
}

/// Build a cycle-style selection item that advances through values on each select.
fn build_cycle_item<F>(
    name: &str,
    description: &str,
    current_value: &str,
    on_cycle: F,
) -> SelectionItem
where
    F: Fn() + Send + Sync + 'static,
{
    let display_name = format!("{name} ({current_value})");

    let actions: Vec<SelectionAction> = vec![Box::new(move |_tx| {
        on_cycle();
    })];

    SelectionItem {
        name: display_name,
        description: Some(description.to_string()),
        is_current: false,
        actions,
        dismiss_on_select: true,
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
            notification_timeout: codex_acp::config::NotificationTimeout::default(),
            nori_home: PathBuf::from("/tmp/test-nori"),
            cwd: PathBuf::from("/tmp"),
            mcp_servers: std::collections::HashMap::new(),
        }
    }

    fn make_test_config_with_timeout(
        timeout: codex_acp::config::NotificationTimeout,
    ) -> NoriConfig {
        NoriConfig {
            notification_timeout: timeout,
            ..make_test_config(false)
        }
    }

    #[test]
    fn config_picker_returns_two_items() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx);

        assert_eq!(params.items.len(), 2);
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

    #[test]
    fn config_picker_shows_notification_timeout_item() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config =
            make_test_config_with_timeout(codex_acp::config::NotificationTimeout::ThirtySeconds);

        let params = config_picker_params(&config, tx);

        let timeout_item = &params.items[1];
        assert!(
            timeout_item.name.contains("Notification Timeout"),
            "expected item name to contain 'Notification Timeout', got: {}",
            timeout_item.name
        );
        assert!(
            timeout_item.name.contains("30s"),
            "expected item name to contain '30s', got: {}",
            timeout_item.name
        );
    }

    #[test]
    fn config_picker_notification_timeout_shows_disabled() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config =
            make_test_config_with_timeout(codex_acp::config::NotificationTimeout::Disabled);

        let params = config_picker_params(&config, tx);

        let timeout_item = &params.items[1];
        assert!(
            timeout_item.name.contains("disabled"),
            "expected item name to contain 'disabled', got: {}",
            timeout_item.name
        );
    }

    #[test]
    fn config_picker_notification_timeout_action_sends_next_value() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config =
            make_test_config_with_timeout(codex_acp::config::NotificationTimeout::FiveSeconds);

        let params = config_picker_params(&config, tx.clone());

        let timeout_item = &params.items[1];
        assert!(timeout_item.name.contains("Notification Timeout"));
        for action in &timeout_item.actions {
            action(&tx);
        }

        let event = rx.try_recv().expect("should receive event");
        match event {
            AppEvent::SetConfigNotificationTimeout(value) => {
                // Was FiveSeconds, next should be TenSeconds
                assert_eq!(
                    value,
                    codex_acp::config::NotificationTimeout::TenSeconds,
                    "expected next value after FiveSeconds to be TenSeconds"
                );
            }
            _ => panic!("expected SetConfigNotificationTimeout event"),
        }
    }
}
