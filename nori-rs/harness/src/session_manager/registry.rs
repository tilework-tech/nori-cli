//! Process-free state of the session manager.
//!
//! The registry owns every session record, the per-conversation envelope
//! sequence, status derivation, family rollup and the concurrency cap. It
//! never touches an agent process: the manager feeds it each session's events
//! and reads the results back, which keeps every rule here testable with plain
//! [`SessionEvent`] values.

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use nori_config::AskForApproval;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEndReason;
use nori_protocol::SessionEnvelope;
use nori_protocol::SessionEvent;
use nori_protocol::SessionPhase;
use nori_protocol::SessionStatus;
use nori_protocol::acp;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tokio::sync::TryAcquireError;

use super::SessionRecord;
use super::SessionSource;
use super::StartError;
use crate::ConversationId;

/// Identifies one agent process supervised by the manager.
///
/// A process carries one session at a time. A branch-at-head fork moves the
/// process from the parent conversation to the child, so events are fed in
/// by process and the registry decides which conversation they belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AgentProcessId(i64);

/// Settings a session starts with.
#[derive(Debug, Clone)]
pub(crate) struct SessionSettings {
    pub conversation_id: ConversationId,
    pub agent: String,
    pub parent: Option<ConversationId>,
    pub source: SessionSource,
    pub resumed_from: Option<ConversationId>,
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub approval_policy: AskForApproval,
}

/// A reserved slot under the concurrency cap. Dropping it frees the slot.
pub(crate) struct Reservation {
    _permit: OwnedSemaphorePermit,
}

struct Entry {
    settings: SessionSettings,
    order: i64,
    process: Option<AgentProcessId>,
    reservation: Option<Reservation>,
    next_seq: i64,
    started: bool,
    busy: bool,
    pending_requests: BTreeSet<acp::v1::RequestId>,
    ended: Option<SessionStatus>,
    transcript_path: Option<PathBuf>,
    acp_session_id: Option<String>,
}

impl Entry {
    fn new(settings: SessionSettings, order: i64, process: Option<AgentProcessId>) -> Self {
        Self {
            settings,
            order,
            process,
            reservation: None,
            next_seq: 1,
            started: false,
            busy: false,
            pending_requests: BTreeSet::new(),
            ended: None,
            transcript_path: None,
            acp_session_id: None,
        }
    }

    fn status(&self) -> SessionStatus {
        if let Some(ended) = self.ended {
            ended
        } else if !self.started {
            SessionStatus::Starting
        } else if !self.pending_requests.is_empty() {
            SessionStatus::NeedsYou
        } else if self.busy {
            SessionStatus::Working
        } else {
            SessionStatus::Ready
        }
    }

    fn end(&mut self, status: SessionStatus) {
        if self.ended.is_none() {
            self.ended = Some(status);
        }
        self.process = None;
        self.reservation = None;
        self.busy = false;
        self.pending_requests.clear();
    }

    fn record(&self) -> SessionRecord {
        SessionRecord {
            conversation_id: self.settings.conversation_id,
            agent: self.settings.agent.clone(),
            parent: self.settings.parent,
            source: self.settings.source,
            resumed_from: self.settings.resumed_from,
            cwd: self.settings.cwd.clone(),
            model: self.settings.model.clone(),
            approval_policy: self.settings.approval_policy,
            transcript_path: self.transcript_path.clone(),
            acp_session_id: self.acp_session_id.clone(),
            status: self.status(),
        }
    }
}

/// Lower ranks win when a family's statuses are rolled up into one.
fn attention_rank(status: SessionStatus) -> i32 {
    match status {
        SessionStatus::NeedsYou => 0,
        SessionStatus::Failed => 1,
        SessionStatus::Working => 2,
        SessionStatus::Starting => 3,
        SessionStatus::Ready => 4,
        SessionStatus::Closed => 5,
    }
}

