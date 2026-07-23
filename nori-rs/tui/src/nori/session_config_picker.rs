//! Generic picker helpers for ACP session config options.

use nori_protocol::acp::v1 as acp;
use ratatui::text::Line;

use crate::app_event::AppEvent;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;

pub(crate) fn acp_session_config_picker_params(
    config_options: &[acp::SessionConfigOption],
) -> SelectionViewParams {
    let supported = config_options
        .iter()
        .filter(|option| matches!(option.kind, acp::SessionConfigKind::Select(_)))
        .cloned()
        .collect::<Vec<_>>();

    if supported.is_empty() {
        return SelectionViewParams {
            title: Some("Session Config".to_string()),
            subtitle: Some("No ACP session settings available".to_string()),
            footer_hint: Some(Line::from("Press esc to dismiss.")),
            items: vec![SelectionItem {
                name: "No editable ACP session config options".to_string(),
                description: Some(
                    "This agent did not expose any supported select-style session settings."
                        .to_string(),
                ),
                dismiss_on_select: true,
                ..Default::default()
            }],
            ..Default::default()
        };
    }

    let items = supported
        .iter()
        .map(|option| {
            let current_value =
                current_value_label(option).unwrap_or_else(|| "unknown".to_string());
            let option_for_action = option.clone();
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(AppEvent::OpenAcpSessionConfigValuePicker {
                    option: option_for_action.clone(),
                });
            })];

            SelectionItem {
                name: format!("{} ({current_value})", option.name),
                description: option.description.clone(),
                actions,
                dismiss_on_select: true,
                search_value: Some(format!("{} {current_value}", option.name)),
                ..Default::default()
            }
        })
        .collect();

    SelectionViewParams {
        title: Some("Session Config".to_string()),
        subtitle: Some("Select an ACP session setting to change".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        initial_selected_idx: Some(0),
        ..Default::default()
    }
}

pub(crate) fn acp_session_config_value_picker_params(
    option: &acp::SessionConfigOption,
) -> SelectionViewParams {
    let acp::SessionConfigKind::Select(select) = &option.kind else {
        return SelectionViewParams {
            title: Some(option.name.clone()),
            subtitle: Some("Unsupported ACP config type".to_string()),
            footer_hint: Some(Line::from("Press esc to dismiss.")),
            items: vec![SelectionItem {
                name: "This ACP config type is not supported yet".to_string(),
                description: option.description.clone(),
                dismiss_on_select: true,
                ..Default::default()
            }],
            ..Default::default()
        };
    };

    let mut items = Vec::new();
    let mut initial_selected_idx = None;

    match &select.options {
        acp::SessionConfigSelectOptions::Ungrouped(options) => {
            for option_value in options {
                let idx = items.len();
                if option_value.value == select.current_value && initial_selected_idx.is_none() {
                    initial_selected_idx = Some(idx);
                }
                items.push(value_item(
                    option,
                    option_value,
                    None,
                    option_value.value == select.current_value,
                ));
            }
        }
        acp::SessionConfigSelectOptions::Grouped(groups) => {
            for group in groups {
                items.push(group_header_item(group));
                for option_value in &group.options {
                    let idx = items.len();
                    if option_value.value == select.current_value && initial_selected_idx.is_none()
                    {
                        initial_selected_idx = Some(idx);
                    }
                    items.push(value_item(
                        option,
                        option_value,
                        Some(&group.name),
                        option_value.value == select.current_value,
                    ));
                }
            }
        }
        _ => {}
    }

    if initial_selected_idx.is_none() {
        initial_selected_idx = items.iter().position(|item| !item.actions.is_empty());
    }

    SelectionViewParams {
        title: Some(option.name.clone()),
        subtitle: Some(
            option
                .description
                .clone()
                .unwrap_or_else(|| "Select a value for this ACP session setting".to_string()),
        ),
        footer_hint: Some(standard_popup_hint_line()),
        is_searchable: count_select_values(&select.options) >= 6,
        search_placeholder: Some("Filter values".to_string()),
        items,
        initial_selected_idx,
        ..Default::default()
    }
}

fn current_value_label(option: &acp::SessionConfigOption) -> Option<String> {
    let acp::SessionConfigKind::Select(select) = &option.kind else {
        return None;
    };

    match &select.options {
        acp::SessionConfigSelectOptions::Ungrouped(options) => options
            .iter()
            .find(|value| value.value == select.current_value)
            .map(|value| value.name.clone()),
        acp::SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .find(|value| value.value == select.current_value)
            .map(|value| value.name.clone()),
        _ => None,
    }
}

fn group_header_item(group: &acp::SessionConfigSelectGroup) -> SelectionItem {
    SelectionItem {
        name: format!("[{}]", group.name),
        dismiss_on_select: false,
        search_value: Some(group.name.clone()),
        ..Default::default()
    }
}

