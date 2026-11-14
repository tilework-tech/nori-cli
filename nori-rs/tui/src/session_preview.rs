//! One-line previews of what each session is doing, for the session list.
//!
//! A preview is derived only from the session's own events, never from a call
//! to the agent. It shows the most urgent thing first: a request waiting on
//! the user, then the running tool, then the latest line of agent text, then
//! how the session ended.

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
            .map(str::to_string)
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
            SessionEvent::Nori(NoriEvent::SessionEnded(ended)) => {
                preview.pending.clear();
                preview.running.clear();
                preview.ended = Some(match ended.reason {
                    SessionEndReason::ConnectionLost => {
                        "agent exited · resume available".to_string()
                    }
                    SessionEndReason::SpawnFailed => "agent failed to start".to_string(),
                    SessionEndReason::TimedOut => "agent timed out · resume available".to_string(),
                    SessionEndReason::Shutdown | SessionEndReason::Closed => "closed".to_string(),
                });
            }
            SessionEvent::Nori(_) => {}
        }
    }

    pub(crate) fn preview(&self, generation: SessionGeneration) -> Option<String> {
        self.sessions.get(&generation).and_then(Preview::line)
    }

    /// The agent's own summary of what the session is about, once it has one.
    pub(crate) fn title(&self, generation: SessionGeneration) -> Option<String> {
        self.sessions
            .get(&generation)
            .and_then(|preview| preview.title.clone())
    }

    /// The request this session is waiting on, if any, as shown in its preview.
    pub(crate) fn pending_request(&self, generation: SessionGeneration) -> Option<String> {
        self.sessions
            .get(&generation)
            .and_then(|preview| preview.pending.first())
            .map(|(_, request)| request.clone())
    }

    pub(crate) fn forget(&mut self, generation: SessionGeneration) {
        self.sessions.remove(&generation);
    }
}

fn observe_update(preview: &mut Preview, update: &acp::SessionUpdate) {
    match update {
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
}
