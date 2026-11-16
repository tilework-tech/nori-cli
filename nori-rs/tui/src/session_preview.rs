//! One-line previews of what each session is doing, for the session list.
//!
//! A preview is derived only from the session's own events, never from a call
//! to the agent. It shows the most urgent thing first: a request waiting on
//! the user, then the running tool, then how the session ended, then the
//! latest line of agent text.

use std::collections::HashMap;

use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEndReason;
use nori_protocol::SessionEvent;
use nori_protocol::SessionPhase;
use nori_protocol::acp::v1 as acp;

use crate::app_event::SessionGeneration;

#[derive(Default)]
struct Preview {
    title: Option<String>,
    /// The session's first user message, which titles it until the agent
    /// summarises it. Stays open for more chunks until something else
    /// arrives.
    first_prompt: Option<String>,
    first_prompt_open: bool,
    /// Requests waiting on the user, in arrival order.
    pending: Vec<(acp::RequestId, String)>,
    running: Vec<(acp::ToolCallId, String)>,
    latest_text: String,
    ended: Option<String>,
}

impl Preview {
    fn line(&self) -> Option<String> {
        if let Some((_, request)) = self.pending.first() {
            return Some(request.clone());
        }
        if let Some((_, tool)) = self.running.last() {
            return Some(format!("running: {tool}"));
        }
        if let Some(ended) = &self.ended {
            return Some(ended.clone());
        }
        self.latest_text
            .lines()
            .rev()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(newest_part)
    }
}

/// Previews and titles for every session, keyed by session generation.
#[derive(Default)]
pub(crate) struct SessionPreviews {
    sessions: HashMap<SessionGeneration, Preview>,
}

