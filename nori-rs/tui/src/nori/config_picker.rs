//! Config picker component for Nori TUI settings.
//!
//! This module provides the UI for modifying TUI configuration settings
//! that are persisted to ~/.nori/cli/config.toml.

use nori_config::AutoWorktree;
use nori_config::BrowserProfileMode;
use nori_config::FooterSegment;
use nori_config::FooterSegmentConfig;
use nori_config::NoriConfig;
use nori_config::NotifyAfterIdle;
use nori_config::OsNotifications;
use nori_config::ScriptTimeout;
use nori_config::TerminalNotifications;
use nori_config::VimEnterBehavior;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;
use crate::nori::skillset_picker;

/// Identifies a row in the `/settings` panel so the panel can be reopened with
/// the cursor on the setting that was just changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsItem {
    PinnedPlanDrawer,
    ResizeReflow,
    CustomWorkingMessages,
    VerticalFooter,
    TerminalNotifications,
    OsNotifications,
    VimMode,
    AutoWorktree,
    PerSessionSkillsets,
    NotifyAfterIdle,
    Hotkeys,
    ScriptTimeout,
    LoopCount,
    FooterSegments,
    FileManager,
}

/// Create selection view parameters for the config picker.
///
/// # Arguments
/// * `config` - The current Nori configuration
/// * `app_event_tx` - The app event sender for triggering config change events
/// * `focus` - Optional setting whose row should be selected when the panel opens
pub fn config_picker_params(
    config: &NoriConfig,
    app_event_tx: AppEventSender,
    focus: Option<SettingsItem>,
) -> SelectionViewParams {
    let vertical_footer_enabled = config.vertical_footer;
    let terminal_notifications_enabled =
        config.terminal_notifications == TerminalNotifications::Enabled;
    let os_notifications_enabled = config.os_notifications == OsNotifications::Enabled;
    let pinned_plan_drawer_enabled = config.pinned_plan_drawer;
    let resize_reflow_enabled = config.resize_reflow;
    let custom_working_messages_enabled = config.custom_working_messages;
    let custom_working_messages_description = if config.custom_working_message_list.is_empty() {
        "Rotate playful status messages while the agent is working".to_string()
    } else {
        format!(
            "Rotate playful status messages while the agent is working (custom list active: {} entries from config.toml)",
            config.custom_working_message_list.len()
        )
    };

    let entries: Vec<(SettingsItem, SelectionItem)> = vec![
        (
            SettingsItem::PinnedPlanDrawer,
            build_toggle_item(
                "Pinned Plan Drawer",
                "Pin plan updates to a drawer in the viewport instead of history",
                pinned_plan_drawer_enabled,
                {
                    let tx = app_event_tx.clone();
                    let new_value = !pinned_plan_drawer_enabled;
                    move || {
                        tx.send(AppEvent::SetConfigPinnedPlanDrawer(new_value));
                    }
                },
            ),
        ),
        (
            SettingsItem::ResizeReflow,
            build_toggle_item(
                "Resize Reflow",
                "Reflow transcript history when the terminal width changes",
                resize_reflow_enabled,
                {
                    let tx = app_event_tx.clone();
                    let new_value = !resize_reflow_enabled;
                    move || {
                        tx.send(AppEvent::SetConfigResizeReflow(new_value));
                    }
                },
            ),
        ),
        (
            SettingsItem::CustomWorkingMessages,
            build_toggle_item(
                "Custom Working Messages",
                &custom_working_messages_description,
                custom_working_messages_enabled,
                {
                    let tx = app_event_tx.clone();
                    let new_value = !custom_working_messages_enabled;
                    move || {
                        tx.send(AppEvent::SetConfigCustomWorkingMessages(new_value));
                    }
                },
            ),
        ),
        (
            SettingsItem::VerticalFooter,
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
        ),
        (
            SettingsItem::TerminalNotifications,
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
        ),
        (
            SettingsItem::OsNotifications,
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
        ),
        (SettingsItem::VimMode, {
            let current_mode = config.vim_mode;
            let display_name = format!("Vim Mode ({})", current_mode.display_name().to_lowercase());
            let actions: Vec<SelectionAction> = vec![Box::new({
                move |tx| {
                    tx.send(AppEvent::OpenVimModePicker);
                }
            })];
            SelectionItem {
                name: display_name,
                description: Some(
                    "Enable vim-style navigation in the textarea (Escape enters normal mode)"
                        .to_string(),
                ),
                is_current: current_mode.is_enabled(),
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        }),
        (SettingsItem::AutoWorktree, {
            let current_mode = config.auto_worktree;
            let display_name = format!(
                "Auto Worktree ({})",
                current_mode.display_name().to_lowercase()
            );
            let actions: Vec<SelectionAction> = vec![Box::new({
                move |tx| {
                    tx.send(AppEvent::OpenAutoWorktreePicker);
                }
            })];
            SelectionItem {
                name: display_name,
                description: Some("Create a git worktree at session start".to_string()),
                is_current: false,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        }),
        (SettingsItem::PerSessionSkillsets, {
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
        }),
        (SettingsItem::NotifyAfterIdle, {
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
        }),
        (SettingsItem::Hotkeys, {
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
        }),
        (SettingsItem::ScriptTimeout, {
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
        }),
        (SettingsItem::LoopCount, {
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
        }),
        (SettingsItem::FooterSegments, {
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
        }),
        (SettingsItem::FileManager, {
            let current_fm = config.file_manager;
            let display_name = match current_fm {
                Some(fm) => format!("File Manager ({})", fm.display_name()),
                None => "File Manager (not set)".to_string(),
            };
            let actions: Vec<SelectionAction> = vec![Box::new({
                move |tx| {
                    tx.send(AppEvent::OpenFileManagerPicker);
                }
            })];
            SelectionItem {
                name: display_name,
                description: Some("Terminal file manager for the /browse command".to_string()),
                is_current: false,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        }),
    ];

    let focus_idx = focus.and_then(|focus| entries.iter().position(|(id, _)| *id == focus));
    let items: Vec<SelectionItem> = entries
        .into_iter()
        .map(|(_, mut item)| {
            item.search_value = Some(match &item.description {
                Some(description) => format!("{} {description}", item.name),
                None => item.name.clone(),
            });
            item
        })
        .collect();

    SelectionViewParams {
        title: Some("Configuration".to_string()),
        subtitle: Some("Toggle TUI settings (changes saved to config.toml)".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        is_searchable: true,
        search_placeholder: Some("Search settings".to_string()),
        initial_selected_idx: Some(focus_idx.unwrap_or(0)),
        ..Default::default()
    }
    .picker()
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
                tx_with.send(AppEvent::SetConfigAutoWorktree(AutoWorktree::Automatic));
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
        title: Some("Per-session skillsets".to_string()),
        subtitle: Some("Choose how skillsets are managed per session".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        ..Default::default()
    }
    .picker()
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

/// Create selection view parameters for the vim mode sub-picker.
///
/// # Arguments
/// * `current` - The currently selected VimEnterBehavior variant
/// * `_app_event_tx` - The app event sender for triggering config change events
pub fn vim_mode_picker_params(
    current: VimEnterBehavior,
    _app_event_tx: AppEventSender,
    from_settings: bool,
) -> SelectionViewParams {
    let items: Vec<SelectionItem> = VimEnterBehavior::all_variants()
        .iter()
        .map(|&variant| {
            let is_current = variant == current;
            let description = match variant {
                VimEnterBehavior::Newline => Some(
                    "Enter inserts a newline in INSERT mode, submits in NORMAL mode".to_string(),
                ),
                VimEnterBehavior::Submit => Some(
                    "Enter submits in INSERT mode, inserts a newline in NORMAL mode".to_string(),
                ),
                VimEnterBehavior::AlwaysSubmit => {
                    Some("Enter submits in both INSERT and NORMAL modes".to_string())
                }
                VimEnterBehavior::Off => Some("Disable vim mode".to_string()),
            };
            let actions: Vec<SelectionAction> = vec![Box::new({
                move |tx| {
                    tx.send(AppEvent::SetConfigVimMode {
                        value: variant,
                        from_settings,
                    });
                }
            })];
            SelectionItem {
                name: variant.display_name().to_string(),
                description,
                is_current,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        })
        .collect();

    SelectionViewParams {
        title: Some("Vim mode".to_string()),
        subtitle: Some("Choose Enter key behavior for vim mode".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        ..Default::default()
    }
    .picker()
}

/// Create selection view parameters for the auto-worktree sub-picker.
///
/// # Arguments
/// * `current` - The currently selected AutoWorktree variant
/// * `app_event_tx` - The app event sender for triggering config change events
pub fn auto_worktree_picker_params(
    current: AutoWorktree,
    _app_event_tx: AppEventSender,
) -> SelectionViewParams {
    let items: Vec<SelectionItem> = AutoWorktree::all_variants()
        .iter()
        .map(|&variant| {
            let is_current = variant == current;
            let description = match variant {
                AutoWorktree::Automatic => {
                    Some("Always create a worktree at session start".to_string())
                }
                AutoWorktree::Ask => Some("Prompt before creating a worktree".to_string()),
                AutoWorktree::Off => Some("Never create a worktree automatically".to_string()),
            };
            let actions: Vec<SelectionAction> = vec![Box::new({
                move |tx| {
                    tx.send(AppEvent::SetConfigAutoWorktree(variant));
                }
            })];
            SelectionItem {
                name: variant.display_name().to_string(),
                description,
                is_current,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        })
        .collect();

    SelectionViewParams {
        title: Some("Auto worktree".to_string()),
        subtitle: Some("Create a git worktree at session start".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        ..Default::default()
    }
    .picker()
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
        title: Some("Notify after idle".to_string()),
        subtitle: Some("Select idle notification delay".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        ..Default::default()
    }
    .picker()
}

/// Create selection view parameters for the `/browser` profile picker.
///
/// Pre-highlights the saved default (`current`); selecting a tier emits
/// [`AppEvent::SetBrowserProfile`], which persists it as the new default and
/// launches the browser with it.
pub fn browser_profile_picker_params(
    current: BrowserProfileMode,
    _app_event_tx: AppEventSender,
) -> SelectionViewParams {
    let items: Vec<SelectionItem> = BrowserProfileMode::all_variants()
        .iter()
        .map(|&variant| {
            let is_current = variant == current;
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(AppEvent::SetBrowserProfile(variant));
            })];
            SelectionItem {
                name: variant.display_name().to_string(),
                description: Some(variant.description().to_string()),
                is_current,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        })
        .collect();

    SelectionViewParams {
        title: Some("Browser profile".to_string()),
        subtitle: Some("Choose which Chrome profile /browser launches".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        ..Default::default()
    }
    .picker()
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
        title: Some("Script timeout".to_string()),
        subtitle: Some("Select script execution timeout".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        ..Default::default()
    }
    .picker()
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
        title: Some("Footer segments".to_string()),
        subtitle: Some("Toggle which segments appear in the footer".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        initial_selected_idx: Some(0),
        ..Default::default()
    }
    .picker()
}

/// Create selection view parameters for the file manager sub-picker.
///
/// # Arguments
/// * `current` - The currently selected file manager, if any
/// * `_app_event_tx` - The app event sender for triggering config change events
pub fn file_manager_picker_params(
    current: Option<nori_config::FileManager>,
    _app_event_tx: AppEventSender,
) -> SelectionViewParams {
    use nori_config::FileManager;

    let variants = [
        FileManager::Vifm,
        FileManager::Ranger,
        FileManager::Lf,
        FileManager::Nnn,
    ];
    let items: Vec<SelectionItem> = variants
        .iter()
        .map(|&variant| {
            let is_current = current == Some(variant);
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(AppEvent::SetConfigFileManager(variant));
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
        title: Some("File manager".to_string()),
        subtitle: Some("Choose a terminal file manager for /browse".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        initial_selected_idx: Some(0),
        ..Default::default()
    }
    .picker()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use std::path::PathBuf;
    use tokio::sync::mpsc::unbounded_channel;

    fn make_test_config(vertical_footer: bool) -> NoriConfig {
        NoriConfig {
            vertical_footer,
            nori_home: PathBuf::from("/tmp/test-nori"),
            cwd: PathBuf::from("/tmp"),
            ..NoriConfig::default()
        }
    }

    #[test]
    fn browser_profile_picker_lists_all_tiers_and_marks_current() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params = browser_profile_picker_params(BrowserProfileMode::Persistent, tx);

        let names: Vec<&str> = params.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Throwaway",
                "Persistent nori profile",
                "Real Chrome profile"
            ]
        );
        // The saved default is pre-highlighted, and every row has a description.
        assert!(params.items.iter().all(|i| i.description.is_some()));
        assert_eq!(
            params.items.iter().position(|i| i.is_current),
            Some(1),
            "the current tier (Persistent) should be marked"
        );
    }

    #[test]
    fn browser_profile_selection_emits_set_event() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params = browser_profile_picker_params(BrowserProfileMode::Throwaway, tx.clone());

        // Select "Real Chrome profile" (index 2).
        let system_item = &params.items[2];
        for action in &system_item.actions {
            action(&tx);
        }

        match rx.try_recv().expect("should receive event") {
            AppEvent::SetBrowserProfile(value) => {
                assert_eq!(value, BrowserProfileMode::System);
            }
            other => panic!("expected SetBrowserProfile event, got: {other:?}"),
        }
    }

    #[test]
    fn config_picker_shows_current_state_on() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(true);

        let params = config_picker_params(&config, tx, None);

        let item = params
            .items
            .iter()
            .find(|item| item.name.contains("Vertical Footer"))
            .expect("config picker should include Vertical Footer");
        assert!(item.name.contains("(on)"));
    }

    #[test]
    fn config_picker_shows_current_state_off() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx, None);

        let item = params
            .items
            .iter()
            .find(|item| item.name.contains("Vertical Footer"))
            .expect("config picker should include Vertical Footer");
        assert!(item.name.contains("(off)"));
    }

    #[test]
    fn config_picker_custom_working_messages_action_sends_correct_event() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx.clone(), None);

        let item = params
            .items
            .iter()
            .find(|item| item.name.contains("Custom Working Messages"))
            .expect("config picker should include Custom Working Messages");
        assert!(item.name.contains("(on)"));
        for action in &item.actions {
            action(&tx);
        }

        let event = rx.try_recv().expect("should receive event");
        match event {
            AppEvent::SetConfigCustomWorkingMessages(value) => {
                assert!(!value, "enabled setting should toggle off");
            }
            _ => panic!("expected SetConfigCustomWorkingMessages event, got: {event:?}"),
        }
    }

    #[test]
    fn config_picker_resize_reflow_action_sends_correct_event() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx.clone(), None);
        let item = params
            .items
            .iter()
            .find(|item| item.name.contains("Resize Reflow"))
            .expect("config picker should include Resize Reflow");
        assert!(item.name.contains("(on)"));
        for action in &item.actions {
            action(&tx);
        }

        match rx.try_recv().expect("should receive event") {
            AppEvent::SetConfigResizeReflow(enabled) => assert!(!enabled),
            event => panic!("expected SetConfigResizeReflow event, got: {event:?}"),
        }
    }

    #[test]
    fn config_picker_resize_reflow_snapshot() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx, None);
        let item = params
            .items
            .iter()
            .find(|item| item.name.contains("Resize Reflow"))
            .expect("config picker should include Resize Reflow");

        insta::assert_snapshot!(format!(
            "{}\n{}",
            item.name,
            item.description.as_deref().unwrap_or_default()
        ));
    }

    #[test]
    fn config_picker_custom_working_messages_description_announces_user_list() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut config = make_test_config(false);
        config.custom_working_message_list = vec!["alpha".to_string(), "beta".to_string()];

        let params = config_picker_params(&config, tx, None);

        let item = params
            .items
            .iter()
            .find(|item| item.name.contains("Custom Working Messages"))
            .expect("config picker should include Custom Working Messages");
        let description = item
            .description
            .as_ref()
            .expect("custom working messages item should have a description");
        assert!(
            description.contains("custom list"),
            "description should mention the custom list when one is configured, got: {description:?}"
        );
    }

    #[test]
    fn config_picker_notify_after_idle_shows_current_value() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx, None);

        let idle_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Notify After Idle"))
            .expect("config picker should include Notify After Idle");
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

        let params = config_picker_params(&config, tx.clone(), None);

        let idle_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Notify After Idle"))
            .expect("config picker should include Notify After Idle");
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

        let params = config_picker_params(&config, tx.clone(), None);

        let vertical_footer_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Vertical Footer"))
            .expect("config picker should include Vertical Footer");
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

        let params = config_picker_params(&config, tx.clone(), None);

        let hotkeys_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Hotkeys"))
            .expect("config picker should include Hotkeys");
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

        let params = notify_after_idle_picker_params(nori_config::NotifyAfterIdle::FiveSeconds, tx);

        assert_eq!(params.items.len(), 5);
        assert!(params.title.unwrap().contains("Notify after idle"));
    }

    #[test]
    fn notify_after_idle_picker_marks_current_variant() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params =
            notify_after_idle_picker_params(nori_config::NotifyAfterIdle::ThirtySeconds, tx);

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

        let params =
            notify_after_idle_picker_params(nori_config::NotifyAfterIdle::FiveSeconds, tx.clone());

        // Select the "1 minute" option (index 3)
        let minute_item = &params.items[3];
        assert!(minute_item.name.contains("1 minute"));
        for action in &minute_item.actions {
            action(&tx);
        }

        let event = rx.try_recv().expect("should receive event");
        match event {
            AppEvent::SetConfigNotifyAfterIdle(value) => {
                assert_eq!(value, nori_config::NotifyAfterIdle::SixtySeconds);
            }
            _ => panic!("expected SetConfigNotifyAfterIdle event, got: {event:?}"),
        }
    }

    #[test]
    fn config_picker_includes_vim_mode_toggle() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx, None);

        assert_eq!(params.items.len(), 15);
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
        config.vim_mode = VimEnterBehavior::Submit;

        let params = config_picker_params(&config, tx, None);

        let vim_mode_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Vim Mode"))
            .expect("should have vim mode item");
        assert!(
            vim_mode_item.name.contains("submit in insert"),
            "vim mode should show current behavior when enabled, got: {}",
            vim_mode_item.name
        );
    }

    #[test]
    fn config_picker_vim_mode_action_opens_picker() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx.clone(), None);

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
        assert!(
            matches!(event, AppEvent::OpenVimModePicker),
            "vim mode action should open picker, got: {event:?}"
        );
    }

    #[test]
    fn vim_mode_picker_explains_all_enter_behaviors() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let always_submit = *VimEnterBehavior::all_variants()
            .iter()
            .find(|behavior| behavior.toml_value() == "always_submit")
            .expect("always-submit Vim behavior should be available");
        let params = vim_mode_picker_params(always_submit, AppEventSender::new(tx_raw), false);

        let choices: Vec<_> = params
            .items
            .iter()
            .map(|item| {
                (
                    item.name.as_str(),
                    item.description.as_deref(),
                    item.is_current,
                )
            })
            .collect();
        assert_eq!(
            choices,
            vec![
                (
                    "Submit in NORMAL",
                    Some("Enter inserts a newline in INSERT mode, submits in NORMAL mode"),
                    false,
                ),
                (
                    "Submit in INSERT",
                    Some("Enter submits in INSERT mode, inserts a newline in NORMAL mode"),
                    false,
                ),
                (
                    "Always Submit",
                    Some("Enter submits in both INSERT and NORMAL modes"),
                    true,
                ),
                ("Off", Some("Disable vim mode"), false),
            ]
        );
    }

    #[test]
    fn vim_mode_selection_carries_settings_origin() {
        for from_settings in [true, false] {
            let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
            let tx = AppEventSender::new(tx_raw);
            let params = vim_mode_picker_params(VimEnterBehavior::Off, tx.clone(), from_settings);

            let item = params
                .items
                .iter()
                .find(|item| !item.is_current)
                .expect("a non-current vim option should be selectable");
            for action in &item.actions {
                action(&tx);
            }

            let event = rx.try_recv().expect("selecting a vim mode emits an event");
            assert!(
                matches!(
                    event,
                    AppEvent::SetConfigVimMode { from_settings: origin, .. } if origin == from_settings
                ),
                "a vim selection should report whether it came from the settings panel"
            );
        }
    }

    #[test]
    fn config_picker_focuses_the_requested_setting() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx, Some(SettingsItem::ScriptTimeout));

        let focused = params
            .initial_selected_idx
            .expect("the panel should have a selected row");
        assert!(
            params.items[focused].name.starts_with("Script Timeout"),
            "reopening the settings panel should land the cursor on the edited setting, got {:?}",
            params.items[focused].name
        );
    }

    #[test]
    fn config_picker_defaults_focus_to_first_row() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx, None);

        assert_eq!(params.initial_selected_idx, Some(0));
    }

    #[test]
    fn config_picker_script_timeout_shows_current_value() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx, None);

        let timeout_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Script Timeout"))
            .expect("config picker should include Script Timeout");
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

        let params = config_picker_params(&config, tx.clone(), None);

        let timeout_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Script Timeout"))
            .expect("config picker should include Script Timeout");
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

        let params = script_timeout_picker_params(nori_config::ScriptTimeout::default(), tx);

        assert_eq!(params.items.len(), 5);
        assert!(params.title.unwrap().contains("Script timeout"));
    }

    #[test]
    fn script_timeout_picker_marks_current_value() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params = script_timeout_picker_params(nori_config::ScriptTimeout::from_str("1m"), tx);

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
            script_timeout_picker_params(nori_config::ScriptTimeout::default(), tx.clone());

        // Select the "2m" option (index 3)
        let two_min_item = &params.items[3];
        assert_eq!(two_min_item.name, "2m");
        for action in &two_min_item.actions {
            action(&tx);
        }

        let event = rx.try_recv().expect("should receive event");
        match event {
            AppEvent::SetConfigScriptTimeout(value) => {
                assert_eq!(value, nori_config::ScriptTimeout::from_str("2m"));
            }
            _ => panic!("expected SetConfigScriptTimeout event, got: {event:?}"),
        }
    }

    #[test]
    fn config_picker_includes_loop_count_item() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = make_test_config(false);

        let params = config_picker_params(&config, tx, None);

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

        let params = config_picker_params(&config, tx, None);

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

        let params = config_picker_params(&config, tx, None);

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

        let params = config_picker_params(&config, tx.clone(), None);

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
    fn config_picker_auto_worktree_shows_current_mode_and_opens_picker() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut config = make_test_config(false);
        config.auto_worktree = nori_config::AutoWorktree::Automatic;

        let params = config_picker_params(&config, tx.clone(), None);

        let auto_worktree_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Auto Worktree"))
            .expect("should have Auto Worktree item");
        // Should show the current mode in the display name
        assert!(
            auto_worktree_item.name.contains("(automatic)"),
            "Auto Worktree should show '(automatic)', got: {}",
            auto_worktree_item.name
        );

        // Clicking should open the sub-picker
        for action in &auto_worktree_item.actions {
            action(&tx);
        }
        let event = rx.try_recv().expect("should receive event");
        assert!(
            matches!(event, AppEvent::OpenAutoWorktreePicker),
            "expected OpenAutoWorktreePicker event, got: {event:?}"
        );
    }

    #[test]
    fn auto_worktree_picker_lists_all_variants_and_sends_correct_events() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params = auto_worktree_picker_params(nori_config::AutoWorktree::Off, tx.clone());

        // Should have 3 items: Automatic, Ask, Off
        assert_eq!(params.items.len(), 3, "should have 3 auto worktree options");

        // Off should be marked as current
        let off_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Off"))
            .expect("should have Off item");
        assert!(off_item.is_current, "Off should be marked as current");

        // Select "Automatic" - should send correct event
        let auto_item = params
            .items
            .iter()
            .find(|item| item.name.contains("Automatic"))
            .expect("should have Automatic item");
        assert!(
            !auto_item.is_current,
            "Automatic should not be marked as current"
        );
        for action in &auto_item.actions {
            action(&tx);
        }
        let event = rx.try_recv().expect("should receive event");
        assert!(
            matches!(
                event,
                AppEvent::SetConfigAutoWorktree(nori_config::AutoWorktree::Automatic)
            ),
            "expected SetConfigAutoWorktree(Automatic), got: {event:?}"
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

        let params = config_picker_params(&config, tx.clone(), None);

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

        let params = config_picker_params(&config, tx.clone(), None);

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
            matches!(
                event2,
                AppEvent::SetConfigAutoWorktree(nori_config::AutoWorktree::Automatic)
            ),
            "expected SetConfigAutoWorktree(Automatic), got: {event2:?}"
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

        let params = config_picker_params(&config, tx, None);

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

    #[test]
    fn file_manager_picker_lists_all_variants_and_sends_correct_events() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params = file_manager_picker_params(Some(nori_config::FileManager::Vifm), tx.clone());

        // Should have 4 items: vifm, ranger, lf, nnn
        assert_eq!(params.items.len(), 4, "should have 4 file manager options");

        // Vifm should be marked as current
        let vifm_item = params
            .items
            .iter()
            .find(|item| item.name.contains("vifm"))
            .expect("should have vifm item");
        assert!(vifm_item.is_current, "vifm should be marked as current");

        // Select "ranger" - should send correct event
        let ranger_item = params
            .items
            .iter()
            .find(|item| item.name.contains("ranger"))
            .expect("should have ranger item");
        assert!(
            !ranger_item.is_current,
            "ranger should not be marked as current"
        );
        for action in &ranger_item.actions {
            action(&tx);
        }
        let event = rx.try_recv().expect("should receive event");
        assert!(
            matches!(
                event,
                AppEvent::SetConfigFileManager(nori_config::FileManager::Ranger)
            ),
            "expected SetConfigFileManager(Ranger), got: {event:?}"
        );
    }

    #[test]
    fn file_manager_picker_none_current_marks_nothing() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params = file_manager_picker_params(None, tx);

        for item in &params.items {
            assert!(
                !item.is_current,
                "no item should be marked as current when file_manager is None, but '{}' was",
                item.name
            );
        }
    }
}
