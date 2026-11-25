//! A refused `/close` keeps its session and is shown once, in the transcript
//! of the session it was meant to end.

use super::runtime::Pending;
use super::runtime::Runtime;
use super::state::Part;
use crate::host_client::HostCall;

impl Runtime {
    pub(super) fn close(&mut self, session_id: String) {
        if self.client.has_pending(
            |pending| matches!(pending, Pending::Close { session_id: closing } if *closing == session_id),
        ) {
            self.state.show_notice("This session is already closing.");
            return;
        }
        let sent = self.send(
            HostCall::Close {
                session_id: session_id.clone(),
            },
            Pending::Close {
                session_id: session_id.clone(),
            },
        );
        if sent {
            self.state
                .conversations
                .entry(session_id)
                .or_default()
                .close_refusal
                .start();
        }
    }

    /// The host refused the close with `text`, which carries the adapter's
    /// reason. A terminal showing another session learns of it in the
    /// notice row.
    pub(super) fn close_refused(&mut self, session_id: &str, text: &str) {
        let conversation = self
            .state
            .conversations
            .entry(session_id.to_owned())
            .or_default();
        for error in conversation.close_refusal.refused(text) {
            conversation.parts.push(Part::Notice(error));
        }
        conversation
            .parts
            .push(Part::Notice(format!("Failed to close the session: {text}")));
        if self.state.selected.as_deref() != Some(session_id) && self.state.stopped.is_none() {
            self.state.notice = Some(format!("Failed to close session {session_id}: {text}"));
        }
    }

    /// The close was accepted, or the connection dropped before it was
    /// answered: errors held for a refusal belong to something else.
    pub(super) fn close_settled(&mut self, session_id: &str) {
        if let Some(conversation) = self.state.conversations.get_mut(session_id) {
            for error in conversation.close_refusal.released() {
                conversation.parts.push(Part::Notice(error));
            }
        }
    }
}
