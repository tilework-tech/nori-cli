//! Correlation of managed requests with UI actions; notification ordering stays intact.

use std::collections::HashMap;

use serde_json::Value;
use tokio::sync::mpsc;

use super::state::AttachState;
use super::state::CancelDelivery;
use crate::host_client::ConnectionEvent;
use crate::host_client::HostCall;
use crate::host_client::HostEvent;
use crate::host_client::HostSessionClient;

pub(super) enum Pending {
    Initialize,
    List,
    Subscribe {
        session_id: String,
    },
    New,
    TakeControl,
    CancelObserved {
        session_id: String,
        observed_turn_id: String,
    },
    Prompt {
        session_id: String,
        draft: String,
    },
    Mutation {
        draft: Option<String>,
    },
    FollowUp {
        session_id: String,
        message_id: String,
        draft: String,
    },
}

pub(super) struct Runtime {
    pub state: AttachState,
    pub composer: crate::ComposerInput,
    pub(super) client: HostSessionClient<Pending>,
    uncertain_delivery: bool,
    pub session_list_binding: nori_config::HotkeyBinding,
    pub next_waiting_binding: nori_config::HotkeyBinding,
    pub(super) next_waiting_after_refresh: bool,
    /// The unsent edit of each session not on screen, restored when it
    /// returns.
    drafts: HashMap<String, crate::bottom_pane::ComposerDraft>,
    /// Holds back option keys just after a request comes on screen.
    pub(super) answer_guard: crate::answer_guard::AnswerGuard,
    /// The request last seen on screen, to notice when another one appears.
    request_on_screen: Option<String>,
    /// The attaching terminal's directory: where `/new` starts a session
    /// and where its relative folders begin.
    pub(super) cwd: std::path::PathBuf,
    /// Whether the host starts sessions in a requested folder.
    pub(super) session_folders: bool,
    /// Whether the host was named with `--host` and may be another machine.
    pub(super) remote: bool,
}

impl Runtime {
    pub fn set_next_waiting_binding(&mut self, binding: nori_config::HotkeyBinding) {
        self.state.jump_binding = binding.clone();
        self.next_waiting_binding = binding;
        self.composer.set_hint_items(vec![
            (self.session_list_binding.as_str(), "sessions"),
            (self.next_waiting_binding.as_str(), "next waiting"),
            ("alt+t", "take control"),
            ("ctrl+d", "detach"),
            ("/delegate", "delegate task"),
        ]);
    }

    pub fn set_session_list_binding(&mut self, binding: nori_config::HotkeyBinding) {
        self.session_list_binding = binding;
        self.composer.set_hint_items(vec![
            (self.session_list_binding.as_str(), "sessions"),
            ("alt+t", "take control"),
            ("ctrl+d", "detach"),
            ("/new", "new session"),
        ]);
    }

    pub fn new(selected: Option<String>, requests: mpsc::UnboundedSender<Value>) -> Self {
        Self::with_clock(selected, requests, crate::answer_guard::Clock::system())
    }

    /// A runtime whose request grace periods read `clock`.
    pub fn with_clock(
        selected: Option<String>,
        requests: mpsc::UnboundedSender<Value>,
        clock: crate::answer_guard::Clock,
    ) -> Self {
        let mut composer = crate::ComposerInput::for_attachment();
        composer.set_hint_items(vec![
            ("alt+s", "sessions"),
            ("alt+t", "take control"),
            ("ctrl+d", "detach"),
            ("/new", "new session"),
        ]);
        Self {
            state: AttachState {
                list_open: selected.is_none(),
                selected,
                ..Default::default()
            },
            composer,
            client: HostSessionClient::new(requests),
            uncertain_delivery: false,
            session_list_binding: nori_config::HotkeyConfig::default().open_session_list,
            next_waiting_binding: nori_config::HotkeyConfig::default().next_waiting_session,
            next_waiting_after_refresh: false,
            drafts: HashMap::new(),
            answer_guard: crate::answer_guard::AnswerGuard::new(clock),
            request_on_screen: None,
            cwd: std::env::current_dir().unwrap_or_default(),
            session_folders: false,
            remote: false,
        }
    }

    /// Starts the answer grace period when a request other than the last one
    /// seen is now on screen.
    pub(super) fn note_request_on_screen(&mut self) {
        let shown = self.state.request_on_screen_id();
        if shown != self.request_on_screen {
            if shown.is_some() {
                self.answer_guard.shown();
            }
            self.request_on_screen = shown;
        }
    }

