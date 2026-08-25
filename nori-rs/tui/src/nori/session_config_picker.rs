//! Generic picker helpers for ACP session config options.

use nori_harness::OtherModel;
use nori_protocol::acp::v1 as acp;
use ratatui::text::Line;

use crate::app_event::AppEvent;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;

pub(crate) fn acp_session_config_picker_params(
    config_options: &[acp::SessionConfigOption],
    focus_config_id: Option<&str>,
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

    let focus_idx = focus_config_id.and_then(|config_id| {
        supported
            .iter()
            .position(|option| option.id.to_string() == config_id)
    });

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
        initial_selected_idx: Some(focus_idx.unwrap_or(0)),
        ..Default::default()
    }
}

pub(crate) fn acp_session_config_value_picker_params(
    option: &acp::SessionConfigOption,
    other_models: &[OtherModel],
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

    let is_model = option.category == Some(acp::SessionConfigOptionCategory::Model);

    // "Recommended": the values the agent advertises, exactly as reported.
    let mut recommended: Vec<SelectionItem> = Vec::new();
    match &select.options {
        acp::SessionConfigSelectOptions::Ungrouped(options) => {
            for option_value in options {
                recommended.push(value_item(
                    option,
                    option_value,
                    None,
                    option_value.value == select.current_value,
                ));
            }
        }
        acp::SessionConfigSelectOptions::Grouped(groups) => {
            for group in groups {
                recommended.push(group_header_item(group));
                for option_value in &group.options {
                    recommended.push(value_item(
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

    // "Other": curated models the agent does not advertise, forced via spawn-time
    // injection. Deduplicated against advertised values so nothing appears twice.
    let mut other: Vec<SelectionItem> = Vec::new();
    if is_model {
        let advertised = advertised_value_strings(&select.options);
        let current_value = select.current_value.to_string();
        for model in other_models {
            if advertised.contains(model.id) {
                continue;
            }
            other.push(other_model_item(option, model, current_value == model.id));
        }
    }

    // Only show labeled sections when both sides have content; otherwise render a
    // flat list exactly as before.
    let mut items = Vec::new();
    if !recommended.is_empty() && !other.is_empty() {
        items.push(section_header_item("Recommended"));
        items.append(&mut recommended);
        items.push(section_header_item("Other"));
        items.append(&mut other);
    } else {
        items.append(&mut recommended);
        items.append(&mut other);
    }

    if is_model {
        let config_id = option.id.to_string();
        let option_name = option.name.clone();
        items.push(SelectionItem {
            name: "Use custom model...".to_string(),
            description: Some("Enter any model ID".to_string()),
            actions: vec![Box::new(move |tx| {
                tx.send(AppEvent::OpenCustomModelInput {
                    config_id: config_id.clone(),
                    option_name: option_name.clone(),
                });
            })],
            dismiss_on_select: true,
            search_value: Some("custom model".to_string()),
            ..Default::default()
        });
    }

    let initial_selected_idx = items.iter().position(|item| item.is_current).or_else(|| {
        items
            .iter()
            .position(|item| !item.is_header && !item.actions.is_empty())
    });

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

fn section_header_item(label: &str) -> SelectionItem {
    SelectionItem {
        name: label.to_string(),
        is_header: true,
        dismiss_on_select: false,
        ..Default::default()
    }
}

fn advertised_value_strings(
    options: &acp::SessionConfigSelectOptions,
) -> std::collections::HashSet<String> {
    match options {
        acp::SessionConfigSelectOptions::Ungrouped(values) => {
            values.iter().map(|value| value.value.to_string()).collect()
        }
        acp::SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .map(|value| value.value.to_string())
            .collect(),
        _ => std::collections::HashSet::new(),
    }
}

fn other_model_item(
    option: &acp::SessionConfigOption,
    model: &OtherModel,
    is_current: bool,
) -> SelectionItem {
    let actions: Vec<SelectionAction> = if is_current {
        Vec::new()
    } else {
        let config_id = option.id.to_string();
        let option_name = option.name.clone();
        let value = model.id.to_string();
        let value_name = model.label.to_string();
        vec![Box::new(move |tx| {
            tx.send(AppEvent::SetAcpSessionConfigOption {
                config_id: config_id.clone(),
                value: value.clone(),
                option_name: option_name.clone(),
                value_name: value_name.clone(),
                is_custom_model: true,
            });
        })]
    };

    SelectionItem {
        name: model.label.to_string(),
        description: Some(model.id.to_string()),
        is_current,
        actions,
        dismiss_on_select: true,
        search_value: Some(format!("{} {}", model.label, model.id)),
        ..Default::default()
    }
}

fn group_header_item(group: &acp::SessionConfigSelectGroup) -> SelectionItem {
    SelectionItem {
        name: group.name.clone(),
        is_header: true,
        dismiss_on_select: false,
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
                is_custom_model: false,
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
        let params = super::acp_session_config_picker_params(&[model_option()], None);

        assert_eq!(params.title.as_deref(), Some("Session Config"));
        assert_eq!(params.items.len(), 1);
        assert_eq!(params.items[0].name, "Model (Mock Default Model)");
        assert_eq!(params.initial_selected_idx, Some(0));
    }

    #[test]
    fn top_level_picker_focuses_the_requested_option() {
        let params = super::acp_session_config_picker_params(
            &[model_option(), grouped_mode_option()],
            Some("mode"),
        );

        assert_eq!(params.items.len(), 2);
        assert!(params.items[1].name.starts_with("Mode"));
        assert_eq!(
            params.initial_selected_idx,
            Some(1),
            "reopening after a change should land the cursor on the edited option"
        );
    }

    #[test]
    fn top_level_picker_defaults_focus_to_first_when_id_absent() {
        let params = super::acp_session_config_picker_params(
            &[model_option(), grouped_mode_option()],
            Some("nonexistent"),
        );

        assert_eq!(params.initial_selected_idx, Some(0));
    }

    #[test]
    fn value_picker_preserves_group_order_and_current_selection() {
        let params = super::acp_session_config_value_picker_params(&grouped_mode_option(), &[]);
        let names = params
            .items
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["Safe", "Ask", "Active", "Plan", "Build"]);
        assert!(params.items[0].is_header, "group labels render as headers");
        assert!(params.items[3].is_current);
        assert_eq!(params.initial_selected_idx, Some(3));
    }

    #[test]
    fn value_picker_current_value_has_no_set_action() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let app_event_tx = crate::app_event_sender::AppEventSender::new(tx);
        let mut current_view = crate::bottom_pane::ListSelectionView::new(
            super::acp_session_config_value_picker_params(&model_option(), &[]),
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
            super::acp_session_config_value_picker_params(&model_option(), &[]),
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
                is_custom_model,
            } if config_id == "model"
                && value == "mock-model-fast"
                && option_name == "Model"
                && value_name == "Mock Fast Model"
                && !is_custom_model
        ));
        assert!(rx.try_recv().is_err(), "expected a single config event");
    }

    #[test]
    fn empty_picker_explains_when_agent_exposes_no_supported_options() {
        let params = super::acp_session_config_picker_params(&[], None);

        assert_eq!(params.title.as_deref(), Some("Session Config"));
        assert_eq!(params.items.len(), 1);
        assert!(params.items[0].actions.is_empty());
    }

    #[test]
    fn model_picker_includes_custom_model_entry() {
        let params = super::acp_session_config_value_picker_params(&model_option(), &[]);
        let names: Vec<&str> = params.items.iter().map(|i| i.name.as_str()).collect();

        assert_eq!(
            names,
            vec![
                "Mock Default Model",
                "Mock Fast Model",
                "Use custom model..."
            ]
        );
        let custom_item = params.items.last().unwrap();
        assert!(
            !custom_item.actions.is_empty(),
            "custom entry should have an action"
        );
        assert!(!custom_item.is_current);
    }

    #[test]
    fn non_model_picker_does_not_include_custom_entry() {
        let params = super::acp_session_config_value_picker_params(&grouped_mode_option(), &[]);

        assert!(!params.items.iter().any(|i| i.name == "Use custom model..."));
    }

    fn model_option_with_current(current: &'static str) -> acp::SessionConfigOption {
        acp::SessionConfigOption::select(
            "model",
            "Model",
            current,
            vec![
                acp::SessionConfigSelectOption::new("mock-model-default", "Mock Default Model"),
                acp::SessionConfigSelectOption::new("mock-model-fast", "Mock Fast Model"),
            ],
        )
        .category(acp::SessionConfigOptionCategory::Model)
    }

    const OTHER_MODELS: &[OtherModel] = &[
        OtherModel {
            id: "legacy-a",
            label: "Legacy A",
        },
        OtherModel {
            id: "legacy-b",
            label: "Legacy B",
        },
    ];

    fn item_names(params: &SelectionViewParams) -> Vec<&str> {
        params.items.iter().map(|item| item.name.as_str()).collect()
    }

    #[test]
    fn model_picker_splits_recommended_and_other_sections() {
        let params = super::acp_session_config_value_picker_params(&model_option(), OTHER_MODELS);

        assert_eq!(
            item_names(&params),
            vec![
                "Recommended",
                "Mock Default Model",
                "Mock Fast Model",
                "Other",
                "Legacy A",
                "Legacy B",
                "Use custom model...",
            ]
        );

        let row = |name: &str| {
            params
                .items
                .iter()
                .find(|item| item.name == name)
                .unwrap_or_else(|| panic!("row {name} present"))
        };
        assert!(row("Recommended").is_header, "section labels are headers");
        assert!(row("Other").is_header, "section labels are headers");
        assert!(
            !row("Mock Default Model").is_header,
            "advertised models are selectable rows"
        );
        assert!(
            !row("Legacy A").is_header,
            "other models are selectable rows"
        );
    }

    #[test]
    fn model_picker_dedupes_other_against_advertised() {
        let others: &[OtherModel] = &[
            OtherModel {
                id: "mock-model-fast",
                label: "Should Not Appear",
            },
            OtherModel {
                id: "legacy-b",
                label: "Legacy B",
            },
        ];

        let params = super::acp_session_config_value_picker_params(&model_option(), others);

        assert_eq!(
            item_names(&params),
            vec![
                "Recommended",
                "Mock Default Model",
                "Mock Fast Model",
                "Other",
                "Legacy B",
                "Use custom model...",
            ],
            "an already-advertised id must not be duplicated in the Other section"
        );
    }

    #[test]
    fn selecting_other_model_requests_injected_config_change() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let app_event_tx = crate::app_event_sender::AppEventSender::new(tx);
        let params = super::acp_session_config_value_picker_params(&model_option(), OTHER_MODELS);

        let other = params
            .items
            .iter()
            .find(|item| item.name == "Legacy A")
            .expect("other model row present");
        for action in &other.actions {
            action(&app_event_tx);
        }

        let event = rx
            .try_recv()
            .expect("selecting an other model emits a config change");
        assert!(matches!(
            event,
            AppEvent::SetAcpSessionConfigOption {
                config_id,
                value,
                option_name,
                value_name,
                is_custom_model,
            } if config_id == "model"
                && value == "legacy-a"
                && option_name == "Model"
                && value_name == "Legacy A"
                && is_custom_model
        ));
        assert!(rx.try_recv().is_err(), "expected a single config event");
    }

    #[test]
    fn current_injected_other_model_is_marked_and_not_resettable() {
        let params = super::acp_session_config_value_picker_params(
            &model_option_with_current("legacy-a"),
            OTHER_MODELS,
        );

        let other = params
            .items
            .iter()
            .find(|item| item.name == "Legacy A")
            .expect("other model row present");
        assert!(
            other.is_current,
            "the active injected model should be current"
        );
        assert!(
            other.actions.is_empty(),
            "the already-current model should not be re-settable"
        );
    }
}
