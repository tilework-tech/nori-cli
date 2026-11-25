//! How the attachment shows replies that failed or settle an observed-turn cancel.

use serde_json::Value;

use super::runtime::Pending;
use super::runtime::Runtime;
use super::state::CancelDelivery;

impl Runtime {
    pub(super) fn reply_failed(&mut self, pending: Pending, text: String) {
        if matches!(pending, Pending::List) {
            self.next_waiting_after_refresh = false;
        }
        if matches!(pending, Pending::Initialize) {
            self.state.connected = false;
            self.next_waiting_after_refresh = false;
            self.state.stopped = Some(format!(
                "Host initialization failed: {text}. Re-run nori attach."
            ));
            self.client.stop();
        }
        if let Pending::Subscribe { session_id } = &pending {
            if self.state.selected.as_ref() != Some(session_id) {
                return;
            }
            self.state.replay_error = Some(format!(
                "Replay unavailable: {text}. Existing conversation retained."
            ));
        }
        if let Pending::FollowUp {
            session_id,
            message_id,
            draft,
        } = &pending
        {
            if self.state.selected.as_ref() == Some(session_id) && self.composer.is_empty() {
                self.composer.set_text(draft.clone());
            }
            self.follow_up_notice(session_id, format!("{text} [{message_id}] {draft}"));
            return;
        }
        if let Pending::Prompt { interrupt, .. } = &pending {
            interrupt.retire();
        }
        if let Pending::Mutation { draft: Some(draft) } | Pending::Prompt { draft, .. } = &pending
            && self.composer.is_empty()
        {
            self.composer.handle_paste(draft.clone());
        }
        self.state.show_notice(text);
    }

    pub(super) fn cancel_observed_reply(
        &mut self,
        session_id: &str,
        observed_turn_id: &str,
        reply: Result<&Value, String>,
    ) {
        let Some(conversation) = self.state.conversations.get_mut(session_id) else {
            return;
        };
        if let Some(turn) = &mut conversation.external_turn
            && turn.id == observed_turn_id
            && conversation.cancel_delivery == CancelDelivery::AwaitingHost
        {
            conversation.cancel_delivery = CancelDelivery::Unsent;
            match reply {
                Err(reason) => conversation.parts.push(super::state::Part::Notice(format!(
                    "Cancel request failed: {reason}"
                ))),
                Ok(result) if result["sessionId"].as_str() == Some(session_id) => {
                    turn.state = nori_protocol::ExternalTurnState::CancelRequested;
                }
                Ok(_) => {
                    conversation.cancel_delivery = CancelDelivery::Unknown;
                    conversation.parts.push(super::state::Part::Notice(
                        "The host did not confirm receipt of the cancel request.".into(),
                    ));
                }
            }
        }
    }
}
