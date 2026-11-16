//! Correlation of managed requests with UI actions; notification ordering stays intact.

use std::collections::HashMap;

use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;

use super::connection::ConnectionEvent;
use super::state::AttachState;

pub(super) enum Pending {
    Initialize,
    List,
    Subscribe { session_id: String },
    New,
    TakeControl,
    Mutation { draft: Option<String> },
}

pub(super) struct Runtime {
    pub state: AttachState,
    pub composer: crate::ComposerInput,
    requests: mpsc::UnboundedSender<Value>,
    pending: HashMap<i64, Pending>,
    next_id: i64,
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
            requests,
            pending: HashMap::new(),
            next_id: 0,
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

    pub fn send(&mut self, method: &str, params: Value, pending: Pending) {
        if self.state.stopped.is_some() {
            return;
        }
        self.next_id += 1;
        let id = self.next_id;
        self.pending.insert(id, pending);
        if self
            .requests
            .send(json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}))
            .is_err()
        {
            self.state
                .show_notice("Host connection worker stopped. Detach and try again.");
        }
    }

    pub fn refresh_list(&mut self) {
        if self.state.connected
            && !self
                .pending
                .values()
                .any(|pending| matches!(pending, Pending::List))
        {
            self.send("_nori/session/list", json!({}), Pending::List);
        }
    }

    pub fn subscribe_selected(&mut self) {
        if let Some(id) = self.state.selected.clone() {
            let cursor = self.state.cursors.get(&id).copied().unwrap_or(0);
            self.send(
                "_nori/session/subscribe",
                json!({"session_id":id, "cursors":{id.clone():cursor}}),
                Pending::Subscribe { session_id: id },
            );
        }
    }

    pub fn connection_event(&mut self, event: ConnectionEvent) -> anyhow::Result<()> {
        match event {
            ConnectionEvent::Stopped(reason) => {
                self.state.connected = false;
                self.next_waiting_after_refresh = false;
                self.state.stopped = Some(reason);
                self.pending.clear();
            }
            ConnectionEvent::Connected => {
                self.send("initialize", json!({}), Pending::Initialize);
            }
            ConnectionEvent::Disconnected(reason) => {
                self.state.connected = false;
                self.next_waiting_after_refresh = false;
                let uncertain = self
                    .pending
                    .values()
                    .any(|pending| matches!(pending, Pending::New | Pending::Mutation { .. }));
                self.uncertain_delivery |= uncertain;
                self.pending.clear();
                self.state.show_notice(if uncertain {
                    "Connection lost. A request may have reached the host; it will not be retried. Check the conversation after reconnecting.".to_string()
                } else {
                    format!("Connection lost. Reconnecting… {reason}")
                });
            }
            ConnectionEvent::Message(message) => {
                let handled = self.message(message);
                self.note_request_on_screen();
                return handled;
            }
        }
        self.note_request_on_screen();
        Ok(())
    }

    fn message(&mut self, message: Value) -> anyhow::Result<()> {
        match message["method"].as_str() {
            Some("_nori/session/event") => {
                if self.state.replay_error.is_none()
                    && let Err(error) = self
                        .state
                        .apply(serde_json::from_value(message["params"].clone())?)
                {
                    self.state.replay_error = Some(error.to_string());
                }
                return Ok(());
            }
            Some("_nori/control/changed") => {
                let role = message["params"]["role"].as_str().unwrap_or("watcher");
                self.state.role_changed(role);
                self.state.show_notice(if self.state.watching {
                    "Control moved to another terminal. You are now watching; alt+t takes control."
                } else {
                    "You now control the host. Other terminals can keep watching."
                });
                return Ok(());
            }
            _ => {}
        }
        let Some(id) = message["id"].as_i64() else {
            return Ok(());
        };
        let Some(pending) = self.pending.remove(&id) else {
            return Ok(());
        };
        if let Some(error) = message.get("error") {
            if matches!(pending, Pending::List) {
                self.next_waiting_after_refresh = false;
            }
            let text = error["message"]
                .as_str()
                .unwrap_or("Host request failed")
                .to_string();
            if matches!(pending, Pending::Initialize) {
                self.state.connected = false;
                self.next_waiting_after_refresh = false;
                self.state.stopped = Some(format!(
                    "Host initialization failed: {text}. Re-run nori attach."
                ));
                self.pending.clear();
            }
            if let Pending::Subscribe { session_id } = &pending {
                if self.state.selected.as_ref() != Some(session_id) {
                    return Ok(());
                }
                self.state.replay_error = Some(format!(
                    "Replay unavailable: {text}. Existing conversation retained."
                ));
            }
            if let Pending::Mutation { draft: Some(draft) } = &pending
                && self.composer.is_empty()
            {
                self.composer.handle_paste(draft.clone());
            }
            self.state.show_notice(text);
            return Ok(());
        }
        let result = &message["result"];
        match pending {
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
            Pending::Subscribe { .. } | Pending::Mutation { .. } => self.refresh_list(),
        }
        Ok(())
    }
}
