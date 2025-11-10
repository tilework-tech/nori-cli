use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum ConversationEvent {
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
    StderrOutput {
        line: String,
    },
    UnknownEvent {
        raw: String,
    },
}

pub fn render_event(event: &ConversationEvent) -> Line<'static> {
    match event {
        ConversationEvent::AssistantMessage { text } => Line::from(text.clone()),
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
        ConversationEvent::UnknownEvent { raw } => Line::from(Span::styled(
            format!("[unknown] {}", raw),
            Style::default().fg(Color::Yellow),
        )),
    }
}
