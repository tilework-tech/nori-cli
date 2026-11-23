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
        if let Some(turn) = &self.external_turn
            && turn.state == ExternalTurnState::Working
        {
            self.app_event_tx.send(AppEvent::HarnessAction(
                crate::app_event::HarnessAction::CancelExternal {
                    generation: self.session_generation,
                    id: turn.id.clone(),
                },
            ));
        }
    }

    pub(super) fn external_cancel_status(&self) -> Option<&'static str> {
        pending_label(self.external_turn.as_ref()?.state)
    }

    pub(super) fn on_external_turn_changed(&mut self, turn: ExternalTurn) {
        use ExternalTurnState::*;
        if turn.state == Working {
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

        match turn.state {
            CancelRequested | Unconfirmed => {
                self.external_turn = Some(turn);
                self.bottom_pane.ensure_status_indicator();
                self.bottom_pane.set_interrupt_hint_visible(false);
                self.set_status_header(self.current_status_header.clone());
            }
            Idle | Superseded | Cancelled => {
                let pending = self.external_cancel_status().is_some();
                self.external_turn = None;
                if turn.state == Superseded {
                    self.separate_proactive_turn();
                    self.bottom_pane.hide_status_indicator();
                } else {
                    self.complete_proactive_turn();
                }
                if let Some(message) = terminal_message(turn.state, pending) {
                    self.add_info_message(message.to_owned(), None);
                }
            }
            Working => unreachable!(),
        }
        self.request_redraw();
    }
}
