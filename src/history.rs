use crate::conversation::{ConversationEvent, render_event};
use crate::text_utils::wrap_text_to_width;
use ratatui::text::{Line, Text};

pub type InlineEntryId = String;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineEntryKind {
    AssistantMessage,
}

#[derive(Debug, Clone)]
enum InlineEntryData {
    AssistantText { buffer: String },
}

#[derive(Debug, Clone)]
pub enum InlineEntryUpdate {
    AppendText(String),
}

#[derive(Debug, Clone)]
pub struct InlineEntryState {
    pub id: InlineEntryId,
    data: InlineEntryData,
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
        let data = match kind {
            InlineEntryKind::AssistantMessage => InlineEntryData::AssistantText {
                buffer: String::new(),
            },
        };

        Self {
            id,
            data,
            wrapped_lines: vec![Line::from("")],
            last_width: 0,
        }
    }

    pub fn apply_update(&mut self, update: InlineEntryUpdate, width: usize) {
        match (&mut self.data, update) {
            (InlineEntryData::AssistantText { buffer }, InlineEntryUpdate::AppendText(text)) => {
                buffer.push_str(&text);
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
        let text = Text::from(render_event(&event));
        let wrapped = wrap_text_to_width(&text, width);
        if wrapped.is_empty() {
            self.wrapped_lines = vec![Line::from("")];
        } else {
            self.wrapped_lines = wrapped;
        }
    }

    pub fn height(&self) -> u16 {
        self.wrapped_lines.len() as u16
    }

    pub fn lines(&self) -> &[Line<'static>] {
        &self.wrapped_lines
    }

    pub fn to_event(&self) -> ConversationEvent {
        match &self.data {
            InlineEntryData::AssistantText { buffer } => ConversationEvent::AssistantMessage {
                text: buffer.clone(),
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
