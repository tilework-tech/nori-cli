//! Bounded envelope history and atomic replay/live subscriptions.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::time::Instant;

use nori_acp_host::remote::ManagedSubscription;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEnvelope;
use nori_protocol::SessionEvent;
use nori_protocol::acp::v1 as acp;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplayError {
    #[error("unknown session {0}")]
    UnknownSession(String),
    #[error("cursor {cursor} for session {session_id} is invalid; latest sequence is {latest_seq}")]
    InvalidCursor {
        session_id: String,
        cursor: i64,
        latest_seq: i64,
    },
    #[error(
        "cursor for session {session_id} has expired; first available sequence is {first_available_seq}; list sessions to choose a newer cursor explicitly"
    )]
    Expired {
        session_id: String,
        first_available_seq: i64,
    },
    #[error("cursor session {0} does not match the selected session")]
    WrongSession(String),
}

/// Session metadata retained even when its oldest envelopes expire.
#[derive(Debug, Clone)]
pub struct ReplayInfo {
    pub latest_seq: i64,
    pub first_available_seq: i64,
    pub age_seconds: i64,
    pub title: Option<String>,
    /// The start of the agent's latest reply.
    pub reply_preview: Option<String>,
    /// What the session asks the user for while it waits, such as
    /// `approve: cargo test -p parser`.
    pub waiting_request: Option<String>,
    /// Seconds since the session's latest event.
    pub idle_seconds: i64,
    /// Tokens in context and the context window, from the agent's latest
    /// usage report.
    pub context: Option<(i64, i64)>,
    /// The latest queue snapshot, so a client that joins after its
    /// announcement expired still sees a paused queue.
    pub queue: Option<nori_protocol::QueueSnapshot>,
    /// The latest goal, so a client that joins after its announcement
    /// expired still sees it.
    pub goal: Option<nori_protocol::ThreadGoal>,
}

struct History {
    registered: Instant,
    last_activity: Instant,
    latest_seq: i64,
    evicted_through: i64,
    /// The agent's own title or the prompt summary, whichever came last.
    title: Option<String>,
    /// The first prompt's first line, or the `/delegate` task a session
    /// started with, which titles the session until it has a title.
    first_prompt: Option<String>,
    reply: nori_protocol::ReplyPreview,
    pending: nori_protocol::PendingRequest,
    context: Option<(i64, i64)>,
    queue: Option<nori_protocol::QueueSnapshot>,
    goal: Option<nori_protocol::ThreadGoal>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            registered: Instant::now(),
            last_activity: Instant::now(),
            latest_seq: 0,
            evicted_through: 0,
            title: None,
            first_prompt: None,
            reply: nori_protocol::ReplyPreview::default(),
            pending: nori_protocol::PendingRequest::default(),
            context: None,
            queue: None,
            goal: None,
        }
    }
}

struct Subscriber {
    session: Option<String>,
    tx: mpsc::Sender<SessionEnvelope>,
}

pub(super) struct Journal {
    pub(super) capacity: usize,
    events: VecDeque<SessionEnvelope>,
    sessions: HashMap<String, History>,
    subscribers: Vec<Subscriber>,
}

impl Journal {
    pub(super) fn new(capacity: i32) -> Self {
        Self {
            capacity: usize::try_from(capacity.max(1)).unwrap_or(1),
            events: VecDeque::new(),
            sessions: HashMap::new(),
            subscribers: Vec::new(),
        }
    }

    pub(super) fn register(&mut self, session: String) {
        self.sessions.entry(session).or_default();
    }

    pub(super) fn publish(&mut self, envelope: &SessionEnvelope) {
        let history = self
            .sessions
            .entry(envelope.conversation_id.clone())
            .or_default();
        history.latest_seq = envelope.seq;
        history.last_activity = Instant::now();
        history.reply.observe(&envelope.event);
        history.pending.observe(&envelope.event);
        match &envelope.event {
            SessionEvent::Nori(NoriEvent::QueueChanged(queue)) => {
                history.queue = Some(queue.clone());
            }
            SessionEvent::Nori(NoriEvent::GoalChanged(goal)) => {
                history.goal = goal.clone();
            }
            SessionEvent::Nori(NoriEvent::PromptSummaryUpdated(summary)) => {
                history.title = Some(summary.summary.clone())
            }
            // A session whose first input is `/delegate` is named by that task.
            SessionEvent::Nori(NoriEvent::DelegationChanged(delegation))
                if history.first_prompt.is_none()
                    && delegation.origin == nori_protocol::DelegationOrigin::User
                    && delegation.parent_id == envelope.conversation_id =>
            {
                history.first_prompt = Some(title_line(&delegation.task));
            }
            SessionEvent::Acp(AcpEvent::Notification(
                acp::AgentNotification::SessionNotification(notification),
            )) => match &notification.update {
                acp::SessionUpdate::SessionInfoUpdate(info) => {
                    if let Some(title) = info.title.as_opt_ref() {
                        history.title = title.cloned();
                    }
                }
                acp::SessionUpdate::UsageUpdate(usage) => {
                    history.context = Some((
                        i64::try_from(usage.used).unwrap_or(i64::MAX),
                        i64::try_from(usage.size).unwrap_or(i64::MAX),
                    ));
                }
                acp::SessionUpdate::UserMessageChunk(chunk) if history.first_prompt.is_none() => {
                    if let acp::ContentBlock::Text(text) = &chunk.content
                        && nori_protocol::delegation_result_summary(&text.text).is_none()
                    {
                        history.first_prompt = Some(title_line(&text.text));
                    }
                }
                _ => {}
            },
            _ => {}
        }
        self.events.push_back(envelope.clone());
        while self.events.len() > self.capacity {
            if let Some(evicted) = self.events.pop_front()
                && let Some(history) = self.sessions.get_mut(&evicted.conversation_id)
            {
                history.evicted_through = evicted.seq;
            }
        }
        self.subscribers.retain(|subscriber| {
            if subscriber
                .session
                .as_ref()
                .is_some_and(|id| id != &envelope.conversation_id)
            {
                return !subscriber.tx.is_closed();
            }
            subscriber.tx.try_send(envelope.clone()).is_ok()
        });
    }

