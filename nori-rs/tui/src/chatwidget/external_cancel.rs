//! Agent-started work has an interrupt control without acquiring prompt ownership.

use super::*;
use crate::external_cancel::pending_label;
use crate::external_cancel::terminal_message;
use nori_protocol::ExternalTurn;
use nori_protocol::ExternalTurnState;

impl ChatWidget {
    pub(super) fn can_control_external_turn(&self) -> bool {
        !self.follow_only_attachment && self.external_turn.is_some()
    }

    pub(super) fn request_external_cancel(&self) {
        if !self.follow_only_attachment
            && let (Some(session), Some(turn)) = (self.managed_conversation_id, &self.external_turn)
            && turn.state == ExternalTurnState::Working
        {
            self.app_event_tx.send(AppEvent::HostRequest {
                call: crate::host_client::HostCall::CancelObserved {
                    session_id: session.to_string(),
                    observed_turn_id: turn.id.clone(),
                },
                reply: crate::app_event_sender::HostReply::ReportError(self.app_event_tx.clone()),
            });
        }
    }

    pub(super) fn interrupt_host_prompt(&self) -> bool {
        let Some(session) = self.managed_conversation_id.filter(|_| !self.cloud_mode) else {
            return false;
        };
        if self.follow_only_attachment {
            return self.bottom_pane.is_task_running();
        }
        let call = if let Some(nori_protocol::acp::v1::RequestId::Str(id)) =
            &self.owned_prompt_request_id
        {
            Some(crate::host_client::HostCall::CancelObserved {
                session_id: session.to_string(),
                observed_turn_id: id.clone(),
            })
        } else if let Some(pending) = self
            .submitted_prompt
            .as_ref()
            .filter(|pending| pending.live())
        {
            pending.interrupt()
        } else {
            // A status row can outlive its turn. Never retarget its key at a later prompt.
            return self.bottom_pane.is_task_running();
        };
        if let Some(call) = call {
            self.app_event_tx.send(AppEvent::HostRequest {
                call,
                reply: crate::app_event_sender::HostReply::ReportError(self.app_event_tx.clone()),
            });
        }
        true
    }

    pub(super) fn external_cancel_status(&self) -> Option<&'static str> {
        pending_label(self.external_turn.as_ref()?.state)
    }

    pub(super) fn on_external_turn_changed(&mut self, turn: ExternalTurn) {
        if turn.state == ExternalTurnState::Working {
            self.start_proactive_turn();
            self.external_turn = Some(turn);
            self.bottom_pane
                .set_interrupt_hint_visible(!self.follow_only_attachment);
            return;
        }
        if self
            .external_turn
            .as_ref()
            .is_none_or(|active| active.id != turn.id)
        {
            return;
        }

        if let Some(label) = pending_label(turn.state) {
            self.external_turn = Some(turn);
            self.bottom_pane.ensure_status_indicator();
            self.bottom_pane.set_interrupt_hint_visible(false);
            self.set_status_header(label.to_owned());
        } else {
            let pending = self.external_cancel_status().is_some();
            self.external_turn = None;
            if turn.state == ExternalTurnState::Superseded {
                self.separate_proactive_turn();
                self.bottom_pane.hide_status_indicator();
            } else {
                self.complete_proactive_turn();
            }
            if let Some(message) = terminal_message(turn.state, pending) {
                self.add_info_message(message.to_owned(), None);
            }
        }
        self.request_redraw();
    }
}
