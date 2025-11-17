use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum ConversationEvent {
    UserMessage {
        text: String,
    },
    AssistantMessage {
        text: String,
    },
    SystemEvent {
        subtype: String,
        details: Option<String>,
    },
    ResultSummary {
        success: bool,
        details: String,
    },
    #[allow(dead_code)]
    StderrOutput {
        line: String,
    },
    #[allow(dead_code)]
    StreamCancelled,
    UnknownEvent {
        raw: String,
    },
    StatusMessage {
        text: String,
    },
    // ACP-specific events
    ToolCallStarted {
        id: String,
        title: String,
        kind: String, // "edit", "write", "bash", "other"
    },
    ToolCallProgress {
        id: String,
        status: String, // "pending", "in_progress", "completed", "failed", "cancelled"
        content: Option<String>,
    },
    AgentPlan {
        entries: Vec<PlanEntry>,
    },
    AgentThinking {
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanEntry {
    pub content: String,
    pub status: String, // "pending", "in_progress", "completed"
    pub priority: Option<String>,
}

pub fn render_event(event: &ConversationEvent) -> Line<'static> {
    match event {
        ConversationEvent::UserMessage { text } => Line::from(vec![
            Span::styled(
                "[user] ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(text.clone()),
        ]),
        ConversationEvent::AssistantMessage { text } => Line::from(vec![
            Span::styled(
                "[agent] ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(text.clone()),
        ]),
        ConversationEvent::SystemEvent { subtype, details } => {
            let mut spans = vec![
                Span::styled(
                    "[system] ",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                ),
                Span::raw(subtype.clone()),
            ];

            if let Some(d) = details {
                spans.push(Span::raw(": "));
                spans.push(Span::styled(
                    d.clone(),
                    Style::default().fg(Color::DarkGray),
                ));
            }

            Line::from(spans)
        }
        ConversationEvent::ResultSummary { success, details } => {
            let prefix_style = if *success {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            };

            Line::from(vec![
                Span::styled("[done] ", prefix_style),
                Span::raw(details.clone()),
            ])
        }
        ConversationEvent::StderrOutput { line } => {
            Line::from(Span::styled(line.clone(), Style::default().fg(Color::Red)))
        }
        ConversationEvent::StreamCancelled => {
            Line::from(Span::styled("Interrupted", Style::default().fg(Color::Red)))
        }
        ConversationEvent::UnknownEvent { raw } => Line::from(Span::styled(
            format!("[unknown] {raw}"),
            Style::default().fg(Color::Yellow),
        )),
        ConversationEvent::StatusMessage { text } => Line::from(vec![
            Span::styled(
                "[status] ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(text.clone()),
        ]),
        ConversationEvent::ToolCallStarted { id: _, title, kind } => Line::from(vec![
            Span::styled(
                "[tool] ",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("{title} ({kind})")),
        ]),
        ConversationEvent::ToolCallProgress {
            id: _,
            status,
            content,
        } => {
            let mut spans = vec![
                Span::styled(
                    "[tool] ",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(status.clone()),
            ];
            if let Some(c) = content {
                spans.push(Span::raw(": "));
                spans.push(Span::raw(c.clone()));
            }
            Line::from(spans)
        }
        ConversationEvent::AgentPlan { entries } => {
            let summary = format!("Plan: {} steps", entries.len());
            Line::from(vec![
                Span::styled(
                    "[plan] ",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(summary),
            ])
        }
        ConversationEvent::AgentThinking { text } => Line::from(vec![
            Span::styled(
                "[thinking] ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD | Modifier::ITALIC),
            ),
            Span::raw(text.clone()),
        ]),
    }
}

/// Determines if an event should be rendered based on debug mode
/// SystemEvent, UnknownEvent, and ResultSummary are debug events (hidden when show_debug is false)
/// All other events are always visible
pub fn should_render_event(event: &ConversationEvent, show_debug: bool) -> bool {
    match event {
        ConversationEvent::SystemEvent { .. }
        | ConversationEvent::UnknownEvent { .. }
        | ConversationEvent::ResultSummary { .. } => show_debug,
        _ => true,
    }
}