    /// Put `id` on screen. The edit in the composer stays with the session
    /// it was typed in, and `id`'s own edit comes back. Text typed while no
    /// session was open goes with the session opened next.
    pub(super) fn open_session(&mut self, id: String) {
        if let Some(previous) = self.state.selected.take()
            && previous != id
        {
            self.drafts.insert(previous, self.composer.take_draft());
            if let Some(draft) = self.drafts.remove(&id) {
                self.composer.restore_draft(draft);
            }
        }
        self.state.selected = Some(id);
        self.state.list_open = false;
        self.state.scroll = 0;
        self.state.notice = None;
        self.state.replay_error = None;
        self.subscribe_selected();
        self.note_request_on_screen();
    }

    pub fn send(&mut self, call: HostCall, pending: Pending) {
        if self.state.stopped.is_some() {
            return;
        }
        let awaiting_cancel = match &pending {
            Pending::CancelObserved { session_id, .. } => Some(session_id.clone()),
            Pending::Initialize
            | Pending::List
            | Pending::Subscribe { .. }
            | Pending::New
            | Pending::TakeControl
            | Pending::Prompt { .. }
            | Pending::Mutation { .. }
            | Pending::FollowUp { .. } => None,
        };
        if self.client.request(call, pending).is_err() {
            self.state
                .show_notice("Host connection worker stopped. Detach and try again.");
            return;
        }
        if let Some(session_id) = awaiting_cancel
            && let Some(conversation) = self.state.conversations.get_mut(&session_id)
        {
            conversation.cancel_delivery = CancelDelivery::AwaitingHost;
        }
    }

    /// Send a JSON-RPC notification, which has no reply.
    pub fn notify(&mut self, call: HostCall) {
        if self.state.stopped.is_some() {
            return;
        }
        if self.client.notify(call).is_err() {
            self.state
                .show_notice("Host connection worker stopped. Detach and try again.");
        }
    }

    /// `session/resume` for `id` in its folder; a host too old to report
    /// the folder gets its own resume request instead.
    pub fn resume(&mut self, id: &str) {
        let cwd = self
            .state
            .sessions
            .iter()
            .find(|row| row.session_id == id)
            .and_then(|row| row.cwd.clone());
        self.send(
            HostCall::Resume {
                session_id: id.to_owned(),
                cwd,
            },
            Pending::Mutation { draft: None },
        );
    }

    pub fn refresh_list(&mut self) {
        if self.state.connected
            && !self
                .client
                .has_pending(|pending| matches!(pending, Pending::List))
        {
            self.send(HostCall::List, Pending::List);
        }
    }

    pub fn subscribe_selected(&mut self) {
        if let Some(id) = self.state.selected.clone() {
            let cursor = self.state.cursors.get(&id).copied().unwrap_or(0);
            self.send(
                HostCall::Subscribe {
                    session_id: id.clone(),
                    cursor,
                },
                Pending::Subscribe { session_id: id },
            );
        }
    }

    pub fn connection_event(&mut self, event: ConnectionEvent) -> anyhow::Result<()> {
        match self.client.incoming(event) {
            HostEvent::Stopped(reason) => {
                self.state.connected = false;
                self.next_waiting_after_refresh = false;
                self.state.stopped = Some(reason);
            }
            HostEvent::Connected => {
                self.send(HostCall::Initialize, Pending::Initialize);
            }
            HostEvent::Disconnected { reason, unanswered } => {
                for conversation in self.state.conversations.values_mut() {
                    conversation.submitted_prompt = false;
                    if conversation.cancel_delivery == CancelDelivery::AwaitingHost {
                        conversation.cancel_delivery = CancelDelivery::Unknown;
                    }
                    if let Some(turn) = &mut conversation.external_turn
                        && turn.state == nori_protocol::ExternalTurnState::CancelRequested
                    {
                        turn.state = nori_protocol::ExternalTurnState::Unconfirmed;
                    }
                }
                self.state.connected = false;
                self.next_waiting_after_refresh = false;
                let uncertain = unanswered.iter().any(|pending| {
                    matches!(
                        pending,
                        Pending::New
                            | Pending::Mutation { .. }
                            | Pending::Prompt { .. }
                            | Pending::CancelObserved { .. }
                            | Pending::FollowUp { .. }
                    )
                });
                self.uncertain_delivery |= uncertain;
                for pending in unanswered {
                    if let Pending::FollowUp {
                        session_id,
                        message_id,
                        draft,
                    } = pending
                    {
                        self.follow_up_notice(
                            &session_id,
                            crate::follow_up::delivery_notice(
                                nori_harness::FollowUpStatus::Unknown,
                                &message_id,
                                &draft,
                            ),
                        );
                    }
                }
                self.state.show_notice(if uncertain {
                    "Connection lost. A request may have reached the host; it will not be retried. Check the conversation after reconnecting.".to_string()
                } else {
                    format!("Connection lost. Reconnecting… {reason}")
                });
            }
            event @ (HostEvent::Session(_)
            | HostEvent::RoleChanged(_)
            | HostEvent::Reply { .. }
            | HostEvent::Ignored) => {
                let handled = self.message(event);
                self.note_request_on_screen();
                return handled;
            }
        }
        self.note_request_on_screen();
        Ok(())
    }

