//! Source-first reduction of host envelopes into presentation owned by this terminal.

use std::collections::HashMap;

use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEnvelope;
use nori_protocol::SessionEvent;
use nori_protocol::SessionStatus;
use nori_protocol::acp::v1 as acp;
use nori_tui_components::PickerColumn;
use nori_tui_components::PickerItem;
use nori_tui_components::PickerState;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

use crate::history_cell::HistoryCell;
use crate::presentation::ClientEvent;
use crate::presentation::ClientEventNormalizer;
use crate::presentation::MessageStream;
use crate::presentation::PlanSnapshot;
use crate::presentation::ToolSnapshot;

pub(super) const READ_ONLY: &str =
    "Read-only: take control with alt+t before sending prompts or permission answers.";

#[derive(Clone, Deserialize)]
pub(super) struct SessionRow {
    pub session_id: String,
    pub agent: String,
    pub status: SessionStatus,
    pub title: Option<String>,
    #[serde(default)]
    pub age_seconds: i64,
    #[serde(default)]
    pub parent: Option<String>,
    /// `session` for a delegated child, `fork` or `user` otherwise.
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub delegations: Vec<nori_protocol::Delegation>,
}

pub(super) enum Part {
    Message {
        stream: MessageStream,
        id: Option<String>,
        text: String,
    },
    Tool(Box<ToolSnapshot>),
    Plan(PlanSnapshot),
    Notice(String),
    Delegation(nori_protocol::Delegation),
}

pub(super) struct Permission {
    pub request_id: acp::RequestId,
    pub title: String,
    pub options: Vec<acp::PermissionOption>,
    pub selected: usize,
}

#[derive(Default)]
pub(super) struct Conversation {
    pub parts: Vec<Part>,
    pub permissions: Vec<Permission>,
    pub delegations: Vec<nori_protocol::Delegation>,
    pub cwd: std::path::PathBuf,
    normalizer: ClientEventNormalizer,
    message_boundary: bool,
}

pub(super) struct AttachState {
    pub cursors: HashMap<String, i64>,
    pub conversations: HashMap<String, Conversation>,
    pub sessions: Vec<SessionRow>,
    pub picker: PickerState<String>,
    pub watching: bool,
    pub connected: bool,
    pub stopped: Option<String>,
    pub selected: Option<String>,
    pub list_open: bool,
    pub notice: Option<String>,
    pub replay_error: Option<String>,
    pub scroll: usize,
    /// Key that moves from a delegated child to its parent.
    pub parent_binding: nori_config::HotkeyBinding,
}

impl Default for AttachState {
    fn default() -> Self {
        Self {
            cursors: HashMap::new(),
            conversations: HashMap::new(),
            sessions: Vec::new(),
            picker: PickerState::new(
                "Sessions",
                [
                    PickerColumn::fixed("id", "ID", 8).hide_below(65),
                    PickerColumn::fixed("agent", "AGENT", 14).hide_below(55),
                    PickerColumn::fixed("status", "STATUS", 16),
                    PickerColumn::fixed("age", "AGE", 5).hide_below(45),
                    PickerColumn::flexible("title", "TITLE"),
                ],
                Vec::new(),
            )
            .search_placeholder("Title or agent")
            .tree_column("title"),
            watching: true,
            connected: false,
            stopped: None,
            selected: None,
            list_open: true,
            notice: None,
            replay_error: None,
            scroll: 0,
            parent_binding: nori_config::HotkeyConfig::default().parent_session,
        }
    }
}

