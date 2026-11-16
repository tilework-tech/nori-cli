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
}

struct History {
    registered: Instant,
    latest_seq: i64,
    evicted_through: i64,
    title: Option<String>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            registered: Instant::now(),
            latest_seq: 0,
            evicted_through: 0,
            title: None,
        }
    }
}

struct Subscriber {
    session: Option<String>,
    tx: mpsc::Sender<SessionEnvelope>,
}

pub(super) struct Journal {
    capacity: usize,
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
        match &envelope.event {
            SessionEvent::Nori(NoriEvent::PromptSummaryUpdated(summary)) => {
                history.title = Some(summary.summary.clone())
            }
            // A session whose first input is `/delegate` is named by that task.
            SessionEvent::Nori(NoriEvent::DelegationChanged(delegation))
                if history.title.is_none()
                    && delegation.origin == nori_protocol::DelegationOrigin::User
                    && delegation.parent_id == envelope.conversation_id =>
            {
                history.title = Some(title_line(&delegation.task));
            }
            SessionEvent::Acp(AcpEvent::Notification(
                acp::AgentNotification::SessionNotification(notification),
            )) => match &notification.update {
                acp::SessionUpdate::SessionInfoUpdate(info) => {
                    if let Some(title) = info.title.as_opt_ref() {
                        history.title = title.cloned();
                    }
                }
                acp::SessionUpdate::UserMessageChunk(chunk) if history.title.is_none() => {
                    if let acp::ContentBlock::Text(text) = &chunk.content
                        && nori_protocol::delegation_result_summary(&text.text).is_none()
                    {
                        history.title = Some(title_line(&text.text));
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
            title: history.title.clone(),
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
