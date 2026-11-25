//! Process-free state of the session manager.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use nori_config::AskForApproval;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::RequestResolved;
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

struct PendingPermission {
    id: acp::v1::RequestId,
    claimed: bool,
}

struct Entry {
    settings: SessionSettings,
    order: i64,
    process: Option<AgentProcessId>,
    reservation: Option<Reservation>,
    next_seq: i64,
    started: bool,
    busy: bool,
    /// Agent requests waiting on a client, in arrival order.
    pending_requests: Vec<PendingPermission>,
    delegation_approvals: std::collections::HashSet<String>,
    /// Answers that arrived before the request they answer. A client can
    /// read a request from the harness directly, ahead of this registry.
    early_answers: Vec<RequestResolved>,
    ended: Option<SessionStatus>,
    /// A delegated child whose task finished; closing it reads as done.
    task_done: bool,
    /// The child that took over this session's agent process in a fork.
    forked_into: Option<ConversationId>,
    transcript_path: Option<PathBuf>,
    acp_session_id: Option<String>,
    /// The display name of the model the agent says it runs.
    model_name: Option<String>,
    /// Whether the agent can ask before every edit and command, read from
    /// the options it advertised when the session started.
    ask_available: bool,
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
            pending_requests: Vec::new(),
            delegation_approvals: std::collections::HashSet::new(),
            early_answers: Vec::new(),
            ended: None,
            task_done: false,
            forked_into: None,
            transcript_path: None,
            acp_session_id: None,
            model_name: None,
            ask_available: false,
        }
    }

    fn status(&self) -> SessionStatus {
        if let Some(ended) = self.ended {
            ended
        } else if !self.started {
            SessionStatus::Starting
        } else if !self.pending_requests.is_empty() || !self.delegation_approvals.is_empty() {
            SessionStatus::WaitingOnYou
        } else if self.busy {
            SessionStatus::Working
        } else {
            SessionStatus::Ready
        }
    }

    fn end(&mut self, status: SessionStatus) {
        if self.ended.is_none() {
            self.ended = Some(match status {
                SessionStatus::Closed if self.task_done => SessionStatus::Done,
                other => other,
            });
        }
        self.process = None;
        self.reservation = None;
        self.busy = false;
        self.pending_requests.clear();
        self.early_answers.clear();
    }

    fn record(&self) -> SessionRecord {
        SessionRecord {
            conversation_id: self.settings.conversation_id,
            agent: self.settings.agent.clone(),
            parent: self.settings.parent,
            source: self.settings.source,
            cwd: self.settings.cwd.clone(),
            model: self.settings.model.clone(),
            model_name: self.model_name.clone(),
            approval_policy: self.settings.approval_policy,
            transcript_path: self.transcript_path.clone(),
            acp_session_id: self.acp_session_id.clone(),
            pending_requests: self
                .pending_requests
                .iter()
                .map(|request| request.id.clone())
                .collect(),
            status: self.status(),
        }
    }
}

fn advertised_model(options: &[acp::v1::SessionConfigOption]) -> Option<String> {
    options.iter().find_map(|option| {
        if option.category != Some(acp::v1::SessionConfigOptionCategory::Model) {
            return None;
        }
        let acp::v1::SessionConfigKind::Select(select) = &option.kind else {
            return None;
        };
        Some(select.current_value.to_string())
    })
}