impl SessionPreviews {
    pub(crate) fn observe(&mut self, generation: SessionGeneration, event: &SessionEvent) {
        let preview = self.sessions.entry(generation).or_default();
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
            SessionEvent::Acp(AcpEvent::Request {
                request_id,
                request: acp::AgentRequest::RequestPermissionRequest(request),
            }) => {
                let subject = first_text(request.tool_call.fields.content.as_deref())
                    .or_else(|| request.tool_call.fields.title.clone())
                    .unwrap_or_else(|| "a tool call".to_string());
                preview.pending.push((
                    request_id.clone(),
                    format!("approve: {}", first_line(&subject)),
                ));
                break_text(preview);
            }
            SessionEvent::Acp(AcpEvent::Response { request_id, .. }) => {
                preview.pending.retain(|(id, _)| id != request_id);
            }
            SessionEvent::Acp(_) => {}
            SessionEvent::Nori(NoriEvent::RequestResolved(resolved)) => {
                preview.pending.retain(|(id, _)| *id != resolved.request_id);
            }
            SessionEvent::Nori(NoriEvent::PromptSummaryUpdated(summary)) => {
                preview.title = Some(summary.summary.clone());
            }
            SessionEvent::Nori(NoriEvent::SessionPhaseChanged(phase)) => match phase {
                SessionPhase::Idle => {
                    preview.pending.clear();
                    preview.running.clear();
                }
                SessionPhase::Prompting { .. } | SessionPhase::Loading { .. } => {
                    preview.latest_text.clear();
                    preview.ended = None;
                }
                SessionPhase::Cancelling { .. } => {}
            },
            // A `/delegate` sent before any prompt names the session by its
            // task. A child's own first prompt is that same task.
            SessionEvent::Nori(NoriEvent::DelegationChanged(delegation))
                if preview.first_prompt.is_none()
                    && delegation.origin == nori_protocol::DelegationOrigin::User =>
            {
                preview.first_prompt = delegation.task.lines().next().map(str::to_owned);
                preview.first_prompt_open = false;
            }
            SessionEvent::Nori(NoriEvent::SessionEnded(ended)) => {
                preview.pending.clear();
                preview.running.clear();
                // The status column already says a session closed; its
                // description keeps the last thing the agent said.
                preview.ended = Some(match ended.reason {
                    SessionEndReason::Shutdown | SessionEndReason::Closed => return,
                    SessionEndReason::ConnectionLost => {
                        "agent exited · resume available".to_string()
                    }
                    SessionEndReason::SpawnFailed => "agent failed to start".to_string(),
                    SessionEndReason::TimedOut => "agent timed out · resume available".to_string(),
                });
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

    pub(crate) fn preview(&self, generation: SessionGeneration) -> Option<String> {
        self.sessions.get(&generation).and_then(Preview::line)
    }

    /// The agent's own summary of what the session is about, once it has one.
    /// Falls back to the first user message until a summary arrives.
    pub(crate) fn title(&self, generation: SessionGeneration) -> Option<String> {
        self.sessions.get(&generation).and_then(|preview| {
            preview
                .title
                .clone()
                .or_else(|| preview.first_prompt.clone())
        })
    }

    /// The request this session is waiting on, if any, as shown in its preview.
    pub(crate) fn pending_request(&self, generation: SessionGeneration) -> Option<String> {
        self.sessions
            .get(&generation)
            .and_then(|preview| preview.pending.first())
            .map(|(_, request)| request.clone())
    }
}

/// End the current line of agent text, so text that follows a request or a
/// tool starts a line of its own.
fn break_text(preview: &mut Preview) {
    if !preview.latest_text.is_empty() && !preview.latest_text.ends_with('\n') {
        preview.latest_text.push('\n');
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
        acp::SessionUpdate::AgentMessageChunk(chunk) => {
            if let acp::ContentBlock::Text(text) = &chunk.content {
                preview.latest_text.push_str(&text.text);
            }
        }
        acp::SessionUpdate::ToolCall(call) => {
            if matches!(
                call.status,
                acp::ToolCallStatus::Pending | acp::ToolCallStatus::InProgress
            ) {
                preview
                    .running
                    .push((call.tool_call_id.clone(), first_line(&call.title)));
            }
            break_text(preview);
        }
        acp::SessionUpdate::ToolCallUpdate(update) => {
            let done = matches!(
                update.fields.status,
                Some(acp::ToolCallStatus::Completed | acp::ToolCallStatus::Failed)
            );
            if done {
                preview.running.retain(|(id, _)| *id != update.tool_call_id);
            } else if let Some(title) = &update.fields.title
                && let Some(entry) = preview
                    .running
                    .iter_mut()
                    .find(|(id, _)| *id == update.tool_call_id)
            {
                entry.1 = first_line(title);
            }
        }
        _ => {}
    }
}

/// Characters of a long streamed line kept for the preview, from its end,
/// so a streaming session shows what it is writing now.
const PREVIEW_TAIL_CHARS: i32 = 100;

fn newest_part(line: &str) -> String {
    let limit = usize::try_from(PREVIEW_TAIL_CHARS).unwrap_or(0);
    let count = line.chars().count();
    if count <= limit {
        return line.to_string();
    }
    let tail: String = line.chars().skip(count - limit + 1).collect();
    format!("…{tail}")
}

fn first_text(content: Option<&[acp::ToolCallContent]>) -> Option<String> {
    content?.iter().find_map(|item| match item {
        acp::ToolCallContent::Content(content) => match &content.content {
            acp::ContentBlock::Text(text) => Some(text.text.clone()),
            _ => None,
        },
        _ => None,
    })
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().trim().to_string()
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

    fn tool_done(id: &str) -> SessionEvent {
        update(acp::SessionUpdate::ToolCallUpdate(
            acp::ToolCallUpdate::new(
                acp::ToolCallId::new(id),
                acp::ToolCallUpdateFields::new().status(acp::ToolCallStatus::Completed),
            ),
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

    fn feed(events: &[SessionEvent]) -> Option<String> {
        let mut previews = SessionPreviews::default();
        for event in events {
            previews.observe(1, event);
        }
        previews.preview(1)
    }

    #[test]
    fn a_waiting_request_comes_before_a_running_tool_and_text() {
        assert_eq!(
            feed(&[
                text("Looking at the build\n"),
                tool("t1", "cargo build"),
                permission(7, "rm -rf target/debug"),
            ]),
            Some("approve: rm -rf target/debug".to_string())
        );
    }

    #[test]
    fn an_answered_request_gives_way_to_the_running_tool() {
        assert_eq!(
            feed(&[
                tool("t1", "cargo build"),
                permission(7, "rm -rf x"),
                resolved(7)
            ]),
            Some("running: cargo build".to_string())
        );
    }

    #[test]
    fn after_tools_finish_the_last_line_of_agent_text_shows() {
        assert_eq!(
            feed(&[
                tool("t1", "cargo build"),
                text("Build passed.\nUpdated the "),
                text("docs\n\n"),
                tool_done("t1"),
            ]),
            Some("Updated the docs".to_string())
        );
    }

    #[test]
    fn a_long_streamed_line_shows_its_newest_text() {
        let long = format!("{}the newest words", "older text ".repeat(20));
        let preview = feed(&[text(&long)]).unwrap_or_default();
        assert!(
            preview.starts_with('…') && preview.ends_with("the newest words"),
            "{preview}"
        );
        assert_eq!(preview.chars().count(), 100);
    }

    #[test]
    fn a_lost_agent_offers_a_resume() {
        assert_eq!(
            feed(&[
                text("half way"),
                SessionEvent::Nori(NoriEvent::SessionEnded(nori_protocol::SessionEnded {
                    reason: SessionEndReason::ConnectionLost,
                    message: None,
                })),
            ]),
            Some("agent exited · resume available".to_string())
        );
    }

    #[test]
    fn a_new_turn_starts_a_fresh_preview_and_the_title_follows_the_summary() {
        let mut previews = SessionPreviews::default();
        previews.observe(1, &text("old answer"));
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
        assert_eq!(
            (previews.preview(1), previews.title(1)),
            (None, Some("Fix the login test".to_string()))
        );
    }

    fn user_text(chunk: &str) -> SessionEvent {
        update(acp::SessionUpdate::UserMessageChunk(
            acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(chunk))),
        ))
    }

    #[test]
    fn text_after_an_answered_request_starts_a_new_line() {
        assert_eq!(
            feed(&[
                text("Test message 1"),
                permission(1, "echo hi"),
                resolved(1),
                text("Permission granted"),
            ]),
            Some("Permission granted".to_string())
        );
    }

    #[test]
    fn text_after_a_tool_starts_a_new_line() {
        assert_eq!(
            feed(&[
                text("Looking"),
                tool("t", "ls"),
                tool_done("t"),
                text("Done")
            ]),
            Some("Done".to_string())
        );
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
    fn a_session_closed_after_its_work_keeps_its_last_answer_as_preview() {
        assert_eq!(
            feed(&[
                text("All tests pass."),
                SessionEvent::Nori(NoriEvent::SessionEnded(nori_protocol::SessionEnded {
                    reason: SessionEndReason::Closed,
                    message: None,
                })),
            ]),
            Some("All tests pass.".to_string())
        );
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
}
