//! Source-first reduction of host envelopes into presentation owned by this terminal.

use std::collections::HashMap;

use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEnvelope;
use nori_protocol::SessionEvent;
use nori_protocol::SessionStatus;
use nori_protocol::acp::v1 as acp;
use nori_tui_components::PickerItem;
use nori_tui_components::PickerState;
use nori_tui_components::session_table;
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
    pub parent: Option<String>,
    /// `session` for a delegated child, `fork` or `user` otherwise.
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub delegations: Vec<nori_protocol::Delegation>,
    /// The status of the whole family under the top-level session: waiting
    /// on you if any member is. Older hosts omit it.
    #[serde(default)]
    pub family_status: Option<SessionStatus>,
    /// The start of the agent's latest reply. Older hosts omit it, and so do
    /// the rest below.
    #[serde(default)]
    pub reply_preview: Option<String>,
    /// The model's display name.
    #[serde(default)]
    pub model: Option<String>,
    /// Seconds since the session last did anything.
    #[serde(default)]
    pub idle_seconds: Option<i64>,
    /// Tokens in context and the context window, from the agent's last usage
    /// report.
    #[serde(default)]
    pub context_used: Option<i64>,
    #[serde(default)]
    pub context_size: Option<i64>,
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
    /// Where the session ended, with the host's message when it sent one.
    Ended(Option<String>),
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
    /// Key that opens the next session waiting on the user.
    pub jump_binding: nori_config::HotkeyBinding,
}

impl Default for AttachState {
    fn default() -> Self {
        Self {
            cursors: HashMap::new(),
            conversations: HashMap::new(),
            sessions: Vec::new(),
            picker: PickerState::new(
                "Sessions",
                nori_tui_components::session_table::session_columns(),
                Vec::new(),
            )
            .search_placeholder("Title or agent")
            .tree_column(session_table::SESSION),
            watching: true,
            connected: false,
            stopped: None,
            selected: None,
            list_open: true,
            notice: None,
            replay_error: None,
            scroll: 0,
            parent_binding: nori_config::HotkeyConfig::default().parent_session,
            jump_binding: nori_config::HotkeyConfig::default().next_waiting_session,
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
                let title = row
                    .title
                    .clone()
                    .unwrap_or_else(|| nori_protocol::UNTITLED_SESSION_NAME.into());
                // A top-level session shows the status of the family member
                // that most needs the user, as in the local list.
                let status = row
                    .family_status
                    .filter(|_| depth == 0)
                    .unwrap_or(row.status);
                // Older hosts send neither field; only a host that reports
                // activity can say there is no reply yet.
                let reply = row
                    .reply_preview
                    .as_deref()
                    .unwrap_or(if row.idle_seconds.is_some() {
                        session_table::NO_REPLY
                    } else {
                        ""
                    });
                let mut item = PickerItem::new(
                    row.session_id.clone(),
                    session_table::SESSION,
                    format!("{} {title}", crate::session_status::status_glyph(status)),
                )
                .cell(session_table::AGENT, row.agent.clone())
                .cell(
                    session_table::STATUS,
                    crate::session_status::status_label(status),
                )
                .cell_detail_tone(
                    session_table::STATUS,
                    crate::session_status::status_tone(status),
                )
                // The reply lines up with the title, after the status glyph.
                .second_row_cell(session_table::SESSION, format!("  {reply}"))
                .depth(depth)
                .current(self.selected.as_ref() == Some(&row.session_id))
                .search_text(format!(
                    "{} {} {}",
                    row.title.as_deref().unwrap_or_default(),
                    row.agent,
                    row.session_id
                ));
                if let Some(model) = &row.model {
                    item = item.second_row_cell(session_table::AGENT, model.clone());
                }
                if let Some(idle) = row.idle_seconds {
                    item = item.second_row_cell(session_table::STATUS, session_table::ago(idle));
                }
                if let Some(context) = row
                    .context_used
                    .zip(row.context_size)
                    .and_then(|(used, size)| session_table::context_from_usage(used, size))
                {
                    item = item.cell(session_table::CONTEXT, context);
                }
                item
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
                        Part::Ended(message) => self.end_notice(id, message.as_deref()),
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

    /// Whether the session on screen shows a permission request or a
    /// delegation waiting for approval.
    pub fn request_on_screen(&self) -> bool {
        self.selected
            .as_ref()
            .and_then(|id| self.conversations.get(id))
            .is_some_and(|conversation| {
                !conversation.permissions.is_empty()
                    || conversation.delegations.iter().any(|delegation| {
                        delegation.status == nori_protocol::DelegationStatus::AwaitingApproval
                    })
            })
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

    /// The line that ends session `id`'s history. A delegated child points
    /// back to its parent and says whether it finished its task; the words
    /// follow the session's current status, which the host may report after
    /// the end itself.
    pub fn end_notice(&self, id: &str, message: Option<&str>) -> String {
        if let Some(message) = message {
            return message.to_owned();
        }
        let row = self.sessions.iter().find(|row| row.session_id == id);
        let Some(row) = row.filter(|row| row.delegating_parent().is_some()) else {
            return "Session ended. Use /new to start another session.".into();
        };
        let ending = if row.status == nori_protocol::SessionStatus::Done {
            "This child finished its task"
        } else {
            "This child session ended"
        };
        if self.parent_binding.is_none() {
            format!("{ending}.")
        } else {
            format!("{ending} · {} to its parent", self.parent_binding.as_str())
        }
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
                conversation.parts.push(Part::Ended(ended.message));
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
