//! User-facing history rendering for ACP session config snapshots.

use std::collections::BTreeMap;

use nori_acp as acp;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

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

/// Initial-snapshot banner shown the first time the agent announces its
/// session config options. Lists every option's current value and surfaces
/// the `/config` affordance so the user knows how to change them.
pub(crate) fn new_agent_options_initial_history_cell(
    agent_display_name: &str,
    config_options: &[acp::SessionConfigOption],
) -> PlainHistoryCell {
    let agent_display_name = if agent_display_name.is_empty() {
        "Agent"
    } else {
        agent_display_name
    };
    let values: Vec<SessionConfigDisplayValue> =
        config_options.iter().filter_map(display_value).collect();

    let mut line = vec!["• ".dim(), format!("{agent_display_name} options: ").into()];
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            line.push(", ".into());
        }
        line.extend(option_assignment_spans(&value.name, &value.value));
    }
    line.push(" (/config to change)".dim());

    PlainHistoryCell::new(vec![Line::from(line)])
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
        line.extend(option_assignment_spans(&change.name, &change.value));
    }

    PlainHistoryCell::new(vec![Line::from(line)])
}

pub(crate) fn new_agent_option_set_history_cell(
    agent_display_name: &str,
    option_name: &str,
    value_name: &str,
    saved_as_default: bool,
) -> PlainHistoryCell {
    let agent_display_name = if agent_display_name.is_empty() {
        "Agent"
    } else {
        agent_display_name
    };
    let mut line = vec![
        "• ".dim(),
        format!("{agent_display_name} option set: ").into(),
    ];
    line.extend(option_assignment_spans(option_name, value_name));
    if saved_as_default {
        line.push(" (saved as default)".dim());
    }

    PlainHistoryCell::new(vec![Line::from(line)])
}

fn option_assignment_spans(name: &str, value: &str) -> Vec<Span<'static>> {
    vec![
        name.to_string().into(),
        "=".into(),
        value.to_string().cyan().bold(),
    ]
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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use ratatui::style::Color;
    use ratatui::style::Modifier;
    use ratatui::style::Style;

    use super::*;
    use crate::history_cell::HistoryCell;

    #[test]
    fn history_cells_highlight_values_not_names() {
        let cell = new_agent_options_initial_history_cell(
            "Claude Code",
            &[acp::SessionConfigOption::select(
                "mode",
                "Mode",
                "default",
                vec![acp::SessionConfigSelectOption::new("default", "Default")],
            )],
        );

        let lines = cell.display_lines(80);
        assert_value_highlighted(&lines[0].spans, "Mode", "Default");

        let cell = new_agent_options_history_cell(
            "Claude Code",
            &[SessionConfigDisplayValue {
                name: "Effort".to_string(),
                value: "High".to_string(),
            }],
        );
        let lines = cell.display_lines(80);
        assert_value_highlighted(&lines[0].spans, "Effort", "High");

        let cell = new_agent_option_set_history_cell("Claude Code", "Model", "Opus 4.6", false);
        let lines = cell.display_lines(80);
        assert_value_highlighted(&lines[0].spans, "Model", "Opus 4.6");

        insta::assert_snapshot!(spans_to_style_snapshot(&lines[0].spans));
    }

    #[test]
    fn option_set_cell_marks_saved_default() {
        let cell = new_agent_option_set_history_cell("Claude Code", "Model", "Opus 4.6", true);

        let lines = cell.display_lines(80);
        let text: String = lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(
            text,
            "• Claude Code option set: Model=Opus 4.6 (saved as default)"
        );

        insta::assert_snapshot!(spans_to_style_snapshot(&lines[0].spans));
    }

    fn assert_value_highlighted(
        spans: &[ratatui::text::Span<'static>],
        expected_name: &str,
        expected_value: &str,
    ) {
        let name = spans
            .iter()
            .find(|span| span.content.as_ref() == expected_name)
            .expect("option name should be its own span");
        let separator = spans
            .iter()
            .find(|span| span.content.as_ref() == "=")
            .expect("separator should be its own span");
        let value = spans
            .iter()
            .find(|span| span.content.as_ref() == expected_value)
            .expect("option value should be its own span");

        assert_eq!(name.style, Style::default());
        assert_eq!(separator.style, Style::default());
        assert_eq!(value.style.fg, Some(Color::Cyan));
        assert!(value.style.add_modifier.contains(Modifier::BOLD));
    }

    fn spans_to_style_snapshot(spans: &[ratatui::text::Span<'static>]) -> String {
        spans
            .iter()
            .map(|span| format!("{:?} {:?}", span.content, span.style))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