    pub(super) fn cursors(&self) -> HashMap<String, i64> {
        self.sessions
            .iter()
            .map(|(id, history)| (id.clone(), history.latest_seq))
            .collect()
    }

    pub(super) fn info(&self, session: &str) -> Option<ReplayInfo> {
        let history = self.sessions.get(session)?;
        Some(ReplayInfo {
            latest_seq: history.latest_seq,
            first_available_seq: history.evicted_through + 1,
            age_seconds: i64::try_from(history.registered.elapsed().as_secs()).unwrap_or(i64::MAX),
            title: history
                .title
                .clone()
                .or_else(|| history.first_prompt.clone()),
            reply_preview: history.reply.text(),
            waiting_request: history.pending.text(),
            idle_seconds: i64::try_from(history.last_activity.elapsed().as_secs())
                .unwrap_or(i64::MAX),
            context: history.context,
            queue: history.queue.clone(),
            goal: history.goal.clone(),
        })
    }

    pub(super) fn subscribe(
        &mut self,
        session: Option<String>,
        capacity: i32,
    ) -> mpsc::Receiver<SessionEnvelope> {
        let (tx, rx) = mpsc::channel(usize::try_from(capacity.max(1)).unwrap_or(1));
        self.subscribers.push(Subscriber { session, tx });
        rx
    }

    pub(super) fn subscribe_from(
        &mut self,
        session: Option<String>,
        cursors: &HashMap<String, i64>,
        capacity: i32,
    ) -> Result<ManagedSubscription, ReplayError> {
        if let Some(id) = &session
            && !self.sessions.contains_key(id)
        {
            return Err(ReplayError::UnknownSession(id.clone()));
        }
        for (id, cursor) in cursors {
            let history = self
                .sessions
                .get(id)
                .ok_or_else(|| ReplayError::UnknownSession(id.clone()))?;
            if session.as_ref().is_some_and(|selected| selected != id) {
                return Err(ReplayError::WrongSession(id.clone()));
            }
            if *cursor < 0 || *cursor > history.latest_seq {
                return Err(ReplayError::InvalidCursor {
                    session_id: id.clone(),
                    cursor: *cursor,
                    latest_seq: history.latest_seq,
                });
            }
        }
        for (id, history) in &self.sessions {
            if session.as_ref().is_some_and(|selected| selected != id) {
                continue;
            }
            if cursors.get(id).copied().unwrap_or(0) < history.evicted_through {
                return Err(ReplayError::Expired {
                    session_id: id.clone(),
                    first_available_seq: history.evicted_through + 1,
                });
            }
        }
        let replay = self
            .events
            .iter()
            .filter(|event| {
                session
                    .as_ref()
                    .is_none_or(|id| *id == event.conversation_id)
                    && event.seq > cursors.get(&event.conversation_id).copied().unwrap_or(0)
            })
            .cloned()
            .collect();
        let events = self.subscribe(session, capacity);
        Ok(ManagedSubscription { replay, events })
    }
}

/// The title a session gets from text the user gave it: its first line.
fn title_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(120)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn envelope(seq: i64, update: acp::SessionUpdate) -> SessionEnvelope {
        SessionEnvelope {
            conversation_id: "s".into(),
            agent: "mock-model".into(),
            seq,
            event: SessionEvent::Acp(AcpEvent::Notification(
                acp::AgentNotification::SessionNotification(acp::SessionNotification::new(
                    acp::SessionId::new("s"),
                    update,
                )),
            )),
        }
    }

    fn agent_title(title: Option<&str>) -> acp::SessionUpdate {
        acp::SessionUpdate::SessionInfoUpdate(
            acp::SessionInfoUpdate::new().title(title.map(str::to_owned)),
        )
    }

    #[test]
    fn a_cleared_agent_title_falls_back_to_the_first_prompt() {
        let mut journal = Journal::new(16);
        journal.publish(&envelope(
            1,
            acp::SessionUpdate::UserMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new("say hi in 3 words"),
            ))),
        ));
        journal.publish(&envelope(2, agent_title(Some("Greeting in three words"))));
        let renamed = journal.info("s").and_then(|info| info.title);
        journal.publish(&envelope(3, agent_title(None)));
        assert_eq!(
            (renamed, journal.info("s").and_then(|info| info.title)),
            (
                Some("Greeting in three words".to_string()),
                Some("say hi in 3 words".to_string())
            )
        );
    }
}
