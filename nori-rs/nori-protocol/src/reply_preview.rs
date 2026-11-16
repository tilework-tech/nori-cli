//! The start of an agent's latest reply, as session lists show it.

use crate::AcpEvent;
use crate::NoriEvent;
use crate::SessionEvent;
use crate::SessionPhase;
use crate::acp::v1 as acp;

/// Characters of a reply kept; lists cut it further to their column.
const REPLY_CHARS: usize = 200;

/// Follows one session's events and keeps its latest reply from the start.
/// The reply of the previous turn stays until the next turn's first text.
#[derive(Debug, Default, Clone)]
pub struct ReplyPreview {
    reply: String,
    /// A new turn began; its first agent text replaces the reply.
    stale: bool,
    /// A tool or a request came after the last text, so the next text
    /// starts a new sentence.
    broken: bool,
}

impl ReplyPreview {
    pub fn observe(&mut self, event: &SessionEvent) {
        match event {
            SessionEvent::Acp(AcpEvent::Notification(
                acp::AgentNotification::SessionNotification(notification),
            )) => match &notification.update {
                acp::SessionUpdate::AgentMessageChunk(chunk) => {
                    if let acp::ContentBlock::Text(text) = &chunk.content {
                        if std::mem::take(&mut self.stale) {
                            self.reply.clear();
                            self.broken = false;
                        }
                        if std::mem::take(&mut self.broken) && !self.reply.is_empty() {
                            self.reply.push(' ');
                        }
                        self.reply.push_str(&text.text);
                    }
                }
                acp::SessionUpdate::ToolCall(_) => self.broken = true,
                _ => {}
            },
            SessionEvent::Acp(AcpEvent::Request { .. }) => self.broken = true,
            SessionEvent::Nori(NoriEvent::SessionPhaseChanged(
                SessionPhase::Prompting { .. } | SessionPhase::Loading { .. },
            )) => self.stale = true,
            SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
        }
    }

    /// The reply on one line, whitespace collapsed, or `None` before the
    /// agent has said anything.
    pub fn text(&self) -> Option<String> {
        let mut line = String::new();
        for word in self.reply.split_whitespace() {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
            if line.chars().count() >= REPLY_CHARS {
                break;
            }
        }
        (!line.is_empty()).then(|| line.chars().take(REPLY_CHARS).collect())
    }
}
