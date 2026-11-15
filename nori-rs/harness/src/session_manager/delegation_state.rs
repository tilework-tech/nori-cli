//! Delegation state, event attribution and explicit-close traversal.
use super::*;
use nori_protocol::AcpEvent;
use nori_protocol::Delegation;
use nori_protocol::DelegationStatus;
use nori_protocol::SessionPhase;
use nori_protocol::acp;
use std::collections::HashSet;
use std::collections::VecDeque;

pub(super) struct Entry {
    pub(super) value: Delegation,
    pub(super) submitted: bool,
    pub(super) submitting: i32,
    pub(super) pending_turns: HashSet<acp::v1::RequestId>,
    pub(super) completed_turns: HashSet<acp::v1::RequestId>,
    pub(super) output: String,
    /// Something other than message text happened since the last chunk, so
    /// the next chunk starts a new paragraph of the result.
    pub(super) output_break: bool,
    pub(super) last_result: String,
    pub(super) failure: Option<String>,
    pub(super) cancelled: bool,
}

pub(super) struct Delivery {
    pub(super) request: String,
    pub(super) parent: ConversationId,
    pub(super) text: String,
    pub(super) in_flight: bool,
    pub(super) submitted: Option<(AgentProcessId, acp::v1::RequestId)>,
    pub(super) process: Option<AgentProcessId>,
}

#[derive(Default)]
pub(super) struct DelegationState {
    pub(super) entries: Vec<Entry>,
    pub(super) closed: HashSet<ConversationId>,
    pub(super) deliveries: VecDeque<Delivery>,
    pub(super) completed: HashSet<(AgentProcessId, acp::v1::RequestId)>,
}

pub(super) fn active(status: DelegationStatus) -> bool {
    matches!(
        status,
        DelegationStatus::Running | DelegationStatus::AwaitingApproval
    )
}

impl DelegationState {
    pub(super) fn observe(&mut self, process: AgentProcessId, envelope: &SessionEnvelope) {
        if let SessionEvent::Acp(AcpEvent::Response {
            request_id,
            response: Ok(acp::v1::AgentResponse::PromptResponse(_)),
        }) = &envelope.event
        {
            if self.deliveries.iter().any(|delivery| {
                delivery.parent.to_string() == envelope.conversation_id
                    && delivery.in_flight
                    && delivery.submitted.is_none()
            }) {
                self.completed.insert((process, request_id.clone()));
            }
            self.deliveries.retain(|delivery| {
                delivery.submitted.as_ref() != Some(&(process, request_id.clone()))
            });
        }
        if let SessionEvent::Nori(NoriEvent::SessionForked(forked)) = &envelope.event {
            for entry in &mut self.entries {
                if entry.value.child_id == forked.previous_conversation_id {
                    entry.value.child_id.clone_from(&forked.new_conversation_id);
                }
                if entry.value.parent_id == forked.previous_conversation_id {
                    entry
                        .value
                        .parent_id
                        .clone_from(&forked.new_conversation_id);
                }
            }
            if let Ok(child) = ConversationId::from_string(&forked.new_conversation_id) {
                for delivery in &mut self.deliveries {
                    if delivery.parent.to_string() == forked.previous_conversation_id {
                        delivery.parent = child;
                    }
                }
            }
        }
        let Some(entry) = self.entries.iter_mut().find(|e| {
            e.value.child_id == envelope.conversation_id
                && e.value.status == DelegationStatus::Running
        }) else {
            return;
        };
        match &envelope.event {
            SessionEvent::Nori(NoriEvent::SessionPhaseChanged(SessionPhase::Prompting {
                ..
            })) => {
                entry.output.clear();
                entry.output_break = false;
            }
            SessionEvent::Acp(AcpEvent::Notification(
                acp::v1::AgentNotification::SessionNotification(n),
            )) => {
                if let acp::v1::SessionUpdate::AgentMessageChunk(chunk) = &n.update
                    && let acp::v1::ContentBlock::Text(text) = &chunk.content
                {
                    if std::mem::take(&mut entry.output_break) && !entry.output.is_empty() {
                        entry.output.push_str("\n\n");
                    }
                    entry.output.push_str(&text.text);
                } else {
                    entry.output_break = true;
                }
            }
            SessionEvent::Acp(AcpEvent::Request { .. }) => entry.output_break = true,
            SessionEvent::Acp(AcpEvent::Response {
                request_id,
                response,
            }) => {
                if matches!(response, Ok(acp::v1::AgentResponse::PromptResponse(_)))
                    || entry.pending_turns.contains(request_id)
                {
                    if !entry.pending_turns.remove(request_id) {
                        entry.completed_turns.insert(request_id.clone());
                    }
                    entry.last_result.clone_from(&entry.output);
                    if let Ok(acp::v1::AgentResponse::PromptResponse(response)) = response {
                        entry.cancelled = response.stop_reason == acp::v1::StopReason::Cancelled;
                    }
                    if let Err(error) = response {
                        entry.failure = Some(error.to_string());
                    }
                }
            }
            SessionEvent::Nori(NoriEvent::RequestFailed(failure)) => {
                // Only the headline reaches the parent; the stderr tail stays
                // in the child's own history.
                entry.failure = Some(
                    failure
                        .message
                        .lines()
                        .map(str::trim)
                        .find(|line| !line.is_empty())
                        .unwrap_or("Child agent request failed")
                        .to_owned(),
                )
            }
            _ => {}
        }
    }
}

