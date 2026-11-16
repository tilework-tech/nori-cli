//! Input belongs to this attachment; mutations require the current control role.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use nori_protocol::acp::v1 as acp;
use nori_tui_components::PickerAction;
use nori_tui_components::PickerOutcome;
use serde_json::json;

use super::runtime::Pending;
use super::runtime::Runtime;

impl Runtime {
    /// False means detach. It never requests host or session shutdown.
    pub fn key(&mut self, key: KeyEvent) -> bool {
        self.note_request_on_screen();
        let handled = self.handle_key(key);
        self.note_request_on_screen();
        handled
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.kind == KeyEventKind::Release {
            return true;
        }
        // A later user action supersedes navigation waiting on a list refresh.
        self.next_waiting_after_refresh = false;
        if key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return false;
        }
        // A draft stays with its session, so the jump never loses it. With
        // nothing waiting, a typed draft keeps the key for the composer.
        if crate::nori::hotkey_match::matches_binding(&self.next_waiting_binding, &key)
            && (self.next_waiting_session(self.composer.is_empty()) || self.composer.is_empty())
        {
            return true;
        }
        if crate::nori::hotkey_match::matches_binding(&self.state.parent_binding, &key)
            && let Some(parent) = self.state.selected.as_ref().and_then(|id| {
                self.state
                    .sessions
                    .iter()
                    .find(|row| &row.session_id == id)
                    .and_then(|row| row.delegating_parent().map(str::to_owned))
            })
            && self
                .state
                .sessions
                .iter()
                .any(|row| row.session_id == parent)
        {
            self.open_session(parent);
            return true;
        }
        if crate::nori::hotkey_match::matches_binding(&self.session_list_binding, &key) {
            self.state.list_open = !self.state.list_open;
            self.refresh_list();
            return true;
        }
        if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('t') {
            if self.state.connected {
                self.send("_nori/control/take", json!({}), Pending::TakeControl);
            } else {
                self.state
                    .show_notice("Reconnecting. Take control after the connection is restored.");
            }
            return true;
        }
        if key.code == KeyCode::Esc && !self.state.list_open && self.state.request_on_screen() {
            self.state.notice = None;
            if !self.delegation_key(key) {
                self.permission_key(key);
            }
            return true;
        }
        if key.code == KeyCode::Esc {
            if self.state.list_open && self.state.picker.search_active {
                self.state.picker.handle(PickerAction::ClearQuery);
                self.state.picker.handle(PickerAction::DeactivateSearch);
            } else {
                self.state.list_open = false;
                self.state.notice = None;
            }
            return true;
        }
        if key.code == KeyCode::Left && key.modifiers.is_empty() && self.composer.is_empty() {
            self.state.list_open = true;
            self.refresh_list();
            return true;
        }
        if self.state.list_open
            && key.code == KeyCode::Char('r')
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            if let Err(error) = self.require_mutation() {
                self.state.show_notice(error.to_string());
            } else if let Some(item) = self.state.picker.selected_item() {
                let id = item.key.clone();
                if self.state.sessions.iter().any(|row| {
                    row.session_id == id && row.status == nori_protocol::SessionStatus::Failed
                }) {
                    self.send(
                        "_nori/session/resume",
                        json!({"session_id":id}),
                        Pending::Mutation { draft: None },
                    );
                } else {
                    self.state
                        .show_notice("Only failed sessions can be resumed.");
                }
            }
            return true;
        }
        if self.state.list_open && self.picker_key(key) {
            return true;
        }
        // A request shown for the session on screen holds the keyboard, as
        // the local approval overlay does: its keys answer it, Ctrl+C cancels
        // it, scrolling still works, and the hidden draft is left untouched.
        if !self.state.list_open && self.state.request_on_screen() {
            let passes = matches!(key.code, KeyCode::PageUp | KeyCode::PageDown);
            if !passes {
                // A key typed just as the request appeared was meant for
                // the composer; it is dropped rather than answering.
                if !self.answer_guard.allows(&key) {
                    return true;
                }
                if !self.delegation_key(key) {
                    self.permission_key(key);
                }
                return true;
            }
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.composer.is_empty() {
                self.submit("/cancel".into());
            } else {
                self.composer.clear();
            }
            return true;
        }
        match key.code {
            KeyCode::PageUp => {
                self.state.scroll = self.state.scroll.saturating_add(10);
                return true;
            }
            KeyCode::PageDown => {
                self.state.scroll = self.state.scroll.saturating_sub(10);
                return true;
            }
            _ => {}
        }
        if key.code == KeyCode::Tab
            && let Some((completed, candidates)) =
                super::new_session::complete(&self.composer.text(), &self.cwd)
        {
            self.composer.set_text(completed);
            if !candidates.is_empty() {
                self.state.show_notice(candidates.join("  "));
            }
            return true;
        }
        if let crate::ComposerAction::Submitted(text) = self.composer.input(key) {
            return self.submit(text);
        }
        true
    }

    /// Returns whether a waiting session was brought on screen.
    pub(super) fn next_waiting_session(&mut self, refresh_if_missing: bool) -> bool {
        let rows = &self.state.sessions;
        let start = rows
            .iter()
            .position(|row| Some(&row.session_id) == self.state.selected.as_ref())
            .unwrap_or(rows.len().saturating_sub(1));
        let target = (1..=rows.len())
            .map(|offset| &rows[(start + offset) % rows.len()])
            .find(|row| {
                row.status == nori_protocol::SessionStatus::WaitingOnYou
                    && Some(&row.session_id) != self.state.selected.as_ref()
            })
            .map(|row| row.session_id.clone());
        if let Some(id) = target {
            self.open_session(id);
            return true;
        }
        if refresh_if_missing && self.state.connected {
            self.next_waiting_after_refresh = true;
            self.refresh_list();
        }
        false
    }

    fn delegation_key(&mut self, key: KeyEvent) -> bool {
        let Some(id) = self.state.selected.clone() else {
            return false;
        };
        let Some(delegation) = self
            .state
            .conversations
            .get(&id)
            .and_then(|conversation| {
                conversation.delegations.iter().find(|delegation| {
                    delegation.status == nori_protocol::DelegationStatus::AwaitingApproval
                })
            })
            .cloned()
        else {
            return false;
        };
        let approved = match (key.code, key.modifiers) {
            (KeyCode::Char('1'), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => true,
            (KeyCode::Char('2') | KeyCode::Esc, KeyModifiers::NONE)
            | (KeyCode::Char('c'), KeyModifiers::CONTROL) => false,
            _ => return false,
        };
        if let Err(error) = self.require_mutation() {
            self.state.show_notice(error.to_string());
            return true;
        }
        self.send(
            "_nori/session/delegation/resolve",
            json!({"session_id":id,"request_id":delegation.request_id,"approved":approved}),
            Pending::Mutation { draft: None },
        );
        true
    }

    /// The shared picker keys. While the composer holds text, only the
    /// arrows, PageUp/PageDown and Ctrl+F act on the list; other keys edit
    /// the draft.
    fn picker_key(&mut self, key: KeyEvent) -> bool {
        let search_active = self.state.picker.search_active;
        let Some(action) = crate::bottom_pane::picker_keys::navigation_action(&key, search_active)
        else {
            return false;
        };
        let list_only = matches!(
            key.code,
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown
        ) || key.modifiers == KeyModifiers::CONTROL;
        if !search_active && !list_only && !self.composer.is_empty() {
            return false;
        }
        if let PickerOutcome::Selected(id) = self.state.picker.handle(action) {
            self.open_session(id);
        }
        true
    }

    fn permission_key(&mut self, key: KeyEvent) -> bool {
        let Some(id) = self.state.selected.clone() else {
            return false;
        };
        let Some(permission) = self
            .state
            .conversations
            .get_mut(&id)
            .and_then(|conversation| conversation.permissions.first_mut())
        else {
            return false;
        };
        let reject = crate::answer_guard::esc_reject_option(&permission.options);
        let selected = match key.code {
            KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => None,
            KeyCode::Esc => reject,
            KeyCode::Up => {
                permission.selected = permission.selected.saturating_sub(1);
                return true;
            }
            KeyCode::Down => {
                permission.selected =
                    (permission.selected + 1).min(permission.options.len().saturating_sub(1));
                return true;
            }
            KeyCode::Enter if key.modifiers.is_empty() => Some(permission.selected),
            KeyCode::Char(character @ '1'..='9') if key.modifiers.is_empty() => {
                let index = usize::try_from(character.to_digit(10).unwrap_or(1)).unwrap_or(1) - 1;
                if index >= permission.options.len() {
                    return true;
                }
                Some(index)
            }
            _ => return false,
        };
        let outcome = match selected.and_then(|index| permission.options.get(index)) {
            Some(option) => acp::RequestPermissionOutcome::Selected(
                acp::SelectedPermissionOutcome::new(option.option_id.clone()),
            ),
            None => acp::RequestPermissionOutcome::Cancelled,
        };
        let request_id = permission.request_id.clone();
        if let Err(error) = self.require_mutation() {
            self.state.show_notice(error.to_string());
            return true;
        }
        let response = acp::RequestPermissionResponse::new(outcome);
        self.send(
            "_nori/session/respond",
            json!({"session_id":id, "request_id":request_id, "response":response}),
            Pending::Mutation { draft: None },
        );
        true
    }

    fn require_mutation(&self) -> anyhow::Result<()> {
        if let Some(reason) = &self.state.stopped {
            anyhow::bail!("{reason}");
        }
        anyhow::ensure!(
            self.state.connected,
            "Reconnecting. Your draft is preserved; send it after the connection is restored."
        );
        self.state.require_control()?;
        anyhow::ensure!(
            self.state.replay_error.is_none(),
            "Session history has a replay gap. Resolve it before sending more work."
        );
        Ok(())
    }

    fn submit(&mut self, text: String) -> bool {
        let command = text.trim();
        if matches!(command, "/quit" | "/exit" | "/detach") {
            return false;
        }
        if command == "/sessions" {
            self.state.list_open = true;
            self.refresh_list();
            return true;
        }
        if command.is_empty() {
            return true;
        }
        if let Err(error) = self.require_mutation() {
            self.composer.handle_paste(text);
            self.state.show_notice(error.to_string());
            return true;
        }
        self.state.notice = None;
        if let Some(args) = command
            .strip_prefix("/new")
            .filter(|args| args.is_empty() || args.starts_with(' '))
        {
            match super::new_session::params(args, &self.cwd, self.session_folders) {
                Ok(params) => self.send("_nori/session/new", params, Pending::New),
                Err(message) => {
                    self.composer.handle_paste(text);
                    self.state.show_notice(message);
                }
            }
            return true;
        }
        let Some(id) = self.state.selected.clone() else {
            self.composer.handle_paste(text);
            self.state
                .show_notice("Choose a session with /sessions or start one with /new.");
            return true;
        };
        if let Some(command) = crate::delegation::parse_command(command) {
            match command {
                Ok((agent, task)) => self.send(
                    "_nori/session/delegate",
                    json!({"session_id":id,"agent":agent,"task":task}),
                    Pending::Mutation { draft: Some(text) },
                ),
                Err(message) => self.state.show_notice(message),
            }
            return true;
        }
        let method = match command {
            "/close" => Some("_nori/session/close"),
            "/cancel" => Some("_nori/session/cancel"),
            "/resume" => Some("_nori/session/resume"),
            _ => None,
        };
        if let Some(method) = method {
            self.send(
                method,
                json!({"session_id":id}),
                Pending::Mutation { draft: None },
            );
        } else {
            match self.state.prompt(&id, &text) {
                Ok(params) => self.send(
                    "_nori/session/prompt",
                    params,
                    Pending::Mutation { draft: Some(text) },
                ),
                Err(error) => {
                    self.composer.handle_paste(text);
                    self.state.show_notice(error.to_string());
                }
            }
        }
        self.state.scroll = 0;
        true
    }
}
