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
use super::state::CancelDelivery;
use crate::host_client::HostCall;
use crate::slash_command::SlashCommand;

/// Folders a host lists for one Tab, the most it returns. A list this long
/// may have been cut short, so Tab then only lists it.
pub(super) const FOLDER_MATCHES: usize = 50;

impl Runtime {
    /// False means detach. It never requests host or session shutdown.
    pub fn key(&mut self, key: KeyEvent) -> bool {
        self.note_request_on_screen();
        self.sync_command_menu();
        let handled = self.handle_key(key);
        self.search_files();
        self.note_request_on_screen();
        handled
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.kind == KeyEventKind::Release
            || (key.code == KeyCode::Esc && key.kind == KeyEventKind::Repeat)
        {
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
        // The parent key only ever navigates. Without a parent it does
        // nothing, so it never reaches the composer or its history recall.
        if crate::nori::hotkey_match::matches_binding(&self.state.parent_binding, &key) {
            if let Some(parent) = self.state.selected.as_ref().and_then(|id| {
                self.state
                    .sessions
                    .iter()
                    .find(|row| &row.session_id == id)
                    .and_then(|row| row.delegating_parent().map(str::to_owned))
            }) && self
                .state
                .sessions
                .iter()
                .any(|row| row.session_id == parent)
            {
                self.open_session(parent);
            }
            return true;
        }
        if crate::nori::hotkey_match::matches_binding(&self.session_list_binding, &key) {
            self.state.list_open = !self.state.list_open;
            self.refresh_list();
            return true;
        }
        if self.mode_menu_key(key) {
            return true;
        }
        if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('t') {
            if self.state.connected {
                self.send(HostCall::TakeControl, Pending::TakeControl);
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
        if key.code == KeyCode::Esc && !self.state.list_open && self.composer.popup_open() {
            self.composer.attachment_input(key, false);
            return true;
        }
        if key.code == KeyCode::Esc {
            if !self.state.list_open
                && self
                    .state
                    .selected
                    .as_ref()
                    .and_then(|id| self.state.conversations.get(id))
                    .is_some_and(|c| {
                        c.active_prompt().is_some()
                            || c.submitted_prompt
                                .as_ref()
                                .is_some_and(crate::host_client::PromptInterrupt::live)
                            || c.external_turn.is_some()
                    })
            {
                self.cancel_selected();
            } else if self.state.list_open && self.state.picker.search_active {
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
                    self.resume(&id);
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
            let observing = !self.state.list_open
                && self
                    .state
                    .selected
                    .as_ref()
                    .and_then(|id| self.state.conversations.get(id))
                    .is_some_and(|c| c.external_turn.is_some());
            if self.composer.is_empty() || observing {
                self.cancel_selected();
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
        if key.code == KeyCode::Tab && self.find_paths {
            let text = self.composer.text();
            if let Some(typed) = crate::session_folder::typed_folder(&text) {
                // The host completes the folder from its own disk, where the
                // session on screen works.
                let query = typed.to_owned();
                self.send(
                    HostCall::FindPaths {
                        session_id: self.state.selected.clone(),
                        query,
                        kind: nori_protocol::PathKind::Folders,
                        limit: FOLDER_MATCHES as i32,
                    },
                    Pending::CompleteFolder { text },
                );
                return true;
            }
        }
        if key.code == KeyCode::Tab
            && !self.find_paths
            && let Some((completed, candidates)) =
                crate::session_folder::complete(&self.composer.text(), &self.cwd)
        {
            self.composer.set_text(completed);
            if !candidates.is_empty() {
                self.state.show_notice(candidates.join("  "));
            }
            return true;
        }
        let active_turn = self
            .state
            .selected
            .as_ref()
            .and_then(|id| self.state.conversations.get(id))
            .and_then(|conversation| conversation.active_prompt().cloned());
        use crate::bottom_pane::InputResult;
        let result = self.composer.attachment_input(key, active_turn.is_some());
        match result {
            InputResult::Queued(text) => {
                self.submit_with_intent(text, active_turn, nori_harness::FollowUpIntent::Queue)
            }
            InputResult::Submitted(text) => {
                self.submit_with_intent(text, active_turn, nori_harness::FollowUpIntent::Steer)
            }
            InputResult::Command(command) => {
                self.run_command(command, "", format!("/{}", command.command()))
            }
            InputResult::None => true,
        }
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
            HostCall::ResolveDelegation {
                session_id: id,
                request_id: json!(delegation.request_id),
                approved,
            },
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
            HostCall::Respond {
                session_id: id,
                request_id: json!(request_id),
                response,
            },
            Pending::Mutation { draft: None },
        );
        true
    }

    pub(super) fn require_mutation(&self) -> anyhow::Result<()> {
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

    fn cancel_selected(&mut self) {
        if self.state.list_open {
            return;
        }
        if let Err(error) = self.require_mutation() {
            self.state.show_notice(error.to_string());
            return;
        }
        let Some(id) = self.state.selected.clone() else {
            self.state
                .show_notice("Choose a session with /sessions or start one with /new.");
            return;
        };
        self.state.notice = None;
        let conversation = self.state.conversations.get(&id);
        if let Some(c) = conversation
            && let Some(turn) = &c.external_turn
        {
            if turn.state == nori_protocol::ExternalTurnState::Working
                && c.cancel_delivery != CancelDelivery::AwaitingHost
            {
                let observed_turn_id = turn.id.clone();
                self.send(
                    HostCall::CancelObserved {
                        session_id: id.clone(),
                        observed_turn_id: observed_turn_id.clone(),
                    },
                    Pending::CancelObserved {
                        session_id: id,
                        observed_turn_id,
                    },
                );
            }
        } else if let Some(acp::RequestId::Str(turn)) = conversation.and_then(|c| c.active_prompt())
        {
            self.send(
                HostCall::CancelObserved {
                    session_id: id,
                    observed_turn_id: turn.clone(),
                },
                Pending::Mutation { draft: None },
            );
        } else if let Some(pending) = conversation
            .and_then(|c| c.submitted_prompt.as_ref())
            .filter(|p| p.live())
        {
            if let Some(call) = pending.interrupt() {
                self.send(call, Pending::Mutation { draft: None });
            }
        } else if !conversation.is_some_and(super::state::Conversation::cancelling_prompt) {
            self.state.show_notice("No active turn to cancel.");
        }
    }

    fn submit_with_intent(
        &mut self,
        text: String,
        active_turn: Option<acp::RequestId>,
        intent: nori_harness::FollowUpIntent,
    ) -> bool {
        let command = text.trim();
        if command == "/detach" {
            return false;
        }
        if command.is_empty() {
            return true;
        }
        if command == "/cancel" {
            self.cancel_selected();
            return true;
        }
        // `/close` and `/resume` take no arguments; with text after them the
        // line is a prompt, as before the shared menu.
        if let Some((builtin, args)) = crate::command_availability::parse_builtin(command)
            && (!matches!(builtin, SlashCommand::Close | SlashCommand::Resume)
                || args.trim().is_empty())
        {
            let args = args.to_owned();
            return self.run_command(builtin, &args, text);
        }
        if let Err(error) = self.require_mutation() {
            self.composer.handle_paste(text);
            self.state.show_notice(error.to_string());
            return true;
        }
        self.state.notice = None;
        let Some(id) = self.state.selected.clone() else {
            self.composer.handle_paste(text);
            self.state
                .show_notice("Choose a session with /sessions or start one with /new.");
            return true;
        };
        if let Some(expected_turn) = active_turn {
            let message_id = uuid::Uuid::new_v4().to_string();
            self.send(
                HostCall::FollowUp {
                    session_id: id.clone(),
                    expected_turn,
                    message_id: message_id.clone(),
                    intent,
                    prompt: vec![acp::ContentBlock::Text(acp::TextContent::new(text.clone()))],
                },
                Pending::FollowUp {
                    session_id: id,
                    message_id,
                    draft: text,
                },
            );
        } else {
            match self.state.prompt(&id, &text) {
                Ok(params) => {
                    let interrupt = crate::host_client::PromptInterrupt::new(id.clone());
                    let conversation = self.state.conversations.entry(id.clone()).or_default();
                    crate::host_client::PromptInterrupt::track_first(
                        &mut conversation.submitted_prompt,
                        &interrupt,
                    );
                    self.send(
                        HostCall::Prompt(params),
                        Pending::Prompt {
                            interrupt,
                            control_epoch: self.control_epoch,
                            draft: text,
                        },
                    );
                }
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
