//! Nori-branded exit message component for the TUI.
//!
//! This module provides an exit message cell that is displayed when the user
//! quits the session, showing a goodbye message, session ID, and message count.

use crate::history_cell::card_inner_width;
use crate::history_cell::with_border;
use crate::history_cell::HistoryCell;
use ratatui::prelude::*;
use ratatui::style::Stylize;

/// Maximum inner width for the exit message card.
const EXIT_MESSAGE_MAX_INNER_WIDTH: usize = 60;

/// The Nori-branded exit message cell.
#[derive(Debug)]
pub(crate) struct ExitMessageCell {
    session_id: String,
    message_count: usize,
}

impl ExitMessageCell {
    pub(crate) fn new(session_id: String, message_count: usize) -> Self {
        Self {
            session_id,
            message_count,
        }
    }
}

impl HistoryCell for ExitMessageCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let Some(_inner_width) = card_inner_width(width, EXIT_MESSAGE_MAX_INNER_WIDTH) else {
            return Vec::new();
        };

        let mut lines: Vec<Line<'static>> = Vec::new();

        // Goodbye message
        lines.push(Line::from(vec![
            Span::from("Goodbye! ").green().bold(),
            Span::from("Thanks for using Nori.").dim(),
        ]));

        // Empty line
        lines.push(Line::from(""));

        // Session ID line
        lines.push(Line::from(vec![
            Span::from("session:  ").dim(),
            Span::from(self.session_id.clone()),
        ]));

        // Message count line
        let message_label = if self.message_count == 1 {
            "message"
        } else {
            "messages"
        };
        lines.push(Line::from(vec![
            Span::from("messages: ").dim(),
            Span::from(format!("{} {}", self.message_count, message_label)),
        ]));

        with_border(lines)
    }
}

/// Create a new exit message cell to be displayed when the session ends.
pub(crate) fn new_exit_message(session_id: String, message_count: usize) -> ExitMessageCell {
    ExitMessageCell::new(session_id, message_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_lines(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn exit_message_renders_correctly() {
        let cell = ExitMessageCell::new("abc123".to_string(), 42);
        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        // Should contain goodbye message
        assert!(
            rendered.contains("Goodbye!"),
            "Exit message should contain goodbye"
        );

        // Should contain session ID
        assert!(
            rendered.contains("session:"),
            "Exit message should show session label"
        );
        assert!(
            rendered.contains("abc123"),
            "Exit message should show session ID"
        );

        // Should contain message count
        assert!(
            rendered.contains("messages:"),
            "Exit message should show messages label"
        );
        assert!(
            rendered.contains("42 messages"),
            "Exit message should show message count"
        );
    }

    #[test]
    fn exit_message_singular_message() {
        let cell = ExitMessageCell::new("xyz789".to_string(), 1);
        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        // Should use singular "message" for count of 1
        assert!(
            rendered.contains("1 message"),
            "Exit message should use singular form for 1 message"
        );
        assert!(
            !rendered.contains("1 messages"),
            "Exit message should not use plural form for 1 message"
        );
    }

    #[test]
    fn exit_message_zero_messages() {
        let cell = ExitMessageCell::new("empty".to_string(), 0);
        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        // Should use plural "messages" for count of 0
        assert!(
            rendered.contains("0 messages"),
            "Exit message should use plural form for 0 messages"
        );
    }

    #[test]
    fn exit_message_snapshot() {
        let cell = ExitMessageCell::new("sess_abc123def456".to_string(), 15);
        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        insta::assert_snapshot!(rendered);
    }
}