impl AttachState {
    pub fn replace_sessions(&mut self, value: Value) -> anyhow::Result<()> {
        self.sessions = serde_json::from_value(value["sessions"].clone())?;
        for row in &self.sessions {
            if let Some(conversation) = self.conversations.get_mut(&row.session_id) {
                conversation.delegations.clone_from(&row.delegations);
            } else if !row.delegations.is_empty() {
                self.conversations
                    .entry(row.session_id.clone())
                    .or_default()
                    .delegations
                    .clone_from(&row.delegations);
            }
        }
        self.picker.title = format!("Sessions {}", self.sessions.len());
        let links: Vec<(&str, Option<&str>)> = self
            .sessions
            .iter()
            .map(|row| (row.session_id.as_str(), row.parent.as_deref()))
            .collect();
        let order = nori_tui_components::picker::tree_order(&links);
        self.picker
            .replace_items(order.into_iter().map(|(index, depth)| {
                let row = &self.sessions[index];
                let seconds = row.age_seconds.max(0);
                let age = if seconds < 60 {
                    format!("{seconds}s")
                } else if seconds < 3600 {
                    format!("{}m", seconds / 60)
                } else {
                    format!("{}h", seconds / 3600)
                };
                let title = row.title.clone().unwrap_or_else(|| "New session".into());
                PickerItem::new(row.session_id.clone(), "title", title)
                    .cell("id", row.session_id.chars().take(8).collect::<String>())
                    .cell("agent", row.agent.clone())
                    .cell("status", crate::session_status::status_label(row.status))
                    .cell_detail_tone("status", crate::session_status::status_tone(row.status))
                    .cell("age", age)
                    .depth(depth)
                    .current(self.selected.as_ref() == Some(&row.session_id))
                    .search_text(format!(
                        "{} {} {}",
                        row.title.as_deref().unwrap_or_default(),
                        row.agent,
                        row.session_id
                    ))
            }));
        Ok(())
    }

    /// Plain text also distinguishes an empty transcript before its cells have
    /// been laid out at the current terminal width.
    pub fn transcript_text(&self, id: &str) -> String {
        self.conversations
            .get(id)
            .map(|conversation| {
                conversation
                    .parts
                    .iter()
                    .map(|part| match part {
                        Part::Message { text, .. } | Part::Notice(text) => text.clone(),
                        Part::Tool(tool) => tool.title.clone(),
                        Part::Delegation(delegation) => delegation_cell(delegation)
                            .display_lines(u16::MAX)
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join("\n"),
                        Part::Plan(plan) => plan
                            .entries
                            .iter()
                            .map(|step| step.step.clone())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n")
            })
            .unwrap_or_default()
    }

    pub fn permission_count(&self, id: &str) -> usize {
        self.conversations
            .get(id)
            .map_or(0, |conversation| conversation.permissions.len())
    }

    pub fn role_label(&self) -> &'static str {
        if self.watching {
            "WATCHING · read-only · alt+t take control"
        } else {
            "CONTROLLING"
        }
    }

    pub fn role_changed(&mut self, role: &str) {
        self.watching = role != "controller";
    }

    pub fn require_control(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.watching, READ_ONLY);
        Ok(())
    }

    pub fn prompt(&self, id: &str, text: &str) -> anyhow::Result<Value> {
        self.require_control()?;
        Ok(json!({"session_id":id,"prompt":[{"type":"text","text":text}]}))
    }

    pub fn show_notice(&mut self, message: impl Into<String>) {
        if self.stopped.is_some() {
            return;
        }
        let message = message.into();
        self.notice = Some(message.clone());
        if let Some(id) = &self.selected {
            self.conversations
                .entry(id.clone())
                .or_default()
                .parts
                .push(Part::Notice(message));
        }
    }

