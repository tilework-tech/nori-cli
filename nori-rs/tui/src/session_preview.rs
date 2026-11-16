//! What the session list shows about each session: its title, the start of
//! the agent's latest reply, a request waiting on the user, and when the
//! session last did anything.
//!
//! Everything is derived from the session's own events, never from a call to
//! the agent.

use std::collections::HashMap;
use std::time::Instant;

use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use nori_protocol::acp::v1 as acp;

use crate::app_event::SessionGeneration;

#[derive(Default)]
struct Preview {
    /// The agent's own title, or the prompt summary, whichever came last.
    title: Option<String>,
    /// The session's first user message, which titles it until the agent
    /// names it. Stays open for more chunks until something else arrives.
    first_prompt: Option<String>,
    first_prompt_open: bool,
    /// Requests waiting on the user.
    pending: nori_protocol::PendingRequest,
    reply: nori_protocol::ReplyPreview,
    last_activity: Option<Instant>,
}

/// Titles and previews for every session, keyed by session generation.
#[derive(Default)]
pub(crate) struct SessionPreviews {
    sessions: HashMap<SessionGeneration, Preview>,
}

impl SessionPreviews {
    pub(crate) fn observe(&mut self, generation: SessionGeneration, event: &SessionEvent) {
        let preview = self.sessions.entry(generation).or_default();
        preview.last_activity = Some(Instant::now());
        preview.reply.observe(event);
        preview.pending.observe(event);
        let user_chunk = matches!(
            event,
            SessionEvent::Acp(AcpEvent::Notification(
                acp::AgentNotification::SessionNotification(notification),
            )) if matches!(notification.update, acp::SessionUpdate::UserMessageChunk(_))
        );
        if !user_chunk {
            preview.first_prompt_open = false;
        }
        match event {
            SessionEvent::Acp(AcpEvent::Notification(
                acp::AgentNotification::SessionNotification(notification),
            )) => observe_update(preview, &notification.update),
            SessionEvent::Acp(_) => {}
            SessionEvent::Nori(NoriEvent::PromptSummaryUpdated(summary)) => {
                preview.title = Some(summary.summary.clone());
            }
            // A `/delegate` sent before any prompt names the session by its
            // task. A child's own first prompt is that same task.
            SessionEvent::Nori(NoriEvent::DelegationChanged(delegation))
                if preview.first_prompt.is_none()
                    && delegation.origin == nori_protocol::DelegationOrigin::User =>
            {
                preview.first_prompt = delegation.task.lines().next().map(str::to_owned);
                preview.first_prompt_open = false;
            }
            SessionEvent::Nori(_) => {}
        }
    }

    /// Title a session that took another's place, such as a restarted
    /// session, until it has a title of its own.
    pub(crate) fn inherit_title(&mut self, generation: SessionGeneration, title: String) {
        let preview = self.sessions.entry(generation).or_default();
        if preview.first_prompt.is_none() {
            preview.first_prompt = Some(title);
        }
    }

    /// The title the agent gave the session, or the prompt summary, whichever
    /// came last. Falls back to the first user message.
    pub(crate) fn title(&self, generation: SessionGeneration) -> Option<String> {
        self.sessions.get(&generation).and_then(|preview| {
            preview
                .title
                .clone()
                .or_else(|| preview.first_prompt.clone())
        })
    }

    /// The start of the agent's latest reply on one line, for the session
    /// list's second row.
    pub(crate) fn reply(&self, generation: SessionGeneration) -> Option<String> {
        self.sessions.get(&generation)?.reply.text()
    }

    /// When this session last had any event.
    pub(crate) fn last_activity(&self, generation: SessionGeneration) -> Option<Instant> {
        self.sessions.get(&generation)?.last_activity
    }

    /// The request this session is waiting on, if any, as shown in its preview.
    pub(crate) fn pending_request(&self, generation: SessionGeneration) -> Option<String> {
        self.sessions.get(&generation)?.pending.text()
    }
}

