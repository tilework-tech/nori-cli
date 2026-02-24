//! Config picker component for Nori TUI settings.
//!
//! This module provides the UI for modifying TUI configuration settings
//! that are persisted to ~/.nori/cli/config.toml.

use codex_acp::config::FooterSegment;
use codex_acp::config::FooterSegmentConfig;
use codex_acp::config::NoriConfig;
use codex_acp::config::NotifyAfterIdle;
use codex_acp::config::OsNotifications;
use codex_acp::config::ScriptTimeout;
use codex_acp::config::TerminalNotifications;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;
use crate::nori::skillset_picker;

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
                let tx = app_event_tx.clone();
                let new_value = !os_notifications_enabled;
                move || {
                    tx.send(AppEvent::SetConfigOsNotifications(new_value));
                }
            },
        ),
        build_toggle_item(
            "Vim Mode",
            "Enable vim-style navigation in the textarea (Escape enters normal mode)",
            config.vim_mode,
            {
                let tx = app_event_tx.clone();
                let new_value = !config.vim_mode;
                move || {
                    tx.send(AppEvent::SetConfigVimMode(new_value));
                }
            },
        ),
        build_toggle_item(
            "Auto Worktree",
            "Automatically create a git worktree at session start",
            config.auto_worktree,
            {
                let tx = app_event_tx.clone();
                let new_value = !config.auto_worktree;
                move || {
                    tx.send(AppEvent::SetConfigAutoWorktree(new_value));
                }
            },
        ),
        {
            let skillset_per_session = config.skillset_per_session;
            let status = if skillset_per_session { "on" } else { "off" };
            let display_name = format!("Per Session Skillsets ({status})");
            let tx = app_event_tx;
            let actions: Vec<SelectionAction> = vec![Box::new(move |_tx_arg| {
                if skillset_per_session {
                    // Toggle off
                    tx.send(AppEvent::SetConfigSkillsetPerSession(false));
                } else if !skillset_picker::is_nori_skillsets_available() {
                    // nori-skillsets not available, show info message
                    tx.send(AppEvent::InsertHistoryCell(Box::new(
                        crate::history_cell::new_error_event(
                            skillset_picker::not_installed_message(),
                        ),
                    )));
                } else {
                    // Open the worktree choice modal
                    tx.send(AppEvent::OpenSkillsetPerSessionWorktreeChoice);
                }
            })];
            SelectionItem {
                name: display_name,
                description: Some("Use unique skillsets for each session".to_string()),
                is_current: skillset_per_session,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        },
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
        {
            let current_timeout = config.script_timeout.clone();
            let display_name = format!("Script Timeout ({})", current_timeout.display_name());
            let actions: Vec<SelectionAction> = vec![Box::new({
                move |tx| {
                    tx.send(AppEvent::OpenScriptTimeoutPicker);
                }
            })];
            SelectionItem {
                name: display_name,
                description: Some("Timeout for custom prompt script execution".to_string()),
                is_current: false,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        },
        {
            let current_loop = config.loop_count;
            let display_name = match current_loop {
                Some(n) => format!("Loop Count ({n})"),
                None => "Loop Count (Disabled)".to_string(),
            };
            let actions: Vec<SelectionAction> = vec![Box::new({
                move |tx| {
                    tx.send(AppEvent::OpenLoopCountPicker);
                }
            })];
            SelectionItem {
                name: display_name,
                description: Some(
                    "Number of times to re-run the first prompt in fresh sessions".to_string(),
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
                    tx.send(AppEvent::OpenFooterSegmentsPicker);
                }
            })];
            SelectionItem {
                name: "Footer Segments".to_string(),
                description: Some("Configure which segments are shown in the footer".to_string()),
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
        initial_selected_idx: Some(0),
        ..Default::default()
    }
}

