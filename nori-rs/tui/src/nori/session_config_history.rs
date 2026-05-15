//! User-facing history rendering for ACP session config snapshots.

use std::collections::BTreeMap;

use nori_acp as acp;
use ratatui::style::Stylize;
use ratatui::text::Line;

use crate::history_cell::PlainHistoryCell;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionConfigDisplayValue {
    pub(crate) name: String,
    pub(crate) value: String,
}

pub(crate) type SessionConfigSnapshot = BTreeMap<String, SessionConfigDisplayValue>;

pub(crate) fn snapshot_from_options(
    config_options: &[acp::SessionConfigOption],
) -> SessionConfigSnapshot {
    config_options
        .iter()
        .filter_map(|option| display_value(option).map(|display| (option.id.to_string(), display)))
        .collect()
}

pub(crate) fn changed_values(
    previous: &SessionConfigSnapshot,
    config_options: &[acp::SessionConfigOption],
) -> Vec<SessionConfigDisplayValue> {
    config_options
        .iter()
        .filter_map(|option| {
            let current = display_value(option)?;
            let previous = previous.get(&option.id.to_string())?;
            (previous.value != current.value).then_some(current)
        })
        .collect()
}

pub(crate) fn new_agent_options_history_cell(
    agent_display_name: &str,
    changes: &[SessionConfigDisplayValue],
) -> PlainHistoryCell {
    let agent_display_name = if agent_display_name.is_empty() {
        "Agent"
    } else {
        agent_display_name
    };
    let noun = if changes.len() == 1 {
        "option"
    } else {
        "options"
    };
    let mut line = vec![
        "• ".dim(),
        format!("{agent_display_name} {noun} updated: ").into(),
    ];

    for (index, change) in changes.iter().enumerate() {
        if index > 0 {
            line.push(", ".into());
        }
        line.push(format!("{}={}", change.name, change.value).cyan().bold());
    }

    PlainHistoryCell::new(vec![Line::from(line)])
}

pub(crate) fn new_agent_option_set_history_cell(
    agent_display_name: &str,
    option_name: &str,
    value_name: &str,
) -> PlainHistoryCell {
    let agent_display_name = if agent_display_name.is_empty() {
        "Agent"
    } else {
        agent_display_name
    };
    PlainHistoryCell::new(vec![Line::from(vec![
        "• ".dim(),
        format!("{agent_display_name} option set: ").into(),
        format!("{option_name}={value_name}").cyan().bold(),
    ])])
}

fn display_value(option: &acp::SessionConfigOption) -> Option<SessionConfigDisplayValue> {
    let acp::SessionConfigKind::Select(select) = &option.kind else {
        return None;
    };

    let value = match &select.options {
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
    .unwrap_or_else(|| select.current_value.to_string());

    Some(SessionConfigDisplayValue {
        name: option.name.clone(),
        value,
    })
}
