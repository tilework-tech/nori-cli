use crate::conversation::{ConversationEvent, render_event};
use ratatui::text::{Line, Span};

pub type InlineEntryId = String;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineEntryKind {
    AssistantMessage,
    AgentThinking,
}

#[derive(Debug, Clone)]
pub enum InlineEntryUpdate {
    AppendText(String),
}

#[derive(Debug, Clone)]
pub struct InlineEntryState {
    pub id: InlineEntryId,
    pub kind: InlineEntryKind,
    buffer: String,
    wrapped_lines: Vec<Line<'static>>,
    last_width: usize,
}

#[derive(Debug, Clone)]
pub struct CommittedInlineEntry {
    pub lines: Vec<Line<'static>>,
    pub height: u16,
    pub event: ConversationEvent,
}

impl InlineEntryState {
    pub fn new(id: InlineEntryId, kind: InlineEntryKind) -> Self {
        Self {
            id,
            kind,
            buffer: String::new(),
            wrapped_lines: vec![Line::from("")],
            last_width: 0,
        }
    }

    pub fn apply_update(&mut self, update: InlineEntryUpdate, width: usize) {
        match update {
            InlineEntryUpdate::AppendText(text) => {
                self.buffer.push_str(&text);
            }
        }

        self.rewrap(width);
    }

    pub fn rewrap(&mut self, width: usize) {
        if width == 0 {
            return;
        }
        self.last_width = width;

        let event = self.to_event();
        let rendered = render_event(&event);
        // Split the text on newlines to preserve whitespace and newlines
        let empty_span = Span::default();
        let text_span = &rendered.spans.last().unwrap_or(&empty_span); // The text span
        let text_lines: Vec<&str> = text_span.content.lines().collect();
        let mut lines = Vec::new();
        for (i, text_line) in text_lines.iter().enumerate() {
            let mut spans = Vec::new();
            if i == 0 {
                // First line with prefix
                spans.push(rendered.spans[0].clone());
                spans.push(rendered.spans[1].clone());
            } else {
                // Subsequent lines with indentation to match prefix
                spans.push(Span::raw("        ")); // 8 spaces to match "[agent] "
            }
            spans.push(Span::styled(text_line.to_string(), text_span.style));
            lines.push(Line::from(spans));
        }
        if lines.is_empty() {
            lines.push(Line::from(""));
        }
        self.wrapped_lines = lines;
    }

    pub fn height(&self) -> u16 {
        self.wrapped_lines.len() as u16
    }

    pub fn lines(&self) -> &[Line<'static>] {
        &self.wrapped_lines
    }

    pub fn to_event(&self) -> ConversationEvent {
        match self.kind {
            InlineEntryKind::AssistantMessage => ConversationEvent::AssistantMessage {
                text: self.buffer.clone(),
            },
            InlineEntryKind::AgentThinking => ConversationEvent::AgentThinking {
                text: self.buffer.clone(),
            },
        }
    }

    pub fn into_committed(self) -> CommittedInlineEntry {
        let event = self.to_event();
        let height = self.wrapped_lines.len() as u16;
        let lines = self.wrapped_lines;
        CommittedInlineEntry {
            lines,
            height,
            event,
        }
    }
}
