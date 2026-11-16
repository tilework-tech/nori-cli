use super::*;
use nori_protocol::Delegation;
use nori_protocol::DelegationStatus;

impl ChatWidget {
    pub(super) fn handle_delegate_command(&mut self, text: &str) -> bool {
        let Some(command) = crate::delegation::parse_command(text) else {
            return false;
        };
        if !self.config.multi_session {
            self.add_error_message("Delegation requires [features] multi_session = true.".into());
            return true;
        }
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
        let manager = self.session_manager.clone();
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
        self.add_info_message(crate::delegation::summary(&delegation), None);
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
        let items = [("Start", true), ("Decline", false)]
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
        self.bottom_pane.show_selection_view(SelectionViewParams {
            on_dismiss: Some(Box::new(move |tx| {
                tx.send(AppEvent::ResolveDelegation {
                    parent,
                    request_id: request_id.clone(),
                    approved: false,
                })
            })),
            header: Box::new(ColumnRenderable::with(vec![
                Box::new(Line::from("Start delegated session?").bold()) as Box<dyn Renderable>,
                Box::new(
                    Paragraph::new(format!(
                        "Child {} ({})",
                        delegation.child_id, delegation.agent
                    ))
                    .wrap(Wrap { trim: false }),
                ),
                Box::new(
                    Paragraph::new(format!(
                        "parent {} ({})",
                        delegation.parent_id, delegation.parent_agent
                    ))
                    .wrap(Wrap { trim: false }),
                ),
                Box::new(Paragraph::new(delegation.task).wrap(Wrap { trim: false })),
            ])),
            items,
            footer_hint: Some(standard_popup_hint_line()),
            ..Default::default()
        });
        true
    }
}