/// Create selection view parameters for the skillset per-session worktree choice.
///
/// Presents two options: enable per-session skillsets with or without auto-worktrees.
///
/// # Arguments
/// * `app_event_tx` - The app event sender for triggering config change events
pub fn skillset_worktree_choice_params(app_event_tx: AppEventSender) -> SelectionViewParams {
    let tx_with = app_event_tx.clone();
    let tx_without = app_event_tx;

    let items: Vec<SelectionItem> = vec![
        SelectionItem {
            name: "With Auto Worktrees".to_string(),
            description: Some(
                "Each session gets an isolated git worktree. Skillsets are installed per-worktree."
                    .to_string(),
            ),
            is_current: false,
            actions: vec![Box::new(move |_tx| {
                tx_with.send(AppEvent::SetConfigSkillsetPerSession(true));
                tx_with.send(AppEvent::SetConfigAutoWorktree(true));
            })],
            dismiss_on_select: true,
            ..Default::default()
        },
        SelectionItem {
            name: "Without Auto Worktrees".to_string(),
            description: Some(
                "Skillsets are installed in the current directory. You are responsible for managing installed skillset files."
                    .to_string(),
            ),
            is_current: false,
            actions: vec![Box::new(move |_tx| {
                tx_without.send(AppEvent::SetConfigSkillsetPerSession(true));
            })],
            dismiss_on_select: true,
            ..Default::default()
        },
    ];

    SelectionViewParams {
        title: Some("Per Session Skillsets".to_string()),
        subtitle: Some("Choose how skillsets are managed per session".to_string()),
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

/// Create selection view parameters for the script timeout sub-picker.
///
/// # Arguments
/// * `current` - The currently configured ScriptTimeout
/// * `_app_event_tx` - The app event sender for triggering config change events
pub fn script_timeout_picker_params(
    current: ScriptTimeout,
    _app_event_tx: AppEventSender,
) -> SelectionViewParams {
    let items: Vec<SelectionItem> = ScriptTimeout::all_common_values()
        .into_iter()
        .map(|variant| {
            let is_current = variant == current;
            let actions: Vec<SelectionAction> = vec![Box::new({
                let variant = variant.clone();
                move |tx| {
                    tx.send(AppEvent::SetConfigScriptTimeout(variant.clone()));
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
        title: Some("Script Timeout".to_string()),
        subtitle: Some("Select script execution timeout".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        ..Default::default()
    }
}

/// Create selection view parameters for the footer segments sub-picker.
///
/// Each segment can be toggled on/off. The order of segments is controlled
/// via the TOML config file (not via this picker).
///
/// # Arguments
/// * `current` - The current footer segment configuration
/// * `_app_event_tx` - The app event sender for triggering config change events
pub fn footer_segments_picker_params(
    current: &FooterSegmentConfig,
    _app_event_tx: AppEventSender,
) -> SelectionViewParams {
    let items: Vec<SelectionItem> = FooterSegment::all_variants()
        .iter()
        .map(|&segment| {
            let is_enabled = current.is_enabled(segment);
            let status = if is_enabled { "on" } else { "off" };
            let name = format!("{} ({})", segment.display_name(), status);

            let actions: Vec<SelectionAction> = vec![Box::new({
                let new_value = !is_enabled;
                move |tx| {
                    tx.send(AppEvent::SetConfigFooterSegment(segment, new_value));
                }
            })];

            SelectionItem {
                name,
                description: None,
                is_current: is_enabled,
                actions,
                dismiss_on_select: false, // Keep picker open for toggling multiple segments
                ..Default::default()
            }
        })
        .collect();

    SelectionViewParams {
        title: Some("Footer Segments".to_string()),
        subtitle: Some("Toggle which segments appear in the footer".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        initial_selected_idx: Some(0),
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
            active_agent: "claude-code".to_string(),
            sandbox_mode: codex_protocol::config_types::SandboxMode::WorkspaceWrite,
            approval_policy: codex_acp::config::ApprovalPolicy::OnRequest,
            history_persistence: codex_acp::config::HistoryPersistence::SaveAll,
            animations: true,
            terminal_notifications: TerminalNotifications::Enabled,
            os_notifications: OsNotifications::Enabled,
            vertical_footer,
            notify_after_idle: codex_acp::config::NotifyAfterIdle::FiveSeconds,
            vim_mode: false,
            hotkeys: codex_acp::config::HotkeyConfig::default(),
            script_timeout: codex_acp::config::ScriptTimeout::default(),
            loop_count: None,
            auto_worktree: false,
            footer_segment_config: FooterSegmentConfig::default(),
            nori_home: PathBuf::from("/tmp/test-nori"),
            cwd: PathBuf::from("/tmp"),
            mcp_servers: std::collections::HashMap::new(),
            session_start_hooks: vec![],
            session_end_hooks: vec![],
            pre_user_prompt_hooks: vec![],
            post_user_prompt_hooks: vec![],
            pre_tool_call_hooks: vec![],
            post_tool_call_hooks: vec![],
            pre_agent_response_hooks: vec![],
            post_agent_response_hooks: vec![],
            async_session_start_hooks: vec![],
            async_session_end_hooks: vec![],
            async_pre_user_prompt_hooks: vec![],
            async_post_user_prompt_hooks: vec![],
            async_pre_tool_call_hooks: vec![],
            async_post_tool_call_hooks: vec![],
            async_pre_agent_response_hooks: vec![],
            async_post_agent_response_hooks: vec![],
            default_models: std::collections::HashMap::new(),
            agents: vec![],
            skillset_per_session: false,
        }
    }

    #[test]
    fn config_picker_returns_expected_items() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx);

        assert_eq!(params.items.len(), 11);
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
    fn config_picker_returns_eight_items() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx);

        assert_eq!(params.items.len(), 11);
        // The 4th item should be Vim Mode
        assert!(params.items[3].name.contains("Vim Mode"));
        // The 5th item should be Auto Worktree
        assert!(params.items[4].name.contains("Auto Worktree"));
        // The 6th item should be Per Session Skillsets
        assert!(params.items[5].name.contains("Per Session Skillsets"));
        // The 7th item should be Notify After Idle
        assert!(params.items[6].name.contains("Notify After Idle"));
        // The 8th item should be Hotkeys
        assert!(params.items[7].name.contains("Hotkeys"));
        // The 9th item should be Script Timeout
        assert!(params.items[8].name.contains("Script Timeout"));
        // The 10th item should be Loop Count
        assert!(params.items[9].name.contains("Loop Count"));
    }

    #[test]
    fn config_picker_notify_after_idle_shows_current_value() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx);

        // Default config has FiveSeconds, so should show "5 seconds"
        let idle_item = &params.items[6];
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

        // Trigger the notify after idle action (7th item, index 6)
        let idle_item = &params.items[6];
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

        // Trigger the hotkeys action (8th item, index 7)
        let hotkeys_item = &params.items[7];
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

    #[test]
    fn config_picker_includes_vim_mode_toggle() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx);

        // Should now have 11 items (includes vim mode, auto worktree, per session skillsets, script timeout, and loop count)
        assert_eq!(params.items.len(), 11);
        // Find the vim mode item
        let vim_mode_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Vim Mode"));
        assert!(
            vim_mode_item.is_some(),
            "config picker should include Vim Mode toggle"
        );
    }

    #[test]
    fn config_picker_vim_mode_shows_current_state() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut config = make_test_config(false);
        config.vim_mode = true;

        let params = config_picker_params(&config, tx);

        let vim_mode_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Vim Mode"))
            .expect("should have vim mode item");
        assert!(
            vim_mode_item.name.contains("(on)"),
            "vim mode should show (on) when enabled"
        );
    }

    #[test]
    fn config_picker_vim_mode_action_sends_correct_event() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut config = make_test_config(false);
        config.vim_mode = false;

        let params = config_picker_params(&config, tx.clone());

        let vim_mode_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Vim Mode"))
            .expect("should have vim mode item");

        // Trigger the action
        for action in &vim_mode_item.actions {
            action(&tx);
        }

        let event = rx.try_recv().expect("should receive event");
        match event {
            AppEvent::SetConfigVimMode(value) => {
                // Was false, should toggle to true
                assert!(value, "vim_mode was off, should toggle to on");
            }
            _ => panic!("expected SetConfigVimMode event, got: {event:?}"),
        }
    }

    #[test]
    fn config_picker_script_timeout_shows_current_value() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx);

        // Default config has 30s timeout
        let timeout_item = &params.items[8];
        assert!(
            timeout_item.name.contains("30s"),
            "Expected '30s' in name, got: {}",
            timeout_item.name
        );
    }

    #[test]
    fn config_picker_script_timeout_action_sends_open_picker_event() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx.clone());

        // Trigger the script timeout action (9th item, index 8)
        let timeout_item = &params.items[8];
        assert!(timeout_item.name.contains("Script Timeout"));
        for action in &timeout_item.actions {
            action(&tx);
        }

        let event = rx.try_recv().expect("should receive event");
        assert!(
            matches!(event, AppEvent::OpenScriptTimeoutPicker),
            "expected OpenScriptTimeoutPicker event, got: {event:?}"
        );
    }

    #[test]
    fn script_timeout_picker_returns_five_items() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params = script_timeout_picker_params(codex_acp::config::ScriptTimeout::default(), tx);

        assert_eq!(params.items.len(), 5);
        assert!(params.title.unwrap().contains("Script Timeout"));
    }

    #[test]
    fn script_timeout_picker_marks_current_value() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params =
            script_timeout_picker_params(codex_acp::config::ScriptTimeout::from_str("1m"), tx);

        for item in &params.items {
            if item.name == "1m" {
                assert!(item.is_current, "1m should be marked current");
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
    fn script_timeout_picker_action_sends_set_event() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params =
            script_timeout_picker_params(codex_acp::config::ScriptTimeout::default(), tx.clone());

        // Select the "2m" option (index 3)
        let two_min_item = &params.items[3];
        assert_eq!(two_min_item.name, "2m");
        for action in &two_min_item.actions {
            action(&tx);
        }

        let event = rx.try_recv().expect("should receive event");
        match event {
            AppEvent::SetConfigScriptTimeout(value) => {
                assert_eq!(value, codex_acp::config::ScriptTimeout::from_str("2m"));
            }
            _ => panic!("expected SetConfigScriptTimeout event, got: {event:?}"),
        }
    }

    #[test]
    fn config_picker_includes_loop_count_item() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx);

        let loop_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Loop Count"));
        assert!(
            loop_item.is_some(),
            "config picker should include a Loop Count item"
        );
    }

    #[test]
    fn config_picker_loop_count_shows_disabled_when_none() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx);

        let loop_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Loop Count"))
            .expect("should have loop count item");
        assert!(
            loop_item.name.contains("Disabled"),
            "Loop count should show 'Disabled' when None, got: {}",
            loop_item.name
        );
    }

    #[test]
    fn config_picker_loop_count_shows_value_when_set() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut config = make_test_config(false);
        config.loop_count = Some(5);

        let params = config_picker_params(&config, tx);

        let loop_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Loop Count"))
            .expect("should have loop count item");
        assert!(
            loop_item.name.contains("5"),
            "Loop count should show '5' when set to Some(5), got: {}",
            loop_item.name
        );
    }

    #[test]
    fn config_picker_loop_count_action_sends_open_picker_event() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx.clone());

        let loop_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Loop Count"))
            .expect("should have loop count item");

        for action in &loop_item.actions {
            action(&tx);
        }

        let event = rx.try_recv().expect("should receive event");
        assert!(
            matches!(event, AppEvent::OpenLoopCountPicker),
            "expected OpenLoopCountPicker event, got: {event:?}"
        );
    }

    #[test]
    fn config_picker_auto_worktree_toggleable_when_skillset_per_session() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut config = make_test_config(false);
        config.skillset_per_session = true;
        config.auto_worktree = true;

        let params = config_picker_params(&config, tx.clone());

        let auto_worktree_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Auto Worktree"))
            .expect("should have Auto Worktree item");
        // Auto Worktree should be a normal toggle, not locked
        assert!(
            auto_worktree_item.name.contains("(on)"),
            "Auto Worktree should show '(on)' as a normal toggle, got: {}",
            auto_worktree_item.name
        );
        assert!(
            !auto_worktree_item.name.contains("required"),
            "Auto Worktree should NOT show 'required', got: {}",
            auto_worktree_item.name
        );

        // Clicking it should send a toggle event, not be a no-op
        for action in &auto_worktree_item.actions {
            action(&tx);
        }
        let event = rx.try_recv().expect("should receive toggle event");
        assert!(
            matches!(event, AppEvent::SetConfigAutoWorktree(false)),
            "expected SetConfigAutoWorktree(false), got: {event:?}"
        );
    }

    #[test]
    fn config_picker_enabling_skillset_per_session_opens_worktree_choice() {
        if !super::skillset_picker::is_nori_skillsets_available() {
            // Skip: nori-skillsets not installed on this machine (e.g. CI).
            return;
        }

        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx.clone());

        let per_session_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Per Session Skillsets"))
            .expect("should have Per Session Skillsets item");

        // When skillset_per_session is off, clicking should open the worktree choice modal
        for action in &per_session_item.actions {
            action(&tx);
        }

        let event = rx.try_recv().expect("should receive event");
        assert!(
            matches!(event, AppEvent::OpenSkillsetPerSessionWorktreeChoice),
            "expected OpenSkillsetPerSessionWorktreeChoice, got: {event:?}"
        );
    }

    #[test]
    fn config_picker_disabling_skillset_per_session_sends_direct_event() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut config = make_test_config(false);
        config.skillset_per_session = true;

        let params = config_picker_params(&config, tx.clone());

        let per_session_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Per Session Skillsets"))
            .expect("should have Per Session Skillsets item");

        // When skillset_per_session is on, clicking should directly toggle off
        for action in &per_session_item.actions {
            action(&tx);
        }

        let event = rx.try_recv().expect("should receive event");
        assert!(
            matches!(event, AppEvent::SetConfigSkillsetPerSession(false)),
            "expected SetConfigSkillsetPerSession(false), got: {event:?}"
        );
    }

    #[test]
    fn skillset_worktree_choice_with_worktrees_sends_both_events() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params = skillset_worktree_choice_params(tx.clone());

        // Select "With Auto Worktrees"
        for action in &params.items[0].actions {
            action(&tx);
        }

        let event1 = rx.try_recv().expect("should receive first event");
        let event2 = rx.try_recv().expect("should receive second event");

        assert!(
            matches!(event1, AppEvent::SetConfigSkillsetPerSession(true)),
            "expected SetConfigSkillsetPerSession(true), got: {event1:?}"
        );
        assert!(
            matches!(event2, AppEvent::SetConfigAutoWorktree(true)),
            "expected SetConfigAutoWorktree(true), got: {event2:?}"
        );
    }

    #[test]
    fn skillset_worktree_choice_without_worktrees_sends_only_skillset_event() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params = skillset_worktree_choice_params(tx.clone());

        // Select "Without Auto Worktrees"
        for action in &params.items[1].actions {
            action(&tx);
        }

        let event = rx.try_recv().expect("should receive event");
        assert!(
            matches!(event, AppEvent::SetConfigSkillsetPerSession(true)),
            "expected SetConfigSkillsetPerSession(true), got: {event:?}"
        );

        // No second event should be sent
        assert!(
            rx.try_recv().is_err(),
            "should NOT receive a second event for auto_worktree"
        );
    }

    #[test]
    fn config_picker_per_session_description_does_not_say_requires() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx);

        let per_session_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Per Session Skillsets"))
            .expect("should have Per Session Skillsets item");

        if let Some(desc) = &per_session_item.description {
            assert!(
                !desc.contains("requires Auto Worktree"),
                "description should not say 'requires Auto Worktree', got: {desc}"
            );
        }
    }
}