fn observe_update(preview: &mut Preview, update: &acp::SessionUpdate) {
    match update {
        acp::SessionUpdate::UserMessageChunk(chunk) => {
            if let acp::ContentBlock::Text(text) = &chunk.content {
                match &mut preview.first_prompt {
                    // A child's result is not the user's own first prompt.
                    None if nori_protocol::delegation_result_summary(&text.text).is_some() => {}
                    None => {
                        preview.first_prompt = Some(text.text.clone());
                        preview.first_prompt_open = true;
                    }
                    Some(prompt) if preview.first_prompt_open => prompt.push_str(&text.text),
                    Some(_) => {}
                }
            }
        }
        acp::SessionUpdate::SessionInfoUpdate(info) => {
            if let Some(title) = info.title.as_opt_ref() {
                preview.title = title.cloned();
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nori_protocol::SessionPhase;
    use pretty_assertions::assert_eq;

    fn update(update: acp::SessionUpdate) -> SessionEvent {
        SessionEvent::Acp(AcpEvent::Notification(
            acp::AgentNotification::SessionNotification(acp::SessionNotification::new(
                acp::SessionId::new("s"),
                update,
            )),
        ))
    }

    fn text(chunk: &str) -> SessionEvent {
        update(acp::SessionUpdate::AgentMessageChunk(
            acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(chunk))),
        ))
    }

    fn tool(id: &str, title: &str) -> SessionEvent {
        update(acp::SessionUpdate::ToolCall(
            acp::ToolCall::new(acp::ToolCallId::new(id), title)
                .status(acp::ToolCallStatus::InProgress),
        ))
    }

    fn permission(id: i64, command: &str) -> SessionEvent {
        SessionEvent::Acp(AcpEvent::Request {
            request_id: acp::RequestId::Number(id),
            request: acp::AgentRequest::RequestPermissionRequest(
                acp::RequestPermissionRequest::new(
                    acp::SessionId::new("s"),
                    acp::ToolCallUpdate::new(
                        acp::ToolCallId::new("perm"),
                        acp::ToolCallUpdateFields::new()
                            .title("Execute shell command")
                            .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                                acp::ContentBlock::Text(acp::TextContent::new(command)),
                            ))]),
                    ),
                    Vec::new(),
                ),
            ),
        })
    }

    fn resolved(id: i64) -> SessionEvent {
        SessionEvent::Nori(NoriEvent::RequestResolved(nori_protocol::RequestResolved {
            request_id: acp::RequestId::Number(id),
            answered_by: "client".to_string(),
        }))
    }

    #[test]
    fn a_waiting_request_is_reported_until_it_is_answered() {
        let mut previews = SessionPreviews::default();
        previews.observe(1, &permission(7, "rm -rf target/debug"));
        let waiting = previews.pending_request(1);
        previews.observe(1, &resolved(7));
        assert_eq!(
            (waiting, previews.pending_request(1)),
            (Some("approve: rm -rf target/debug".to_string()), None)
        );
    }

    #[test]
    fn the_title_follows_the_prompt_summary() {
        let mut previews = SessionPreviews::default();
        previews.observe(
            1,
            &SessionEvent::Nori(NoriEvent::SessionPhaseChanged(SessionPhase::Prompting {
                request_id: acp::RequestId::Number(1),
            })),
        );
        previews.observe(
            1,
            &SessionEvent::Nori(NoriEvent::PromptSummaryUpdated(
                nori_protocol::PromptSummary {
                    summary: "Fix the login test".to_string(),
                },
            )),
        );
        assert_eq!(previews.title(1), Some("Fix the login test".to_string()));
    }

    fn user_text(chunk: &str) -> SessionEvent {
        update(acp::SessionUpdate::UserMessageChunk(
            acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(chunk))),
        ))
    }

    #[test]
    fn the_first_user_message_titles_a_session_until_the_agent_summarises_it() {
        let mut previews = SessionPreviews::default();
        for event in [
            user_text("fix the "),
            user_text("login test"),
            text("On it"),
        ] {
            previews.observe(1, &event);
        }
        previews.observe(
            1,
            &SessionEvent::Nori(NoriEvent::SessionPhaseChanged(SessionPhase::Prompting {
                request_id: acp::RequestId::Number(2),
            })),
        );
        previews.observe(1, &user_text("second prompt"));
        let before_summary = previews.title(1);
        previews.observe(
            1,
            &SessionEvent::Nori(NoriEvent::PromptSummaryUpdated(
                nori_protocol::PromptSummary {
                    summary: "Fix the login test".to_string(),
                },
            )),
        );
        assert_eq!(
            (before_summary, previews.title(1)),
            (
                Some("fix the login test".to_string()),
                Some("Fix the login test".to_string())
            )
        );
    }

    #[test]
    fn a_delegation_result_never_titles_the_session_that_receives_it() {
        let mut previews = SessionPreviews::default();
        previews.observe(
            1,
            &user_text(
                "[Nori delegation result] from mock-model · inspect the parser · done\n\
                 Child session: aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\nAll good.",
            ),
        );
        previews.observe(1, &text("Thanks"));
        assert_eq!(previews.title(1), None);
        previews.observe(
            1,
            &SessionEvent::Nori(NoriEvent::SessionPhaseChanged(SessionPhase::Prompting {
                request_id: acp::RequestId::Number(2),
            })),
        );
        previews.observe(1, &user_text("now fix it"));
        assert_eq!(previews.title(1), Some("now fix it".to_string()));
    }

    #[test]
    fn a_delegation_sent_before_any_prompt_names_the_session_by_its_task() {
        let delegation = nori_protocol::Delegation {
            request_id: "request-1".into(),
            parent_id: "parent".into(),
            child_id: "child".into(),
            parent_agent: "mock-model".into(),
            agent: "mock-model".into(),
            task: "inspect the parser\nand report".into(),
            status: nori_protocol::DelegationStatus::Running,
            depth: 1,
            result: None,
            origin: nori_protocol::DelegationOrigin::User,
        };
        let mut previews = SessionPreviews::default();
        previews.observe(
            1,
            &SessionEvent::Nori(NoriEvent::DelegationChanged(delegation)),
        );
        previews.observe(1, &user_text("now fix it"));
        assert_eq!(previews.title(1), Some("inspect the parser".to_string()));
    }

    fn agent_title(title: Option<&str>) -> SessionEvent {
        update(acp::SessionUpdate::SessionInfoUpdate(
            acp::SessionInfoUpdate::new().title(title.map(str::to_owned)),
        ))
    }

    #[test]
    fn the_list_reply_is_the_agents_latest_reply() {
        let mut previews = SessionPreviews::default();
        for event in [text("Let me look."), tool("t", "rg"), text("Found   it.")] {
            previews.observe(1, &event);
        }
        assert_eq!(previews.reply(1), Some("Found it.".to_string()));
    }

    #[test]
    fn the_agents_own_title_renames_the_session_and_clearing_it_falls_back_to_the_first_prompt() {
        let mut previews = SessionPreviews::default();
        previews.observe(1, &user_text("say hi in 3 words"));
        previews.observe(1, &text("Hi there, friend"));
        let before = previews.title(1);
        previews.observe(1, &agent_title(Some("Greeting in three words")));
        let renamed = previews.title(1);
        previews.observe(1, &agent_title(None));
        assert_eq!(
            (before, renamed, previews.title(1)),
            (
                Some("say hi in 3 words".to_string()),
                Some("Greeting in three words".to_string()),
                Some("say hi in 3 words".to_string()),
            )
        );
    }

    #[test]
    fn any_event_marks_the_session_active() {
        let mut previews = SessionPreviews::default();
        let before = std::time::Instant::now();
        assert_eq!(previews.last_activity(1), None);
        previews.observe(1, &text("hi"));
        assert!(previews.last_activity(1).is_some_and(|at| at >= before));
    }
}