/// The display name of the model option's current value.
fn advertised_model_name(options: &[acp::v1::SessionConfigOption]) -> Option<String> {
    options.iter().find_map(|option| {
        if option.category != Some(acp::v1::SessionConfigOptionCategory::Model) {
            return None;
        }
        let acp::v1::SessionConfigKind::Select(select) = &option.kind else {
            return None;
        };
        let options: Vec<&acp::v1::SessionConfigSelectOption> = match &select.options {
            acp::v1::SessionConfigSelectOptions::Ungrouped(options) => options.iter().collect(),
            acp::v1::SessionConfigSelectOptions::Grouped(groups) => groups
                .iter()
                .flat_map(|group| group.options.iter())
                .collect(),
            _ => Vec::new(),
        };
        Some(
            options
                .into_iter()
                .find(|choice| choice.value == select.current_value)
                .map_or_else(
                    || select.current_value.to_string(),
                    |choice| choice.name.clone(),
                ),
        )
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

    /// Reserve one answer for this occurrence of a permission request.
    pub(super) fn claim_permission(
        &mut self,
        session: &ConversationId,
        id: &acp::v1::RequestId,
    ) -> Option<AgentProcessId> {
        let entry = self.sessions.get_mut(session)?;
        let process = entry.process?;
        let request = entry
            .pending_requests
            .iter_mut()
            .find(|request| &request.id == id && !request.claimed)?;
        request.claimed = true;
        Some(process)
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

    /// Slots left for new sessions.
    pub(crate) fn free_slots(&self) -> i32 {
        i32::try_from(self.capacity.available_permits()).unwrap_or(i32::MAX)
    }

    pub(crate) fn limit(&self) -> i32 {
        self.limit
    }

    /// Refuse every later reservation, for shutdown.
    pub(crate) fn close_admission(&self) {
        self.capacity.close();
    }

    /// Register a new session or restart a failed identity on a new process.
    /// Restart admission and replacement happen in the same registry mutation.
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
        let conversation_id = settings.conversation_id;
        if self
            .sessions
            .get(&conversation_id)
            .is_some_and(|entry| entry.status() != SessionStatus::Failed)
        {
            return Err(StartError::NotFailed(conversation_id));
        }
        let order = self.next_order;
        self.next_order += 1;
        let process = AgentProcessId(order);
        if let Some(entry) = self.sessions.get_mut(&conversation_id) {
            tracing::debug!(resumed_from = ?settings.resumed_from, ?process, "restarting failed session");
            // The retired pump can still reach EOF after the replacement has
            // started. It must no longer address this session's state.
            self.processes.retain(|_, id| *id != conversation_id);
            entry.settings = settings;
            entry.process = Some(process);
            entry.reservation = Some(reservation);
            entry.started = false;
            entry.busy = false;
            entry.pending_requests.clear();
            entry.early_answers.clear();
            entry.ended = None;
        } else {
            let mut entry = Entry::new(settings, order, Some(process));
            entry.reservation = Some(reservation);
            self.sessions.insert(conversation_id, entry);
        }
        self.processes.insert(process, conversation_id);
        Ok(process)
    }

    pub(super) fn import_history(
        &mut self,
        settings: SessionSettings,
        status: SessionStatus,
        acp_session_id: Option<String>,
    ) -> anyhow::Result<()> {
        let id = settings.conversation_id;
        anyhow::ensure!(
            !self.sessions.contains_key(&id),
            "session {id} is already registered"
        );
        let mut ancestor = settings.parent;
        while let Some(parent) = ancestor {
            anyhow::ensure!(
                parent != id,
                "checkpoint has a cycle in its session ancestry"
            );
            ancestor = self
                .sessions
                .get(&parent)
                .and_then(|entry| entry.settings.parent);
        }
        let mut entry = Entry::new(settings, self.next_order, None);
        self.next_order += 1;
        entry.end(if status.is_closed() {
            status
        } else {
            SessionStatus::Failed
        });
        entry.acp_session_id = acp_session_id;
        self.sessions.insert(id, entry);
        Ok(())
    }

    /// Tell clients the session's current mode and the modes it offers.
    pub(crate) fn mode_announcement(
        &mut self,
        conversation: &ConversationId,
    ) -> Option<SessionEnvelope> {
        let entry = self.sessions.get(conversation)?;
        let event =
            crate::permission_mode::announced(entry.settings.approval_policy, entry.ask_available);
        self.emit(conversation, event)
    }

    /// The modes `conversation` offers.
    pub(crate) fn available_modes(
        &self,
        conversation: &ConversationId,
    ) -> Vec<nori_protocol::ApprovalMode> {
        crate::permission_mode::available(
            self.sessions
                .get(conversation)
                .is_some_and(|entry| entry.ask_available),
        )
    }

    /// Number manager-owned events even when the session has no live process.
    pub(crate) fn emit(
        &mut self,
        conversation: &ConversationId,
        event: SessionEvent,
    ) -> Option<SessionEnvelope> {
        let entry = self.sessions.get_mut(conversation)?;
        if let SessionEvent::Nori(NoriEvent::DelegationChanged(delegation)) = &event
            && delegation.parent_id == conversation.to_string()
        {
            if delegation.status == nori_protocol::DelegationStatus::AwaitingApproval {
                entry
                    .delegation_approvals
                    .insert(delegation.request_id.clone());
            } else {
                entry.delegation_approvals.remove(&delegation.request_id);
            }
        }
        let seq = entry.next_seq;
        entry.next_seq += 1;
        Some(SessionEnvelope {
            conversation_id: conversation.to_string(),
            agent: entry.settings.agent.clone(),
            seq,
            event,
        })
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
        if let SessionEvent::Nori(NoriEvent::RequestResolved(resolved)) = &event {
            let Some(position) = entry
                .pending_requests
                .iter()
                .position(|pending| pending.id == resolved.request_id)
            else {
                if !entry
                    .early_answers
                    .iter()
                    .any(|early| early.request_id == resolved.request_id)
                {
                    entry.early_answers.push(resolved.clone());
                }
                return None;
            };
            entry.pending_requests.remove(position);
        }
        match &event {
            SessionEvent::Nori(NoriEvent::SessionStarted(started)) => {
                entry.started = true;
                entry.transcript_path = started.transcript_path.clone();
                entry.acp_session_id = Some(started.acp_session_id.to_string());
                if let Some(model) = advertised_model(&started.config_options) {
                    entry.settings.model = Some(model);
                }
                if let Some(name) = advertised_model_name(&started.config_options) {
                    entry.model_name = Some(name);
                }
            }
            SessionEvent::Nori(NoriEvent::SessionPhaseChanged(phase)) => match phase {
                SessionPhase::Idle => {
                    entry.busy = false;
                    entry.pending_requests.clear();
                    entry.early_answers.clear();
                }
                SessionPhase::Cancelling { .. } => {
                    entry.busy = true;
                    entry.pending_requests.clear();
                    entry.early_answers.clear();
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
                if !entry
                    .pending_requests
                    .iter()
                    .any(|pending| pending.id == *request_id)
                {
                    entry.pending_requests.push(PendingPermission {
                        id: request_id.clone(),
                        claimed: false,
                    });
                }
            }
            SessionEvent::Acp(AcpEvent::Notification(
                acp::v1::AgentNotification::SessionNotification(notification),
            )) => {
                if let acp::v1::SessionUpdate::ConfigOptionUpdate(update) = &notification.update {
                    if let Some(model) = advertised_model(&update.config_options) {
                        entry.settings.model = Some(model);
                    }
                    if let Some(name) = advertised_model_name(&update.config_options) {
                        entry.model_name = Some(name);
                    }
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

    /// Apply one event and return every envelope it produces.
    /// An answer that arrived before its request is announced right after
    /// the request.
    pub(crate) fn observe_all(
        &mut self,
        process: AgentProcessId,
        event: SessionEvent,
    ) -> Vec<SessionEnvelope> {
        let request_id = match &event {
            SessionEvent::Acp(AcpEvent::Request {
                request_id,
                request: acp::v1::AgentRequest::RequestPermissionRequest(_),
            }) => Some(request_id.clone()),
            _ => None,
        };
        // Clients learn the mode before the session's start, so whatever
        // they show for the started session already has it.
        let mut envelopes = Vec::new();
        if let SessionEvent::Nori(NoriEvent::SessionStarted(started)) = &event
            && let Some(id) = self.processes.get(&process).copied()
            && let Some(entry) = self.sessions.get_mut(&id)
        {
            entry.ask_available =
                crate::adapter_modes::offers_ask(&entry.settings.agent, &started.config_options);
            envelopes.extend(self.mode_announcement(&id));
        }
        let forked = matches!(&event, SessionEvent::Nori(NoriEvent::SessionForked(_)));
        envelopes.extend(self.observe(process, event));
        // A fork's child starts with no start event, so it is told its mode
        // right after the fork.
        if forked
            && let Some(child) = envelopes
                .last()
                .and_then(|envelope| ConversationId::from_string(&envelope.conversation_id).ok())
        {
            envelopes.extend(self.mode_announcement(&child));
        }
        let early = request_id.and_then(|request_id| {
            let entry = self
                .processes
                .get(&process)
                .and_then(|id| self.sessions.get_mut(id))?;
            let position = entry
                .early_answers
                .iter()
                .position(|early| early.request_id == request_id)?;
            Some(entry.early_answers.remove(position))
        });
        if let Some(answer) = early {
            envelopes.extend(self.observe(
                process,
                SessionEvent::Nori(NoriEvent::RequestResolved(answer)),
            ));
        }
        envelopes
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
        let ask_available = parent_entry.ask_available;
        parent_entry.end(SessionStatus::Closed);
        parent_entry.forked_into = Some(child);

        settings.conversation_id = child;
        settings.parent = Some(parent);
        settings.source = SessionSource::Fork;
        settings.resumed_from = None;
        let order = self.next_order;
        self.next_order += 1;
        let mut entry = Entry::new(settings, order, Some(process));
        entry.reservation = reservation;
        entry.started = true;
        entry.ask_available = ask_available;
        entry.acp_session_id = Some(acp_session_id);
        entry.transcript_path = transcript_dir.map(|dir| dir.join(format!("{child}.jsonl")));
        self.sessions.insert(child, entry);
        for entry in self.sessions.values_mut() {
            if entry.settings.parent == Some(parent)
                && entry.settings.source == SessionSource::Session
            {
                entry.settings.parent = Some(child);
            }
        }
        self.processes.insert(process, child);
    }

    /// The event stream of `process` closed. A session that never reported
    /// how it ended has failed.
    pub(crate) fn process_exited(&mut self, process: AgentProcessId) {
        if let Some(entry) = self
            .processes
            .remove(&process)
            .and_then(|id| self.sessions.get_mut(&id))
            && entry.process == Some(process)
        {
            entry.end(SessionStatus::Failed);
        }
    }

    /// Record a change made to a session's live approval policy.
    pub(crate) fn set_approval_policy(
        &mut self,
        conversation_id: &ConversationId,
        policy: AskForApproval,
    ) {
        if let Some(entry) = self.sessions.get_mut(conversation_id) {
            entry.settings.approval_policy = policy;
        }
    }

    /// Let a session whose process was stopped to replace its agent start
    /// again under the same ID.
    pub(crate) fn allow_restart(&mut self, conversation_id: &ConversationId) {
        if let Some(entry) = self.sessions.get_mut(conversation_id)
            && entry.process.is_none()
        {
            entry.ended = Some(SessionStatus::Failed);
        }
    }

    /// Mark a delegated child whose task finished, so closing it reads as
    /// done rather than closed.
    pub(crate) fn mark_task_done(&mut self, conversation_id: &ConversationId) {
        if let Some(entry) = self.sessions.get_mut(conversation_id) {
            entry.task_done = true;
        }
    }

    /// The live agent process of a session, if it still has one.
    pub(crate) fn live_process(&self, conversation_id: &ConversationId) -> Option<AgentProcessId> {
        self.sessions.get(conversation_id)?.process
    }

    /// The live agent process of a session, following branch-at-head forks
    /// to the child that took over the parent's process.
    pub(crate) fn process_following_forks(
        &self,
        conversation_id: &ConversationId,
    ) -> Option<AgentProcessId> {
        let mut current = *conversation_id;
        loop {
            let entry = self.sessions.get(&current)?;
            match (entry.process, entry.forked_into) {
                (Some(process), _) => return Some(process),
                (None, Some(child)) => current = child,
                (None, None) => return None,
            }
        }
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
            // The member that most needs the user decides the family status.
            .min_by_key(|status| match status {
                SessionStatus::WaitingOnYou => 0,
                SessionStatus::Failed => 1,
                SessionStatus::Working => 2,
                SessionStatus::Starting => 3,
                SessionStatus::Ready => 4,
                SessionStatus::Closed | SessionStatus::Done => 5,
            })
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
