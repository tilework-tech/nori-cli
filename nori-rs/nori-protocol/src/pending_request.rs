//! What a session waiting on the user is asking for, as session lists show it.

use crate::AcpEvent;
use crate::DelegationStatus;
use crate::NoriEvent;
use crate::SessionEvent;
use crate::SessionPhase;
use crate::acp::v1 as acp;

/// An open request: the ID that answers it and what it asks for.
#[derive(Debug, Clone)]
enum Open {
    Permission(acp::RequestId, String),
    Delegation(String, String),
}

/// Follows one session's events and keeps its open requests: permission
/// requests and delegations waiting for approval, oldest first.
#[derive(Debug, Default, Clone)]
pub struct PendingRequest {
    open: Vec<Open>,
}

impl PendingRequest {
    pub fn observe(&mut self, event: &SessionEvent) {
        match event {
            SessionEvent::Acp(AcpEvent::Request {
                request_id,
                request: acp::AgentRequest::RequestPermissionRequest(request),
            }) => {
                let subject = request
                    .tool_call
                    .fields
                    .content
                    .as_deref()
                    .and_then(|content| {
                        content.iter().find_map(|item| match item {
                            acp::ToolCallContent::Content(content) => match &content.content {
                                acp::ContentBlock::Text(text) => Some(text.text.clone()),
                                _ => None,
                            },
                            _ => None,
                        })
                    })
                    .or_else(|| request.tool_call.fields.title.clone())
                    .unwrap_or_else(|| "a tool call".to_string());
                self.open.push(Open::Permission(
                    request_id.clone(),
                    format!("approve: {}", first_line(&subject)),
                ));
            }
            SessionEvent::Acp(AcpEvent::Response { request_id, .. })
            | SessionEvent::Nori(NoriEvent::RequestResolved(crate::RequestResolved {
                request_id,
                ..
            })) => self
                .open
                .retain(|open| !matches!(open, Open::Permission(id, _) if id == request_id)),
            SessionEvent::Nori(NoriEvent::DelegationChanged(delegation)) => {
                self.open.retain(
                    |open| !matches!(open, Open::Delegation(id, _) if *id == delegation.request_id),
                );
                if delegation.status == DelegationStatus::AwaitingApproval {
                    self.open.push(Open::Delegation(
                        delegation.request_id.clone(),
                        format!(
                            "approve: start {} · {}",
                            delegation.agent,
                            first_line(&delegation.task)
                        ),
                    ));
                }
            }
            // A finished turn leaves no permission open; a delegation waits
            // for its own answer.
            SessionEvent::Nori(NoriEvent::SessionPhaseChanged(SessionPhase::Idle)) => self
                .open
                .retain(|open| matches!(open, Open::Delegation(..))),
            SessionEvent::Nori(NoriEvent::SessionEnded(_)) => self.open.clear(),
            SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
        }
    }

    /// The oldest open request on one line, such as
    /// `approve: cargo test -p parser`, or `None` when nothing is open.
    pub fn text(&self) -> Option<String> {
        self.open.first().map(|open| match open {
            Open::Permission(_, text) | Open::Delegation(_, text) => text.clone(),
        })
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AcpEvent;
    use crate::NoriEvent;
    use crate::SessionPhase;
    use crate::acp::v1 as acp;
    use pretty_assertions::assert_eq;

    fn permission(id: i64, title: &str) -> SessionEvent {
        SessionEvent::Acp(AcpEvent::Request {
            request_id: acp::RequestId::Number(id),
            request: acp::AgentRequest::RequestPermissionRequest(
                acp::RequestPermissionRequest::new(
                    acp::SessionId::new("s"),
                    acp::ToolCallUpdate::new(
                        acp::ToolCallId::new(format!("call-{id}")),
                        acp::ToolCallUpdateFields::new().title(title.to_string()),
                    ),
                    Vec::new(),
                ),
            ),
        })
    }

    fn resolved(id: i64) -> SessionEvent {
        SessionEvent::Nori(NoriEvent::RequestResolved(crate::RequestResolved {
            request_id: acp::RequestId::Number(id),
            answered_by: "client".to_string(),
        }))
    }

    fn delegation(status: &str) -> SessionEvent {
        serde_json::from_value(serde_json::json!({"source":"nori","event":{
        "event_type":"delegation_changed","event":{
            "request_id":"d1","parent_id":"p","child_id":"c","parent_agent":"claude",
            "agent":"codex","task":"Review the parser\nCover every branch.","depth":1,
            "status":status,"result":null
        }}}))
        .unwrap()
    }

    fn after(events: &[SessionEvent]) -> Option<String> {
        let mut pending = PendingRequest::default();
        for event in events {
            pending.observe(event);
        }
        pending.text()
    }

    #[test]
    fn a_permission_request_reads_as_what_to_approve() {
        assert_eq!(
            after(&[permission(1, "cargo test -p parser\nthen more")]),
            Some("approve: cargo test -p parser".to_string())
        );
    }

    #[test]
    fn the_oldest_open_request_is_shown_until_it_is_answered() {
        let both = [permission(1, "first"), permission(2, "second")];
        assert_eq!(after(&both), Some("approve: first".to_string()));
        let mut answered = both.to_vec();
        answered.push(resolved(1));
        assert_eq!(after(&answered), Some("approve: second".to_string()));
        answered.push(resolved(2));
        assert_eq!(after(&answered), None);
    }

    #[test]
    fn a_finished_turn_closes_permissions_but_not_a_waiting_delegation() {
        let idle = SessionEvent::Nori(NoriEvent::SessionPhaseChanged(SessionPhase::Idle));
        assert_eq!(after(&[permission(1, "rm -rf build"), idle.clone()]), None);
        assert_eq!(
            after(&[delegation("awaiting_approval"), idle]),
            Some("approve: start codex · Review the parser".to_string())
        );
    }

    #[test]
    fn an_ended_session_has_nothing_open() {
        let ended = SessionEvent::Nori(NoriEvent::SessionEnded(crate::SessionEnded {
            reason: crate::SessionEndReason::Shutdown,
            message: None,
        }));
        assert_eq!(
            after(&[
                permission(1, "rm -rf build"),
                delegation("awaiting_approval"),
                ended
            ]),
            None
        );
    }

    #[test]
    fn a_delegation_waiting_for_approval_names_the_agent_and_task() {
        assert_eq!(
            after(&[delegation("awaiting_approval")]),
            Some("approve: start codex · Review the parser".to_string())
        );
        assert_eq!(
            after(&[delegation("awaiting_approval"), delegation("running")]),
            None,
            "a decided delegation no longer waits"
        );
    }
}
