use std::collections::HashSet;

use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use nori_protocol::SessionPhase;

use super::*;

/// Deterministic projection shared by live sessions and transcript readers.
#[derive(Default)]
pub(crate) struct ContextTimelineState {
    pub(crate) timeline: ContextTimeline,
    session_id: Option<acp::SessionId>,
    replaying: bool,
    seen: HashSet<String>,
}

impl ContextTimelineState {
    pub(crate) fn with_session(session_id: Option<String>) -> Self {
        Self {
            session_id: session_id.map(Into::into),
            ..Self::default()
        }
    }

    pub(crate) fn observe(&mut self, event: &SessionEvent, id: &str, at: &str) {
        if !is_timeline_evidence(event) || !self.seen.insert(id.to_string()) {
            return;
        }
        match event {
            SessionEvent::Nori(NoriEvent::SessionStarted(started)) => {
                if self.active().is_some() {
                    self.timeline.incomplete_history = true;
                    let mut checkpoint = self.checkpoint(at);
                    checkpoint.observation = None;
                    if let Some(turn) = self.timeline.turns.last_mut() {
                        turn.end = Some(checkpoint);
                    }
                }
                self.replaying = false;
                self.replace_session(&started.acp_session_id);
            }
            SessionEvent::Nori(NoriEvent::SessionForked(forked)) => {
                self.session_id = Some(forked.new_acp_session_id.clone());
            }
            SessionEvent::Nori(NoriEvent::ReplayStarted(_)) => {
                self.replaying = true;
                if self.timeline.turns.is_empty() {
                    self.timeline.incomplete_history = true;
                }
            }
            SessionEvent::Nori(NoriEvent::ReplayFinished) => self.replaying = false,
            _ if self.replaying => {}
            SessionEvent::Nori(NoriEvent::SessionPhaseChanged(SessionPhase::Prompting {
                request_id,
            })) => {
                if self
                    .active()
                    .is_some_and(|turn| &turn.request_id == request_id)
                {
                    return;
                }
                if self.active().is_some() {
                    self.timeline.incomplete_history = true;
                    self.finish(at);
                }
                self.timeline.turns.push(ContextTurn {
                    number: i64::try_from(self.timeline.turns.len())
                        .unwrap_or(i64::MAX)
                        .saturating_add(1),
                    request_id: request_id.clone(),
                    start: self.checkpoint(at),
                    end: None,
                });
            }
            SessionEvent::Acp(AcpEvent::Notification(
                acp::AgentNotification::SessionNotification(notification),
            )) => {
                if self
                    .session_id
                    .as_ref()
                    .is_some_and(|session| session != &notification.session_id)
                {
                    return;
                }
                if let acp::SessionUpdate::UsageUpdate(usage) = &notification.update {
                    self.session_id = Some(notification.session_id.clone());
                    self.observe_usage(usage, id, at);
                }
            }
            SessionEvent::Acp(AcpEvent::Response {
                response: Ok(acp::AgentResponse::NewSessionResponse(response)),
                ..
            }) => self.replace_session(&response.session_id),
            SessionEvent::Acp(AcpEvent::Response {
                response: Ok(acp::AgentResponse::ForkSessionResponse(response)),
                ..
            }) => self.session_id = Some(response.session_id.clone()),
            SessionEvent::Acp(AcpEvent::Response {
                request_id,
                response,
            }) => {
                if matches!(
                    response,
                    Ok(acp::AgentResponse::PromptResponse(_)
                        | acp::AgentResponse::NewSessionResponse(_)
                        | acp::AgentResponse::ForkSessionResponse(_))
                        | Err(_)
                ) && self
                    .active()
                    .is_some_and(|turn| &turn.request_id == request_id)
                {
                    self.finish(at);
                } else if matches!(response, Ok(acp::AgentResponse::PromptResponse(_)))
                    && !self
                        .timeline
                        .turns
                        .iter()
                        .any(|turn| &turn.request_id == request_id)
                {
                    self.timeline.incomplete_history = true;
                }
            }
            SessionEvent::Nori(NoriEvent::RequestFailed(failure)) => {
                if self.active().is_some_and(|turn| {
                    failure
                        .request_id
                        .as_ref()
                        .is_none_or(|id| id == &turn.request_id)
                }) {
                    self.finish(at);
                }
            }
            SessionEvent::Nori(NoriEvent::SessionEnded(_)) => self.finish(at),
            SessionEvent::Nori(NoriEvent::ContextCompacted(_)) => {
                self.timeline.epoch = self.timeline.epoch.saturating_add(1);
                self.timeline.current = None;
                self.timeline.segments.clear();
            }
            _ => {}
        }
    }