    pub fn apply(&mut self, envelope: SessionEnvelope) -> anyhow::Result<()> {
        let last = self
            .cursors
            .get(&envelope.conversation_id)
            .copied()
            .unwrap_or(0);
        if envelope.seq <= last {
            return Ok(());
        }
        anyhow::ensure!(
            envelope.seq == last + 1,
            "Replay gap for {}: expected event {}, received {}. Existing conversation retained; detach to investigate.",
            envelope.conversation_id,
            last + 1,
            envelope.seq
        );
        let conversation = self
            .conversations
            .entry(envelope.conversation_id.clone())
            .or_default();
        match envelope.event {
            SessionEvent::Acp(AcpEvent::Notification(
                acp::AgentNotification::SessionNotification(notification),
            )) => {
                for event in conversation
                    .normalizer
                    .push_session_update(&notification.update)
                {
                    conversation.apply_presentation(event);
                }
            }
            SessionEvent::Acp(AcpEvent::Request {
                request_id,
                request: acp::AgentRequest::RequestPermissionRequest(request),
            }) => {
                let title = conversation
                    .normalizer
                    .push_permission_request(&request)
                    .into_iter()
                    .find_map(|event| {
                        if let ClientEvent::ApprovalRequest(approval) = event {
                            Some(approval.title)
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "Permission requested".into());
                conversation.permissions.push(Permission {
                    request_id,
                    title,
                    options: request.options,
                    selected: 0,
                });
                conversation.message_boundary = true;
            }
            SessionEvent::Acp(AcpEvent::Response {
                response: Err(error),
                ..
            }) => {
                conversation.parts.push(Part::Notice(error.message));
                conversation.message_boundary = true;
            }
            SessionEvent::Nori(NoriEvent::DelegationChanged(delegation)) => {
                if delegation.parent_id == envelope.conversation_id {
                    conversation
                        .delegations
                        .retain(|current| current.request_id != delegation.request_id);
                    if delegation.status != nori_protocol::DelegationStatus::AwaitingApproval {
                        conversation
                            .parts
                            .push(Part::Delegation(delegation.clone()));
                    }
                    conversation.delegations.push(delegation);
                    conversation.message_boundary = true;
                }
            }
            SessionEvent::Nori(NoriEvent::SessionStarted(started)) => {
                conversation.cwd = started.cwd
            }
            SessionEvent::Nori(NoriEvent::RequestResolved(resolved)) => {
                conversation
                    .permissions
                    .retain(|permission| permission.request_id != resolved.request_id);
            }
            SessionEvent::Nori(NoriEvent::SessionEnded(ended)) => {
                conversation
                    .parts
                    .push(Part::Notice(ended.message.unwrap_or_else(|| {
                        "Session ended. Use /new to start another session.".into()
                    })));
                conversation.permissions.clear();
                conversation.message_boundary = true;
            }
            SessionEvent::Nori(NoriEvent::Notice(notice)) => {
                conversation.parts.push(Part::Notice(notice.message))
            }
            SessionEvent::Nori(NoriEvent::RequestFailed(failed)) => {
                conversation.parts.push(Part::Notice(failed.message));
                conversation.message_boundary = true;
            }
            SessionEvent::Nori(NoriEvent::SessionPhaseChanged(phase)) => {
                if matches!(
                    phase,
                    nori_protocol::SessionPhase::Idle
                        | nori_protocol::SessionPhase::Cancelling { .. }
                ) {
                    conversation.permissions.clear();
                }
                conversation.message_boundary = true;
            }
            SessionEvent::Nori(NoriEvent::ReplayStarted(_)) => {
                // A restored transcript replaces presentation; transport replay
                // alone does not emit this marker and is deduplicated by seq.
                *conversation = Conversation {
                    cwd: conversation.cwd.clone(),
                    ..Default::default()
                };
            }
            SessionEvent::Nori(NoriEvent::ReplayFinished) => conversation.message_boundary = true,
            // Unprojected metadata consumes its sequence too, preserving the
            // complete wire ordering without inventing presentation events.
            _ => {}
        }
        self.cursors.insert(envelope.conversation_id, envelope.seq);
        Ok(())
    }
}

impl Conversation {
    fn apply_presentation(&mut self, event: ClientEvent) {
        match event {
            ClientEvent::MessageDelta(delta) => {
                if !self.message_boundary
                    && let Some(Part::Message { stream, id, text }) = self.parts.last_mut()
                    && *stream == delta.stream
                    && *id == delta.message_id
                {
                    text.push_str(&delta.delta);
                } else {
                    self.parts.push(Part::Message {
                        stream: delta.stream,
                        id: delta.message_id,
                        text: delta.delta,
                    });
                }
                self.message_boundary = false;
            }
            ClientEvent::ToolSnapshot(snapshot) => {
                if let Some(Part::Tool(previous)) = self.parts.iter_mut().find(
                    |part| matches!(part, Part::Tool(tool) if tool.call_id == snapshot.call_id),
                ) {
                    *previous = Box::new(snapshot);
                } else {
                    self.parts.push(Part::Tool(Box::new(snapshot)));
                }
                self.message_boundary = true;
            }
            ClientEvent::PlanSnapshot(plan) => {
                self.parts.push(Part::Plan(plan));
                self.message_boundary = true;
            }
            _ => {}
        }
    }
}

/// Attached clients have no transcript view or local session list key, so
/// delegation cells show no key hints.
pub(super) fn delegation_cell(
    delegation: &nori_protocol::Delegation,
) -> crate::delegation::DelegationCell {
    crate::delegation::DelegationCell::new(
        delegation.clone(),
        crate::delegation::DelegationHints::default(),
    )
}

impl SessionRow {
    /// The parent of a delegated child; forks and top-level sessions have none.
    pub(super) fn delegating_parent(&self) -> Option<&str> {
        self.parent
            .as_deref()
            .filter(|_| self.source.as_deref() == Some("session"))
    }
}
