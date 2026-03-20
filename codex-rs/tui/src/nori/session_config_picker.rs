//! ACP session-config picker helpers.
//!
//! Builds generic selection popups from ACP `configOptions`.

use ratatui::text::Line;

use codex_acp::SessionConfigKind;
use codex_acp::SessionConfigOption;
use codex_acp::SessionConfigSelectGroup;
use codex_acp::SessionConfigSelectOption;
use codex_acp::SessionConfigSelectOptions;

use crate::app_event::AppEvent;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;

pub fn acp_session_config_picker_params(
    config_options: &[SessionConfigOption],
) -> SelectionViewParams {
    let supported: Vec<SessionConfigOption> = config_options
        .iter()
        .filter(|option| matches!(option.kind, SessionConfigKind::Select(_)))
        .cloned()
        .collect();

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

pub fn acp_session_config_value_picker_params(option: &SessionConfigOption) -> SelectionViewParams {
    let SessionConfigKind::Select(select) = &option.kind else {
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
        SessionConfigSelectOptions::Ungrouped(options) => {
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
        SessionConfigSelectOptions::Grouped(groups) => {
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

    let total_values = count_select_values(&select.options);

    SelectionViewParams {
        title: Some(option.name.clone()),
        subtitle: Some(
            option
                .description
                .clone()
                .unwrap_or_else(|| "Select a value for this ACP session setting".to_string()),
        ),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        is_searchable: total_values >= 6,
        search_placeholder: Some("Filter values".to_string()),
        initial_selected_idx,
        ..Default::default()
    }
}

fn current_value_label(option: &SessionConfigOption) -> Option<String> {
    let SessionConfigKind::Select(select) = &option.kind else {
        return None;
    };

    match &select.options {
        SessionConfigSelectOptions::Ungrouped(options) => options
            .iter()
            .find(|value| value.value == select.current_value)
            .map(|value| value.name.clone()),
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .find(|value| value.value == select.current_value)
            .map(|value| value.name.clone()),
        _ => None,
    }
}

fn group_header_item(group: &SessionConfigSelectGroup) -> SelectionItem {
    SelectionItem {
        name: format!("[{}]", group.name),
        description: None,
        actions: vec![],
        dismiss_on_select: false,
        search_value: Some(group.name.clone()),
        ..Default::default()
    }
}

fn value_item(
    option: &SessionConfigOption,
    option_value: &SessionConfigSelectOption,
    group_name: Option<&str>,
    is_current: bool,
) -> SelectionItem {
    let config_id = option.id.to_string();
    let value_id = option_value.value.to_string();
    let option_name = option.name.clone();
    let value_name = option_value.name.clone();
    let group_name = group_name.map(ToOwned::to_owned);

    let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
        tx.send(AppEvent::SetAcpSessionConfigOption {
            config_id: config_id.clone(),
            value: value_id.clone(),
            option_name: option_name.clone(),
            value_name: value_name.clone(),
        });
    })];

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

fn count_select_values(options: &SessionConfigSelectOptions) -> usize {
    match options {
        SessionConfigSelectOptions::Ungrouped(options) => options.len(),
        SessionConfigSelectOptions::Grouped(groups) => {
            groups.iter().map(|group| group.options.len()).sum()
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_acp::SessionConfigOptionCategory;

    fn grouped_model_option() -> SessionConfigOption {
        SessionConfigOption::select(
            "model",
            "Model",
            "openai-gpt5",
            vec![
                SessionConfigSelectGroup::new(
                    "openai",
                    "OpenAI",
                    vec![
                        SessionConfigSelectOption::new("openai-gpt5", "GPT-5"),
                        SessionConfigSelectOption::new("openai-gpt5-mini", "GPT-5 Mini"),
                    ],
                ),
                SessionConfigSelectGroup::new(
                    "anthropic",
                    "Anthropic",
                    vec![SessionConfigSelectOption::new(
                        "anthropic-sonnet",
                        "Claude Sonnet",
                    )],
                ),
            ],
        )
        .category(SessionConfigOptionCategory::Model)
    }

    #[test]
    fn top_level_picker_shows_current_value() {
        let params = acp_session_config_picker_params(&[grouped_model_option()]);
        assert_eq!(params.items.len(), 1);
        assert!(params.items[0].name.contains("GPT-5"));
    }

    #[test]
    fn grouped_value_picker_includes_headers_and_current_selection() {
        let params = acp_session_config_value_picker_params(&grouped_model_option());

        assert!(params.items.iter().any(|item| item.name == "[OpenAI]"));
        assert!(params.items.iter().any(|item| item.name == "[Anthropic]"));
        assert!(
            params
                .items
                .iter()
                .any(|item| item.name == "GPT-5" && item.is_current)
        );
    }
}
