//! Cancellation feedback shared by the local chat and the host attachment.

use nori_protocol::ExternalTurnState;

pub(crate) fn pending_label(state: ExternalTurnState) -> Option<&'static str> {
    match state {
        ExternalTurnState::CancelRequested => Some("Cancel requested"),
        ExternalTurnState::Unconfirmed => Some("Cancel requested · unconfirmed"),
        _ => None,
    }
}

pub(crate) fn terminal_message(state: ExternalTurnState, pending: bool) -> Option<&'static str> {
    match state {
        ExternalTurnState::Cancelled => Some("Agent confirmed cancellation."),
        ExternalTurnState::Idle if pending => {
            Some("Agent is idle. Cancellation was not confirmed.")
        }
        ExternalTurnState::Superseded if pending => {
            Some("A new turn started. Cancellation was not confirmed.")
        }
        _ => None,
    }
}