impl State {
    pub(super) fn observe_delegation(
        &mut self,
        process: AgentProcessId,
        envelope: &SessionEnvelope,
    ) {
        self.delegation.observe(process, envelope);
        if let SessionEvent::Nori(NoriEvent::SessionForked(forked)) = &envelope.event {
            let changed: Vec<_> = self
                .delegation
                .entries
                .iter()
                .filter(|entry| {
                    entry.value.parent_id == forked.new_conversation_id
                        || entry.value.child_id == forked.new_conversation_id
                })
                .map(|entry| entry.value.clone())
                .collect();
            for value in changed {
                self.publish_delegation(&value);
            }
        }
    }
    pub(super) fn notify_child_request(&mut self, envelope: &SessionEnvelope) {
        if !matches!(
            &envelope.event,
            SessionEvent::Acp(AcpEvent::Request {
                request: acp::v1::AgentRequest::RequestPermissionRequest(_),
                ..
            })
        ) {
            return;
        }
        let Some(entry) = self
            .delegation
            .entries
            .iter()
            .find(|entry| entry.value.child_id == envelope.conversation_id)
        else {
            return;
        };
        let value = entry.value.clone();
        let Ok(parent) = ConversationId::from_string(&value.parent_id) else {
            return;
        };
        let parent_name = self
            .journal
            .info(&value.parent_id)
            .and_then(|info| info.title)
            .and_then(|title| nori_protocol::session_display_name(&title))
            .unwrap_or_else(|| "this session".to_owned());
        let event = SessionEvent::Nori(NoriEvent::Notice(nori_protocol::Notice {
            message: format!(
                "{} · {} is waiting on you (delegated by {} · {parent_name}). Press Alt+Down to answer it.",
                value.agent,
                nori_protocol::delegation_child_name(&value),
                value.parent_agent,
            ),
        }));
        if let Some(envelope) = self.registry.emit(&parent, event) {
            self.journal.publish(&envelope);
            if let Some(primary) = self.primary.get(&parent) {
                let _ = primary.send(envelope);
            }
        }
    }
    pub(super) fn publish_delegation(&mut self, value: &Delegation) {
        for owner in [&value.parent_id, &value.child_id] {
            if let Ok(id) = ConversationId::from_string(owner)
                && let Some(envelope) = self.registry.emit(
                    &id,
                    SessionEvent::Nori(NoriEvent::DelegationChanged(value.clone())),
                )
            {
                self.journal.publish(&envelope);
                if let Some(primary) = self.primary.get(&id) {
                    let _ = primary.send(envelope);
                }
            }
        }
    }

    pub(super) fn queue_result(&mut self, value: &Delegation) {
        let Ok(parent) = ConversationId::from_string(&value.parent_id) else {
            return;
        };
        if self.delegation.closed.contains(&parent) {
            return;
        }
        self.delegation.deliveries.push_back(Delivery {
            request: value.request_id.clone(),
            parent,
            in_flight: false,
            submitted: None,
            process: None,
            text: nori_protocol::delegation_result_prompt(value),
        });
    }

    pub(super) fn cancel_tree(&mut self, parent: &ConversationId) -> Vec<ConversationId> {
        let mut tree = vec![*parent];
        let mut next = 0;
        while next < tree.len() {
            let member = tree[next];
            for record in self.registry.records() {
                if record.parent == Some(member) && !tree.contains(&record.conversation_id) {
                    tree.push(record.conversation_id);
                }
            }
            next += 1;
        }
        self.delegation.closed.extend(tree.iter().copied());
        self.delegation
            .deliveries
            .retain(|d| !tree.contains(&d.parent));
        let mut changed = Vec::new();
        for entry in &mut self.delegation.entries {
            if active(entry.value.status)
                && tree.iter().any(|id| {
                    entry.value.parent_id == id.to_string()
                        || entry.value.child_id == id.to_string()
                })
            {
                entry.value.status = DelegationStatus::Cancelled;
                entry.value.result = Some("Session closed by the user".into());
                changed.push(entry.value.clone());
            }
        }
        for value in changed {
            self.publish_delegation(&value);
            self.queue_result(&value);
        }
        tree.reverse();
        tree
    }
}
