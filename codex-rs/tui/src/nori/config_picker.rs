//! Config picker component for Nori TUI settings.
//!
//! This module provides the UI for modifying TUI configuration settings
//! that are persisted to ~/.nori/cli/config.toml.

use codex_acp::config::NoriConfig;
use codex_acp::config::NotifyAfterIdle;
use codex_acp::config::OsNotifications;
use codex_acp::config::TerminalNotifications;

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
    let terminal_notifications_enabled =
        config.terminal_notifications == TerminalNotifications::Enabled;
    let os_notifications_enabled = config.os_notifications == OsNotifications::Enabled;

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
        build_toggle_item(
            "Terminal Notifications",
            "Send OSC 9 escape sequences to notify the terminal on events",
            terminal_notifications_enabled,
            {
                let tx = app_event_tx.clone();
                let new_value = !terminal_notifications_enabled;
                move || {
                    tx.send(AppEvent::SetConfigTerminalNotifications(new_value));
                }
            },
        ),
        build_toggle_item(
            "OS Notifications",
            "Send native desktop notifications on events",
            os_notifications_enabled,
            {
                let tx = app_event_tx;
                let new_value = !os_notifications_enabled;
                move || {
                    tx.send(AppEvent::SetConfigOsNotifications(new_value));
                }
            },
        ),
        {
            let current_idle = config.notify_after_idle;
            let display_name = format!("Notify After Idle ({})", current_idle.display_name());
            let actions: Vec<SelectionAction> = vec![Box::new({
                move |tx| {
                    tx.send(AppEvent::OpenNotifyAfterIdlePicker);
                }
            })];
            SelectionItem {
                name: display_name,
                description: Some(
                    "How long to wait before sending an idle notification".to_string(),
                ),
                is_current: false,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        },
        {
            let actions: Vec<SelectionAction> = vec![Box::new({
                move |tx| {
                    tx.send(AppEvent::OpenHotkeyPicker);
                }
            })];
            SelectionItem {
                name: "Hotkeys".to_string(),
                description: Some("Configure keyboard shortcuts for actions".to_string()),
                is_current: false,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        },
    ];

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

/// Create selection view parameters for the notify-after-idle sub-picker.
///
/// # Arguments
/// * `current` - The currently selected NotifyAfterIdle variant
/// * `app_event_tx` - The app event sender for triggering config change events
pub fn notify_after_idle_picker_params(
    current: NotifyAfterIdle,
    _app_event_tx: AppEventSender,
) -> SelectionViewParams {
    let items: Vec<SelectionItem> = NotifyAfterIdle::all_variants()
        .iter()
        .map(|&variant| {
            let is_current = variant == current;
            let actions: Vec<SelectionAction> = vec![Box::new({
                move |tx| {
                    tx.send(AppEvent::SetConfigNotifyAfterIdle(variant));
                }
            })];
            SelectionItem {
                name: variant.display_name().to_string(),
                description: None,
                is_current,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        })
        .collect();

    SelectionViewParams {
        title: Some("Notify After Idle".to_string()),
        subtitle: Some("Select idle notification delay".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use codex_acp::config::OsNotifications;
    use codex_acp::config::TerminalNotifications;
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
            terminal_notifications: TerminalNotifications::Enabled,
            os_notifications: OsNotifications::Enabled,
            vertical_footer,
            notify_after_idle: codex_acp::config::NotifyAfterIdle::FiveSeconds,
            hotkeys: codex_acp::config::HotkeyConfig::default(),
            nori_home: PathBuf::from("/tmp/test-nori"),
            cwd: PathBuf::from("/tmp"),
            mcp_servers: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn config_picker_returns_expected_items() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx);

        assert_eq!(params.items.len(), 5);
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
    fn config_picker_returns_five_items() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx);

        assert_eq!(params.items.len(), 5);
        // The 4th item should be Notify After Idle
        assert!(params.items[3].name.contains("Notify After Idle"));
        // The 5th item should be Hotkeys
        assert!(params.items[4].name.contains("Hotkeys"));
    }

    #[test]
    fn config_picker_notify_after_idle_shows_current_value() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx);

        // Default config has FiveSeconds, so should show "5 seconds"
        let idle_item = &params.items[3];
        assert!(
            idle_item.name.contains("5 seconds"),
            "Expected '5 seconds' in name, got: {}",
            idle_item.name
        );
    }

    #[test]
    fn config_picker_notify_after_idle_action_sends_open_picker_event() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx.clone());

        // Trigger the notify after idle action (4th item)
        let idle_item = &params.items[3];
        for action in &idle_item.actions {
            action(&tx);
        }

        let event = rx.try_recv().expect("should receive event");
        assert!(
            matches!(event, AppEvent::OpenNotifyAfterIdlePicker),
            "expected OpenNotifyAfterIdlePicker event, got: {event:?}"
        );
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
    fn config_picker_hotkeys_action_sends_open_picker_event() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx.clone());

        // Trigger the hotkeys action (5th item)
        let hotkeys_item = &params.items[4];
        assert!(hotkeys_item.name.contains("Hotkeys"));
        for action in &hotkeys_item.actions {
            action(&tx);
        }

        let event = rx.try_recv().expect("should receive event");
        assert!(
            matches!(event, AppEvent::OpenHotkeyPicker),
            "expected OpenHotkeyPicker event, got: {event:?}"
        );
    }

    #[test]
    fn notify_after_idle_picker_returns_five_items() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params =
            notify_after_idle_picker_params(codex_acp::config::NotifyAfterIdle::FiveSeconds, tx);

        assert_eq!(params.items.len(), 5);
        assert!(params.title.unwrap().contains("Notify After Idle"));
    }

    #[test]
    fn notify_after_idle_picker_marks_current_variant() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params =
            notify_after_idle_picker_params(codex_acp::config::NotifyAfterIdle::ThirtySeconds, tx);

        // Only the "30 seconds" item should be marked current
        for item in &params.items {
            if item.name.contains("30 seconds") {
                assert!(item.is_current, "30 seconds should be marked current");
            } else {
                assert!(
                    !item.is_current,
                    "{} should not be marked current",
                    item.name
                );
            }
        }
    }

    #[test]
    fn notify_after_idle_picker_action_sends_set_event() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params = notify_after_idle_picker_params(
            codex_acp::config::NotifyAfterIdle::FiveSeconds,
            tx.clone(),
        );

        // Select the "1 minute" option (index 3)
        let minute_item = &params.items[3];
        assert!(minute_item.name.contains("1 minute"));
        for action in &minute_item.actions {
            action(&tx);
        }

        let event = rx.try_recv().expect("should receive event");
        match event {
            AppEvent::SetConfigNotifyAfterIdle(value) => {
                assert_eq!(value, codex_acp::config::NotifyAfterIdle::SixtySeconds);
            }
            _ => panic!("expected SetConfigNotifyAfterIdle event, got: {event:?}"),
        }
    }
}
