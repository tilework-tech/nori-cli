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
}

impl Runtime {
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
        }
    }

    pub fn send(&mut self, method: &str, params: Value, pending: Pending) {
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
            ConnectionEvent::Connected => {
                self.send("initialize", json!({}), Pending::Initialize);
            }
            ConnectionEvent::Disconnected(reason) => {
                self.state.connected = false;
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
            ConnectionEvent::Message(message) => self.message(message)?,
        }
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
            let text = error["message"]
                .as_str()
                .unwrap_or("Host request failed")
                .to_string();
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
                self.state
                    .role_changed(result["control"]["role"].as_str().unwrap_or("watcher"));
                self.state.connected = true;
                self.state.notice = self.uncertain_delivery.then(|| "Connection restored. A previous request may have reached the host and was not retried; check the conversation before submitting again.".into());
                self.refresh_list();
                self.subscribe_selected();
            }
            Pending::List => {
                self.state.replace_sessions(result.clone())?;
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
                self.state.selected = Some(id.into());
                self.state.list_open = false;
                self.state.scroll = 0;
                self.state.notice = None;
                self.state.replay_error = None;
                self.subscribe_selected();
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
