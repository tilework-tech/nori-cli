//! Nori-branded exit message component for the TUI.
//!
//! This module provides an exit message cell that is displayed when the user
//! quits the session, showing a goodbye message, session ID, and session statistics
//! including message counts, tool calls, skills used, and subagents invoked.

use crate::history_cell::HistoryCell;
use crate::history_cell::card_inner_width;
use crate::history_cell::with_border;
use crate::session_stats::SessionStats;
use ratatui::prelude::*;
use ratatui::style::Stylize;

/// Maximum inner width for the exit message card.
const EXIT_MESSAGE_MAX_INNER_WIDTH: usize = 60;

/// The Nori-branded exit message cell.
#[derive(Debug)]
pub(crate) struct ExitMessageCell {
    session_id: String,
    stats: SessionStats,
}

impl ExitMessageCell {
    pub(crate) fn new(session_id: String, stats: SessionStats) -> Self {
        Self { session_id, stats }
    }
}

impl HistoryCell for ExitMessageCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let Some(_inner_width) = card_inner_width(width, EXIT_MESSAGE_MAX_INNER_WIDTH) else {
            return Vec::new();
        };

        let mut lines: Vec<Line<'static>> = vec![
            // Goodbye message
            Line::from(vec![
                Span::from("Goodbye! ").green().bold(),
                Span::from("Thanks for using Nori.").dim(),
            ]),
            // Empty line
            Line::from(""),
        ];

        // Session ID line
        lines.push(Line::from(vec![
            Span::from("Session: ").dim(),
            Span::from(self.session_id.clone()),
        ]));

        // Empty line before statistics
        lines.push(Line::from(""));

        // Messages section
        let total_messages = self.stats.user_messages + self.stats.assistant_messages;
        lines.push(Line::from(vec![
            Span::from("Messages").bold(),
            Span::from(format!(
                "      User: {}  Assistant: {}  Total: {}",
                self.stats.user_messages, self.stats.assistant_messages, total_messages
            ))
            .dim(),
        ]));

        // Tool Calls section
        let tool_calls_text = if self.stats.tool_calls.is_empty() {
            "(none)".to_string()
        } else {
            let mut sorted: Vec<_> = self.stats.tool_calls.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(b.0));
            sorted
                .iter()
                .map(|(name, count)| format!("{name}: {count}"))
                .collect::<Vec<_>>()
                .join("  ")
        };
        lines.push(Line::from(vec![
            Span::from("Tool Calls").bold(),
            Span::from("    "),
            Span::from(tool_calls_text).dim(),
        ]));

        // Skills Used section
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::from("Skills Used").bold()]));
        if self.stats.skills_used.is_empty() {
            lines.push(Line::from(vec![Span::from("  (none)").dim()]));
        } else {
            for skill in &self.stats.skills_used {
                lines.push(Line::from(format!("  {skill}")));
            }
        }

        // Subagents Used section
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::from("Subagents Used").bold()]));
        if self.stats.subagents_used.is_empty() {
            lines.push(Line::from(vec![Span::from("  (none)").dim()]));
        } else {
            for subagent in &self.stats.subagents_used {
                lines.push(Line::from(format!("  {subagent}")));
            }
        }

        with_border(lines)
    }
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
        let mut stats = SessionStats::new();
        stats.user_messages = 5;
        stats.assistant_messages = 7;
        let cell = ExitMessageCell::new("abc123".to_string(), stats);
        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        // Should contain goodbye message
        assert!(
            rendered.contains("Goodbye!"),
            "Exit message should contain goodbye"
        );

        // Should contain session ID
        assert!(
            rendered.contains("Session:"),
            "Exit message should show session label"
        );
        assert!(
            rendered.contains("abc123"),
            "Exit message should show session ID"
        );

        // Should contain message counts
        assert!(
            rendered.contains("Messages"),
            "Exit message should show Messages label"
        );
        assert!(
            rendered.contains("User: 5"),
            "Exit message should show user message count"
        );
        assert!(
            rendered.contains("Assistant: 7"),
            "Exit message should show assistant message count"
        );
    }

    #[test]
    fn exit_message_shows_tool_calls() {
        let mut stats = SessionStats::new();
        stats.tool_calls.insert("Bash".to_string(), 3);
        stats.tool_calls.insert("Read".to_string(), 2);
        let cell = ExitMessageCell::new("test123".to_string(), stats);
        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        assert!(
            rendered.contains("Tool Calls"),
            "Exit message should show Tool Calls label"
        );
        assert!(
            rendered.contains("Bash: 3"),
            "Exit message should show Bash count"
        );
        assert!(
            rendered.contains("Read: 2"),
            "Exit message should show Read count"
        );
    }

    #[test]
    fn exit_message_shows_no_tool_calls_when_empty() {
        let stats = SessionStats::new();
        let cell = ExitMessageCell::new("test123".to_string(), stats);
        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        assert!(
            rendered.contains("Tool Calls"),
            "Exit message should show Tool Calls label"
        );
        assert!(
            rendered.contains("(none)"),
            "Exit message should show (none) for empty tool calls"
        );
    }

    #[test]
    fn exit_message_shows_skills_used() {
        let mut stats = SessionStats::new();
        stats.skills_used.push("brainstorming".to_string());
        stats.skills_used.push("tdd".to_string());
        let cell = ExitMessageCell::new("test123".to_string(), stats);
        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        assert!(
            rendered.contains("Skills Used"),
            "Exit message should show Skills Used section"
        );
        assert!(
            rendered.contains("brainstorming"),
            "Exit message should show brainstorming skill"
        );
        assert!(
            rendered.contains("tdd"),
            "Exit message should show tdd skill"
        );
    }

    #[test]
    fn exit_message_shows_no_skills_when_empty() {
        let stats = SessionStats::new();
        let cell = ExitMessageCell::new("test123".to_string(), stats);
        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        assert!(
            rendered.contains("Skills Used"),
            "Exit message should show Skills Used section"
        );
        // Check for (none) indicator after Skills Used
        let skills_idx = rendered.find("Skills Used").unwrap();
        let after_skills = &rendered[skills_idx..];
        assert!(
            after_skills.contains("(none)"),
            "Exit message should show (none) for empty skills"
        );
    }

    #[test]
    fn exit_message_shows_subagents_used() {
        let mut stats = SessionStats::new();
        stats
            .subagents_used
            .push("nori-codebase-locator".to_string());
        stats
            .subagents_used
            .push("nori-knowledge-researcher".to_string());
        let cell = ExitMessageCell::new("test123".to_string(), stats);
        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        assert!(
            rendered.contains("Subagents Used"),
            "Exit message should show Subagents Used section"
        );
        assert!(
            rendered.contains("nori-codebase-locator"),
            "Exit message should show locator subagent"
        );
        assert!(
            rendered.contains("nori-knowledge-researcher"),
            "Exit message should show researcher subagent"
        );
    }

    #[test]
    fn exit_message_shows_no_subagents_when_empty() {
        let stats = SessionStats::new();
        let cell = ExitMessageCell::new("test123".to_string(), stats);
        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        assert!(
            rendered.contains("Subagents Used"),
            "Exit message should show Subagents Used section"
        );
        // Check for (none) indicator after Subagents Used
        let subagents_idx = rendered.find("Subagents Used").unwrap();
        let after_subagents = &rendered[subagents_idx..];
        assert!(
            after_subagents.contains("(none)"),
            "Exit message should show (none) for empty subagents"
        );
    }

    #[test]
    fn exit_message_snapshot() {
        let mut stats = SessionStats::new();
        stats.user_messages = 5;
        stats.assistant_messages = 8;
        stats.tool_calls.insert("Bash".to_string(), 12);
        stats.tool_calls.insert("Read".to_string(), 25);
        stats.tool_calls.insert("Edit".to_string(), 7);
        stats.skills_used.push("commit".to_string());
        stats.skills_used.push("review-pr".to_string());
        stats
            .subagents_used
            .push("nori-codebase-locator".to_string());

        let cell = ExitMessageCell::new("sess_abc123def456".to_string(), stats);
        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        insta::assert_snapshot!(rendered);
    }

    #[test]
    fn exit_message_empty_session_snapshot() {
        let stats = SessionStats::new();
        let cell = ExitMessageCell::new("sess_empty".to_string(), stats);
        let lines = cell.display_lines(80);
        let rendered = render_lines(&lines).join("\n");

        insta::assert_snapshot!(rendered);
    }
}