fn value_item(
    option: &acp::SessionConfigOption,
    option_value: &acp::SessionConfigSelectOption,
    group_name: Option<&str>,
    is_current: bool,
) -> SelectionItem {
    let config_id = option.id.to_string();
    let value = option_value.value.to_string();
    let option_name = option.name.clone();
    let value_name = option_value.name.clone();
    let group_name = group_name.map(str::to_string);

    let actions: Vec<SelectionAction> = if is_current {
        Vec::new()
    } else {
        vec![Box::new(move |tx| {
            tx.send(AppEvent::SetAcpSessionConfigOption {
                config_id: config_id.clone(),
                value: value.clone(),
                option_name: option_name.clone(),
                value_name: value_name.clone(),
            });
        })]
    };

    let description = match (&option_value.description, group_name) {
        (Some(description), Some(group_name)) => Some(format!("[{group_name}] {description}")),
        (Some(description), None) => Some(description.clone()),
        (None, Some(group_name)) => Some(format!("Group: {group_name}")),
        (None, None) => None,
    };

    SelectionItem {
        name: option_value.name.clone(),
        description,
        is_current,
        actions,
        dismiss_on_select: true,
        search_value: Some(format!(
            "{} {}",
            option_value.name,
            option_value.description.clone().unwrap_or_default()
        )),
        ..Default::default()
    }
}

fn count_select_values(options: &acp::SessionConfigSelectOptions) -> usize {
    match options {
        acp::SessionConfigSelectOptions::Ungrouped(options) => options.len(),
        acp::SessionConfigSelectOptions::Grouped(groups) => {
            groups.iter().map(|group| group.options.len()).sum()
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_option() -> acp::SessionConfigOption {
        acp::SessionConfigOption::select(
            "model",
            "Model",
            "mock-model-default",
            vec![
                acp::SessionConfigSelectOption::new("mock-model-default", "Mock Default Model"),
                acp::SessionConfigSelectOption::new("mock-model-fast", "Mock Fast Model"),
            ],
        )
        .category(acp::SessionConfigOptionCategory::Model)
    }

    fn grouped_mode_option() -> acp::SessionConfigOption {
        acp::SessionConfigOption::select(
            "mode",
            "Mode",
            "plan",
            vec![
                acp::SessionConfigSelectGroup::new(
                    "safe",
                    "Safe",
                    vec![acp::SessionConfigSelectOption::new("ask", "Ask")],
                ),
                acp::SessionConfigSelectGroup::new(
                    "active",
                    "Active",
                    vec![
                        acp::SessionConfigSelectOption::new("plan", "Plan"),
                        acp::SessionConfigSelectOption::new("build", "Build"),
                    ],
                ),
            ],
        )
        .category(acp::SessionConfigOptionCategory::Mode)
    }

    #[test]
    fn top_level_picker_shows_select_options_with_current_value() {
        let params = super::acp_session_config_picker_params(&[model_option()]);

        assert_eq!(params.title.as_deref(), Some("Session Config"));
        assert_eq!(params.items.len(), 1);
        assert_eq!(params.items[0].name, "Model (Mock Default Model)");
        assert_eq!(params.initial_selected_idx, Some(0));
    }

    #[test]
    fn value_picker_preserves_group_order_and_current_selection() {
        let params = super::acp_session_config_value_picker_params(&grouped_mode_option());
        let names = params
            .items
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["[Safe]", "Ask", "[Active]", "Plan", "Build"]);
        assert!(params.items[3].is_current);
        assert_eq!(params.initial_selected_idx, Some(3));
    }

    #[test]
    fn value_picker_current_value_has_no_set_action() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let app_event_tx = crate::app_event_sender::AppEventSender::new(tx);
        let mut current_view = crate::bottom_pane::ListSelectionView::new(
            super::acp_session_config_value_picker_params(&model_option()),
            app_event_tx.clone(),
        );

        crate::bottom_pane::BottomPaneView::handle_key_event(
            &mut current_view,
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ),
        );
        assert!(
            rx.try_recv().is_err(),
            "accepting the already-current value should not request a config change"
        );

        let mut alternate_view = crate::bottom_pane::ListSelectionView::new(
            super::acp_session_config_value_picker_params(&model_option()),
            app_event_tx,
        );
        crate::bottom_pane::BottomPaneView::handle_key_event(
            &mut alternate_view,
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Down,
                crossterm::event::KeyModifiers::NONE,
            ),
        );
        crate::bottom_pane::BottomPaneView::handle_key_event(
            &mut alternate_view,
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ),
        );

        let event = rx.try_recv().expect("alternate value emits config change");
        assert!(matches!(
            event,
            AppEvent::SetAcpSessionConfigOption {
                config_id,
                value,
                option_name,
                value_name,
            } if config_id == "model"
                && value == "mock-model-fast"
                && option_name == "Model"
                && value_name == "Mock Fast Model"
        ));
        assert!(rx.try_recv().is_err(), "expected a single config event");
    }

    #[test]
    fn empty_picker_explains_when_agent_exposes_no_supported_options() {
        let params = super::acp_session_config_picker_params(&[]);

        assert_eq!(params.title.as_deref(), Some("Session Config"));
        assert_eq!(params.items.len(), 1);
        assert!(params.items[0].actions.is_empty());
    }
}