    pub(super) fn follow_up_notice(&mut self, session_id: &str, text: String) {
        self.state
            .conversations
            .entry(session_id.to_owned())
            .or_default()
            .parts
            .push(super::state::Part::Notice(text));
    }

    fn message(&mut self, event: HostEvent<Pending>) -> anyhow::Result<()> {
        let (pending, outcome) = match event {
            HostEvent::Session(params) => {
                if self.state.replay_error.is_none()
                    && let Err(error) = self.state.apply(serde_json::from_value(params)?)
                {
                    self.state.replay_error = Some(error.to_string());
                }
                return Ok(());
            }
            HostEvent::RoleChanged(role) => {
                self.state.role_changed(&role);
                self.state.show_notice(if self.state.watching {
                    "Control moved to another terminal. You are now watching; alt+t takes control."
                } else {
                    "You now control the host. Other terminals can keep watching."
                });
                return Ok(());
            }
            HostEvent::Reply { tag, result } => (tag, result),
            HostEvent::Connected
            | HostEvent::Disconnected { .. }
            | HostEvent::Stopped(_)
            | HostEvent::Ignored => return Ok(()),
        };
        let result = match (outcome, pending) {
            (Ok(result), pending) => (result, pending),
            (
                Err(error),
                Pending::CancelObserved {
                    session_id,
                    observed_turn_id,
                },
            ) => {
                let reason = error
                    .message
                    .unwrap_or_else(|| "Host rejected the request".into());
                self.cancel_observed_reply(&session_id, &observed_turn_id, Err(reason));
                return Ok(());
            }
            (Err(error), pending) => {
                let text = error
                    .message
                    .unwrap_or_else(|| "Host request failed".into());
                self.reply_failed(pending, text);
                return Ok(());
            }
        };
        let (result, pending) = result;
        let result = &result;
        match pending {
            Pending::CancelObserved {
                session_id,
                observed_turn_id,
            } => self.cancel_observed_reply(&session_id, &observed_turn_id, Ok(result)),
            Pending::Initialize => {
                self.session_folders =
                    result["capabilities"]["nori_sessions"]["session_cwd"] == true;
                self.state
                    .role_changed(result["control"]["role"].as_str().unwrap_or("watcher"));
                self.state.connected = true;
                self.state.notice = self.uncertain_delivery.then(|| "Connection restored. A previous request may have reached the host and was not retried; check the conversation before submitting again.".into());
                self.refresh_list();
                self.subscribe_selected();
            }
            Pending::List => {
                self.state.replace_sessions(result.clone())?;
                if std::mem::take(&mut self.next_waiting_after_refresh) && self.composer.is_empty()
                {
                    self.next_waiting_session(false);
                }
                if let Some(id) = &self.state.selected
                    && !self.state.sessions.iter().any(|row| &row.session_id == id)
                {
                    self.state.notice = Some(format!(
                        "Session {id} is not on this host. Choose one from the list."
                    ));
                    self.state.list_open = true;
                }
            }
            Pending::New => {
                let id = result["session_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Host did not return the new session ID"))?;
                self.open_session(id.into());
                self.refresh_list();
            }
            Pending::TakeControl => {
                self.state
                    .role_changed(result["role"].as_str().unwrap_or("watcher"));
            }
            Pending::FollowUp {
                session_id,
                message_id,
                draft,
            } => {
                let status =
                    serde_json::from_value::<nori_harness::FollowUpReceipt>(result.clone())
                        .ok()
                        .filter(|receipt| receipt.message_id == message_id)
                        .map_or(nori_harness::FollowUpStatus::Unknown, |receipt| {
                            receipt.status
                        });
                if status == nori_harness::FollowUpStatus::Rejected
                    && self.state.selected.as_ref() == Some(&session_id)
                    && self.composer.is_empty()
                {
                    self.composer.set_text(draft.clone());
                }
                self.follow_up_notice(
                    &session_id,
                    crate::follow_up::delivery_notice(status, &message_id, &draft),
                );
            }
            Pending::Subscribe { .. } | Pending::Mutation { .. } | Pending::Prompt { .. } => {
                self.refresh_list()
            }
        }
        Ok(())
    }
}
