//! The start of an agent's latest reply, as session lists show it.

use crate::AcpEvent;
use crate::NoriEvent;
use crate::SessionEvent;
use crate::SessionPhase;
use crate::acp::v1 as acp;

/// Characters of a reply kept; lists cut it further to their column.
const REPLY_CHARS: usize = 200;

/// Follows one session's events and keeps the start of the agent's latest
/// reply: the text it sent since the last user message, tool call, request or
/// answered request.
/// The previous reply stays until the next one's first text, so a session
/// that is thinking or running a tool still shows what it last said.
#[derive(Debug, Default, Clone)]
pub struct ReplyPreview {
    reply: String,
    /// Something ended the reply; the next agent text starts a new one.
    stale: bool,
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
                        }
                        // Only the start is shown; whitespace may collapse
                        // to a quarter of the raw text at most.
                        if self.reply.chars().count() < REPLY_CHARS * 4 {
                            self.reply.push_str(&text.text);
                        }
                    }
                }
                // A loaded history replays its turns with no phase changes
                // between them, so a user message ends the reply too.
                acp::SessionUpdate::UserMessageChunk(_) | acp::SessionUpdate::ToolCall(_) => {
                    self.stale = true;
                }
                _ => {}
            },
            // A request, or its answer when the request itself was seen
            // elsewhere, also ends the reply.
            SessionEvent::Acp(
                AcpEvent::Request { .. }
                | AcpEvent::Response { .. }
                | AcpEvent::ConfigSnapshot { .. },
            )
            | SessionEvent::Nori(NoriEvent::RequestResolved(_))
            | SessionEvent::Nori(NoriEvent::SessionPhaseChanged(
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn update(update: acp::SessionUpdate) -> SessionEvent {
        SessionEvent::Acp(AcpEvent::Notification(
            acp::AgentNotification::SessionNotification(acp::SessionNotification::new(
                acp::SessionId::new("s"),
                update,
            )),
        ))
    }

    fn agent(text: &str) -> SessionEvent {
        update(acp::SessionUpdate::AgentMessageChunk(
            acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(text))),
        ))
    }

    fn user(text: &str) -> SessionEvent {
        update(acp::SessionUpdate::UserMessageChunk(
            acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(text))),
        ))
    }

    fn tool() -> SessionEvent {
        update(acp::SessionUpdate::ToolCall(acp::ToolCall::new(
            acp::ToolCallId::new("t"),
            "rg recover",
        )))
    }

    fn phase(phase: SessionPhase) -> SessionEvent {
        SessionEvent::Nori(NoriEvent::SessionPhaseChanged(phase))
    }

    fn after(events: &[SessionEvent]) -> Option<String> {
        let mut preview = ReplyPreview::default();
        for event in events {
            preview.observe(event);
        }
        preview.text()
    }

    #[test]
    fn the_reply_reads_from_its_start_on_one_line() {
        assert_eq!(
            after(&[agent("Fixed the\n\nbrace,   "), agent("tests\tpass.")]),
            Some("Fixed the brace, tests pass.".to_string())
        );
    }

    #[test]
    fn text_after_a_tool_or_a_request_is_the_new_reply() {
        let request = SessionEvent::Acp(AcpEvent::Request {
            request_id: acp::RequestId::Number(1),
            request: acp::AgentRequest::RequestPermissionRequest(
                acp::RequestPermissionRequest::new(
                    acp::SessionId::new("s"),
                    acp::ToolCallUpdate::new(
                        acp::ToolCallId::new("p"),
                        acp::ToolCallUpdateFields::new(),
                    ),
                    Vec::new(),
                ),
            ),
        });
        assert_eq!(
            after(&[agent("Let me look."), tool(), agent("Found it.")]),
            Some("Found it.".to_string())
        );
        assert_eq!(
            after(&[agent("Asking."), request, agent("Done.")]),
            Some("Done.".to_string())
        );
    }

    #[test]
    fn text_after_an_answered_request_is_the_new_reply() {
        let resolved = SessionEvent::Nori(NoriEvent::RequestResolved(crate::RequestResolved {
            request_id: acp::RequestId::Number(1),
            answered_by: "client".to_string(),
        }));
        assert_eq!(
            after(&[
                agent("Asking first."),
                resolved,
                agent("Permission granted.")
            ]),
            Some("Permission granted.".to_string())
        );
    }

    #[test]
    fn a_new_turn_keeps_the_last_reply_until_the_agent_answers_again() {
        let prompting = phase(SessionPhase::Prompting {
            request_id: acp::RequestId::Number(2),
        });
        assert_eq!(
            after(&[agent("Old answer"), prompting.clone(), user("and now?")]),
            Some("Old answer".to_string())
        );
        assert_eq!(
            after(&[agent("Old answer"), prompting, agent("New answer")]),
            Some("New answer".to_string())
        );
    }

    #[test]
    fn a_loaded_history_shows_its_last_turns_reply() {
        let loading = phase(SessionPhase::Loading {
            request_id: acp::RequestId::Number(1),
        });
        assert_eq!(
            after(&[
                loading,
                user("first question"),
                agent("first answer"),
                user("second question"),
                agent("second answer"),
            ]),
            Some("second answer".to_string())
        );
    }

    #[test]
    fn a_long_reply_keeps_its_first_two_hundred_characters() {
        let long = "word ".repeat(400);
        let text = after(&[agent(&long)]).unwrap_or_default();
        assert_eq!(text.chars().count(), 200);
        assert!(text.starts_with("word word"));
    }

    #[test]
    fn nothing_said_yet_is_none() {
        assert_eq!(after(&[user("hello")]), None);
    }
}