fn advertised_model(options: &[acp::v1::SessionConfigOption]) -> Option<String> {
    options.iter().find_map(|option| {
        if option.category != Some(acp::v1::SessionConfigOptionCategory::Model) {
            return None;
        }
        match &option.kind {
            acp::v1::SessionConfigKind::Select(select) => Some(select.current_value.to_string()),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    })
}

pub(crate) struct Registry {
    sessions: HashMap<ConversationId, Entry>,
    processes: HashMap<AgentProcessId, ConversationId>,
    capacity: Arc<Semaphore>,
    limit: i32,
    next_order: i64,
}

impl Registry {
    pub(crate) fn new(max_sessions: i32) -> Self {
        let permits = usize::try_from(max_sessions.max(1)).unwrap_or(1);
        Self {
            sessions: HashMap::new(),
            processes: HashMap::new(),
            capacity: Arc::new(Semaphore::new(permits)),
            limit: max_sessions.max(1),
            next_order: 0,
        }
    }

    /// Reserve a slot before an agent process is started.
    pub(crate) fn reserve(&self) -> Result<Reservation, StartError> {
        match Arc::clone(&self.capacity).try_acquire_owned() {
            Ok(permit) => Ok(Reservation { _permit: permit }),
            Err(TryAcquireError::NoPermits) => {
                Err(StartError::CapacityReached { limit: self.limit })
            }
            Err(TryAcquireError::Closed) => Err(StartError::ShuttingDown),
        }
    }

    /// Refuse every later reservation, for shutdown.
    pub(crate) fn close_admission(&self) {
        self.capacity.close();
    }

    /// Register a session that is about to start on a new agent process.
    pub(crate) fn register(
        &mut self,
        settings: SessionSettings,
        reservation: Reservation,
    ) -> Result<AgentProcessId, StartError> {
        if let Some(parent) = settings.parent
            && !self.sessions.contains_key(&parent)
        {
            return Err(StartError::UnknownParent(parent));
        }
        let order = self.next_order;
        self.next_order += 1;
        let process = AgentProcessId(order);
        let conversation_id = settings.conversation_id;
        let mut entry = Entry::new(settings, order, Some(process));
        entry.reservation = Some(reservation);
        self.sessions.insert(conversation_id, entry);
        self.processes.insert(process, conversation_id);
        Ok(process)
    }

    /// Apply one event from `process` and return it wrapped in its envelope.
    pub(crate) fn observe(
        &mut self,
        process: AgentProcessId,
        event: SessionEvent,
    ) -> Option<SessionEnvelope> {
        let mut conversation_id = *self.processes.get(&process)?;
        if let SessionEvent::Nori(NoriEvent::SessionForked(forked)) = &event
            && let Ok(child) = ConversationId::from_string(&forked.new_conversation_id)
        {
            self.fork(
                process,
                conversation_id,
                child,
                forked.new_acp_session_id.to_string(),
            );
            conversation_id = child;
        }
        let entry = self.sessions.get_mut(&conversation_id)?;
        match &event {
            SessionEvent::Nori(NoriEvent::SessionStarted(started)) => {
                entry.started = true;
                entry.transcript_path = started.transcript_path.clone();
                entry.acp_session_id = Some(started.acp_session_id.to_string());
                if let Some(model) = advertised_model(&started.config_options) {
                    entry.settings.model = Some(model);
                }
            }
            SessionEvent::Nori(NoriEvent::SessionPhaseChanged(phase)) => match phase {
                SessionPhase::Idle => {
                    entry.busy = false;
                    entry.pending_requests.clear();
                }
                SessionPhase::Cancelling { .. } => {
                    entry.busy = true;
                    entry.pending_requests.clear();
                }
                SessionPhase::Loading { .. } | SessionPhase::Prompting { .. } => {
                    entry.busy = true;
                }
            },
            SessionEvent::Nori(NoriEvent::SessionEnded(ended)) => {
                let status = match ended.reason {
                    SessionEndReason::Shutdown | SessionEndReason::Closed => SessionStatus::Closed,
                    SessionEndReason::ConnectionLost
                    | SessionEndReason::SpawnFailed
                    | SessionEndReason::TimedOut => SessionStatus::Failed,
                };
                entry.end(status);
            }
            SessionEvent::Acp(AcpEvent::Request {
                request_id,
                request: acp::v1::AgentRequest::RequestPermissionRequest(_),
            }) => {
                entry.pending_requests.insert(request_id.clone());
            }
            SessionEvent::Acp(AcpEvent::Notification(
                acp::v1::AgentNotification::SessionNotification(notification),
            )) => {
                if let acp::v1::SessionUpdate::ConfigOptionUpdate(update) = &notification.update
                    && let Some(model) = advertised_model(&update.config_options)
                {
                    entry.settings.model = Some(model);
                }
            }
            SessionEvent::Acp(AcpEvent::Request { .. } | AcpEvent::Response { .. })
            | SessionEvent::Acp(AcpEvent::Notification(_))
            | SessionEvent::Nori(_) => {}
        }
        let seq = entry.next_seq;
        entry.next_seq += 1;
        Some(SessionEnvelope {
            conversation_id: conversation_id.to_string(),
            agent: entry.settings.agent.clone(),
            seq,
            event,
        })
    }

    /// Branch at head: the parent is frozen and the agent process, with its
    /// slot, now carries the child conversation.
    fn fork(
        &mut self,
        process: AgentProcessId,
        parent: ConversationId,
        child: ConversationId,
        acp_session_id: String,
    ) {
        let Some(parent_entry) = self.sessions.get_mut(&parent) else {
            return;
        };
        let reservation = parent_entry.reservation.take();
        let transcript_dir = parent_entry
            .transcript_path
            .as_ref()
            .and_then(|path| path.parent().map(std::path::Path::to_path_buf));
        let mut settings = parent_entry.settings.clone();
        parent_entry.end(SessionStatus::Closed);

        settings.conversation_id = child;
        settings.parent = Some(parent);
        settings.source = SessionSource::Fork;
        settings.resumed_from = None;
        let order = self.next_order;
        self.next_order += 1;
        let mut entry = Entry::new(settings, order, Some(process));
        entry.reservation = reservation;
        entry.started = true;
        entry.acp_session_id = Some(acp_session_id);
        entry.transcript_path = transcript_dir.map(|dir| dir.join(format!("{child}.jsonl")));
        self.sessions.insert(child, entry);
        self.processes.insert(process, child);
    }

    /// A client answered an agent request on `process`.
    pub(crate) fn resolve_request(
        &mut self,
        process: AgentProcessId,
        request_id: &acp::v1::RequestId,
    ) {
        if let Some(entry) = self
            .processes
            .get(&process)
            .and_then(|id| self.sessions.get_mut(id))
        {
            entry.pending_requests.remove(request_id);
        }
    }

    /// The event stream of `process` closed. A session that never reported
    /// how it ended has failed.
    pub(crate) fn process_exited(&mut self, process: AgentProcessId) {
        if let Some(entry) = self
            .processes
            .remove(&process)
            .and_then(|id| self.sessions.get_mut(&id))
        {
            entry.end(SessionStatus::Failed);
        }
    }

    /// The live agent process of a session, if it still has one.
    pub(crate) fn live_process(&self, conversation_id: &ConversationId) -> Option<AgentProcessId> {
        self.sessions.get(conversation_id)?.process
    }

    /// The conversation an agent process currently carries.
    pub(crate) fn conversation_of(&self, process: AgentProcessId) -> Option<ConversationId> {
        self.processes.get(&process).copied()
    }

    pub(crate) fn record(&self, conversation_id: &ConversationId) -> Option<SessionRecord> {
        self.sessions.get(conversation_id).map(Entry::record)
    }

    /// Every session in registration order.
    pub(crate) fn records(&self) -> Vec<SessionRecord> {
        let mut entries: Vec<&Entry> = self.sessions.values().collect();
        entries.sort_by_key(|entry| entry.order);
        entries.into_iter().map(Entry::record).collect()
    }

    /// Status of the family that `conversation_id` belongs to: its top-level
    /// session plus every descendant.
    pub(crate) fn family_status(&self, conversation_id: &ConversationId) -> Option<SessionStatus> {
        let mut root = *conversation_id;
        while let Some(parent) = self.sessions.get(&root)?.settings.parent {
            if !self.sessions.contains_key(&parent) {
                break;
            }
            root = parent;
        }
        let mut family = vec![root];
        let mut index = 0;
        while let Some(member) = family.get(index).copied() {
            family.extend(
                self.sessions
                    .values()
                    .filter(|entry| entry.settings.parent == Some(member))
                    .map(|entry| entry.settings.conversation_id),
            );
            index += 1;
        }
        family
            .iter()
            .filter_map(|member| self.sessions.get(member))
            .map(Entry::status)
            .min_by_key(|status| attention_rank(*status))
    }

    /// Replace a failed session with the session that resumes it. Its
    /// children move to the replacement.
    pub(crate) fn supersede(&mut self, failed: &ConversationId, replacement: &ConversationId) {
        if self.sessions.remove(failed).is_none() {
            return;
        }
        for entry in self.sessions.values_mut() {
            if entry.settings.parent == Some(*failed) {
                entry.settings.parent = Some(*replacement);
            }
        }
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
