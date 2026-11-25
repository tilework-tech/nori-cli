use super::*;
use nori_protocol::Delegation;
use nori_protocol::DelegationStatus;

impl ChatWidget {
    pub(super) fn handle_delegate_command(&mut self, text: &str) -> bool {
        let Some(command) = crate::delegation::parse_command(text) else {
            return false;
        };
        if text.trim() == "/delegate" && self.show_delegation_approval() {
            return true;
        }
        let (agent, task) = match command {
            Ok(command) => command,
            Err(message) => {
                self.add_error_message(message);
                return true;
            }
        };
        let Some(parent) = self.managed_conversation_id else {
            self.add_error_message("Start a session before delegating a task.".into());
            return true;
        };
        if self.session_manager.is_none() && self.host_connection.is_some() {
            self.delegate_on_host(parent, agent, task);
            return true;
        }
        let Some(manager) = self.session_manager.clone() else {
            self.add_error_message(NEEDS_OWN_HOST_MESSAGE.to_string());
            return true;
        };
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            if let Err(error) = manager.delegate(&parent, agent, task).await {
                tx.send(AppEvent::InsertHistoryCell(Box::new(
                    history_cell::new_error_event(format!("Delegation failed: {error}")),
                )));
            }
        });
        true
    }

    /// A joined host starts the child itself; it appears in the host's list.
    fn delegate_on_host(&self, parent: ConversationId, agent: String, task: String) {
        let tx = self.app_event_tx.clone();
        let reply = tx.host_receipt(crate::host_client::HostCall::Delegate {
            session_id: parent.to_string(),
            agent,
            task,
        });
        tokio::spawn(async move {
            if let Err(error) = reply.await {
                tx.send(AppEvent::InsertHistoryCell(Box::new(
                    history_cell::new_error_event(format!("Delegation failed: {error}")),
                )));
            }
        });
    }

    pub(super) fn handle_delegation_changed(&mut self, delegation: Delegation) {
        if self
            .managed_conversation_id
            .is_some_and(|id| id.to_string() != delegation.parent_id)
        {
            return;
        }
        if self.delegations.get(&delegation.request_id) == Some(&delegation) {
            return;
        }
        let parent_changed = self
            .delegations
            .get(&delegation.request_id)
            .is_some_and(|previous| previous.parent_id != delegation.parent_id);
        if (delegation.status != DelegationStatus::AwaitingApproval || parent_changed)
            && self.delegation_approval.as_ref() == Some(&delegation.request_id)
        {
            self.bottom_pane.dismiss_selection(&delegation.request_id);
            self.delegation_approval = None;
        }
        if delegation.status != DelegationStatus::AwaitingApproval {
            let hints = self.delegation_hints();
            self.add_boxed_history(Box::new(crate::delegation::DelegationCell::new(
                delegation.clone(),
                hints,
            )));
        }
        self.delegations
            .insert(delegation.request_id.clone(), delegation);
        self.show_delegation_approval();
    }

    fn show_delegation_approval(&mut self) -> bool {
        if self.delegation_approval.is_some() {
            return true;
        }
        let Some(delegation) = self
            .delegations
            .values()
            .find(|delegation| delegation.status == DelegationStatus::AwaitingApproval)
            .cloned()
        else {
            return false;
        };
        let Ok(parent) = ConversationId::from_string(&delegation.parent_id) else {
            return false;
        };
        let items = [("Yes, start it", true), ("No, decline", false)]
            .into_iter()
            .map(|(name, approved)| {
                let request_id = delegation.request_id.clone();
                let stable_id = request_id.clone();
                SelectionItem {
                    name: name.into(),
                    actions: vec![Box::new(move |tx| {
                        tx.send(AppEvent::ResolveDelegation {
                            parent,
                            request_id: request_id.clone(),
                            approved,
                        })
                    })],
                    search_value: Some(stable_id),
                    dismiss_on_select: false,
                    ..Default::default()
                }
            })
            .collect();
        self.delegation_approval = Some(delegation.request_id.clone());
        let request_id = delegation.request_id.clone();
        let (title, rows) = crate::delegation::approval_lines(&delegation);
        self.bottom_pane.show_selection_view(SelectionViewParams {
            on_dismiss: Some(Box::new(move |tx| {
                tx.send(AppEvent::ResolveDelegation {
                    parent,
                    request_id: request_id.clone(),
                    approved: false,
                })
            })),
            header: Box::new(ColumnRenderable::with(vec![
                Box::new(Line::from(title.bold())) as Box<dyn Renderable>,
                Box::new(Line::from("")),
                Box::new(Paragraph::new(rows).wrap(Wrap { trim: false })),
            ])),
            items,
            footer_hint: Some(Line::from(vec![
                "Press ".into(),
                crate::key_hint::plain(crossterm::event::KeyCode::Enter).into(),
                " to confirm or ".into(),
                crate::key_hint::plain(crossterm::event::KeyCode::Esc).into(),
                " to decline".into(),
            ])),
            answer_guard: Some(crate::answer_guard::AnswerGuard::new(
                self.bottom_pane.clock(),
            )),
            ..Default::default()
        });
        true
    }

    fn delegation_hints(&self) -> crate::delegation::DelegationHints {
        let key = |action| {
            let binding = self.config.hotkeys.binding_for(action);
            (!binding.is_none()).then(|| binding.display_name())
        };
        crate::delegation::DelegationHints {
            transcript: key(nori_config::HotkeyAction::OpenTranscript),
            sessions: key(nori_config::HotkeyAction::OpenSessionList),
        }
    }
}
