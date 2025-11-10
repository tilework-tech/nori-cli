use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum ConversationEvent {
    AssistantMessage { text: String },
    SystemEvent { subtype: String, details: Option<String> },
    ResultSummary { success: bool, details: String },
    StderrOutput { line: String },
    UnknownEvent { raw: String },
}

pub fn parse_jsonl_event(line: &str) -> Option<ConversationEvent> {
    let json: Value = serde_json::from_str(line).ok()?;
    let event_type = json.get("type")?.as_str()?;

    match event_type {
        "assistant" => {
            let message = json.get("message")?;
            let content_array = message.get("content")?.as_array()?;

            let mut text = String::new();
            for item in content_array {
                if let Some("text") = item.get("type").and_then(|v| v.as_str()) {
                    if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(t);
                    }
                }
            }

            Some(ConversationEvent::AssistantMessage { text })
        }
        "system" => {
            let subtype = json.get("subtype")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            let details = json.get("cwd")
                .or_else(|| json.get("session_id"))
                .and_then(|v| v.as_str())
                .map(String::from);

            Some(ConversationEvent::SystemEvent { subtype, details })
        }
        "result" => {
            let subtype = json.get("subtype").and_then(|v| v.as_str());
            let success = subtype == Some("success");

            let details = json.get("result")
                .and_then(|v| v.as_str())
                .unwrap_or("Completed")
                .to_string();

            Some(ConversationEvent::ResultSummary { success, details })
        }
        _ => {
            Some(ConversationEvent::UnknownEvent { raw: line.to_string() })
        }
    }
}

pub fn render_event(event: &ConversationEvent) -> Line<'static> {
    match event {
        ConversationEvent::AssistantMessage { text } => {
            Line::from(text.clone())
        }
        ConversationEvent::SystemEvent { subtype, details } => {
            let mut spans = vec![
                Span::styled(
                    "[system] ",
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
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
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            };

            Line::from(vec![
                Span::styled("[done] ", prefix_style),
                Span::raw(details.clone()),
            ])
        }
        ConversationEvent::StderrOutput { line } => {
            Line::from(Span::styled(
                line.clone(),
                Style::default().fg(Color::Red),
            ))
        }
        ConversationEvent::UnknownEvent { raw } => {
            Line::from(Span::styled(
                format!("[unknown] {}", raw),
                Style::default().fg(Color::Yellow),
            ))
        }
    }
}