    fn replace_session(&mut self, session_id: &acp::SessionId) {
        if self
            .session_id
            .as_ref()
            .is_some_and(|old| old != session_id)
            && self.timeline.current.is_some()
        {
            self.timeline.epoch = self.timeline.epoch.saturating_add(1);
            self.timeline.current = None;
            self.timeline.segments.clear();
        }
        self.session_id = Some(session_id.clone());
    }

    fn active(&self) -> Option<&ContextTurn> {
        self.timeline.turns.last().filter(|turn| turn.end.is_none())
    }

    fn checkpoint(&self, at: &str) -> ContextCheckpoint {
        ContextCheckpoint {
            recorded_at: at.to_string(),
            observation: self.timeline.current.clone(),
            epoch: self.timeline.epoch,
        }
    }

    fn finish(&mut self, at: &str) {
        let checkpoint = self.checkpoint(at);
        if let Some(turn) = self
            .timeline
            .turns
            .last_mut()
            .filter(|turn| turn.end.is_none())
        {
            turn.end = Some(checkpoint);
        }
    }

    fn observe_usage(&mut self, usage: &acp::UsageUpdate, id: &str, at: &str) {
        let old = self
            .timeline
            .current
            .as_ref()
            .map(|observation| &observation.usage);
        let reset = old.is_some_and(|old| old.size != usage.size || old.used > usage.used);
        if reset {
            self.timeline.epoch = self.timeline.epoch.saturating_add(1);
            self.timeline.segments.clear();
        }
        let baseline = old.is_none() || reset;
        let growth = if baseline {
            usage.used
        } else {
            usage.used.saturating_sub(old.map_or(0, |old| old.used))
        };
        let turn = if baseline {
            None
        } else {
            self.active().map(|turn| turn.number)
        };
        let tokens = i64::try_from(growth).unwrap_or(i64::MAX);
        if tokens > 0 {
            if let Some(last) = self
                .timeline
                .segments
                .last_mut()
                .filter(|last| last.turn == turn)
            {
                last.tokens = last.tokens.saturating_add(tokens);
            } else {
                self.timeline.segments.push(ContextSegment { turn, tokens });
            }
        }
        self.timeline.current = Some(ContextObservation {
            id: id.to_string(),
            observed_at: at.to_string(),
            usage: usage.clone(),
        });
    }
}

/// Events required to reconstruct usage and its prompt-boundary associations.
pub(crate) fn is_timeline_evidence(event: &SessionEvent) -> bool {
    match event {
        SessionEvent::Nori(event) => matches!(
            event,
            NoriEvent::SessionStarted(_)
                | NoriEvent::SessionPhaseChanged(_)
                | NoriEvent::SessionEnded(_)
                | NoriEvent::ReplayStarted(_)
                | NoriEvent::ReplayFinished
                | NoriEvent::ContextCompacted(_)
                | NoriEvent::SessionForked(_)
                | NoriEvent::RequestFailed(_)
        ),
        SessionEvent::Acp(AcpEvent::Notification(acp::AgentNotification::SessionNotification(
            notification,
        ))) => matches!(notification.update, acp::SessionUpdate::UsageUpdate(_)),
        SessionEvent::Acp(AcpEvent::Response { response, .. }) => {
            matches!(
                response,
                Ok(acp::AgentResponse::PromptResponse(_)
                    | acp::AgentResponse::NewSessionResponse(_)
                    | acp::AgentResponse::ForkSessionResponse(_))
                    | Err(_)
            )
        }
        _ => false,
    }
}
