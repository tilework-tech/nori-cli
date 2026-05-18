//! History-cell rendering for agent-initiated mode changes.

use ratatui::style::Stylize;
use ratatui::text::Line;

use crate::history_cell::PlainHistoryCell;

pub(crate) fn new_agent_mode_changed_cell(
    agent_display_name: &str,
    mode_label: &str,
) -> PlainHistoryCell {
    let agent_display_name = if agent_display_name.is_empty() {
        "Agent"
    } else {
        agent_display_name
    };
    PlainHistoryCell::new(vec![Line::from(vec![
        "• ".dim(),
        format!("{agent_display_name} mode changed: ").into(),
        mode_label.to_string().cyan().bold(),
    ])])
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use ratatui::style::Color;
    use ratatui::style::Modifier;

    use crate::history_cell::HistoryCell;

    #[test]
    fn renders_dim_bullet_plain_prefix_and_cyan_bold_label() {
        let cell = new_agent_mode_changed_cell("ElizACP", "Don't Ask");
        let lines = cell.display_lines(0);

        assert_eq!(lines.len(), 1);
        let spans = &lines[0].spans;
        assert_eq!(spans.len(), 3);

        assert_eq!(spans[0].content, "• ");
        assert!(spans[0].style.add_modifier.contains(Modifier::DIM));

        assert_eq!(spans[1].content, "ElizACP mode changed: ");
        assert_eq!(spans[1].style.fg, None);
        assert_eq!(spans[1].style.add_modifier, Modifier::empty());

        assert_eq!(spans[2].content, "Don't Ask");
        assert_eq!(spans[2].style.fg, Some(Color::Cyan));
        assert!(spans[2].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn falls_back_to_agent_when_display_name_is_empty() {
        let cell = new_agent_mode_changed_cell("", "Review");
        let lines = cell.display_lines(0);
        assert_eq!(lines[0].spans[1].content, "Agent mode changed: ");
    }
}
