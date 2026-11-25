//! `/compact` on a session the host runs asks that host, through
//! `_nori/session/compact`, to compact that session. The host answers once
//! it admits the compaction; the finish arrives as the session's own
//! `context_compacted` event, so it lands in that session's transcript even
//! when another session is on screen by then.

use super::*;
use crate::app_event::HarnessAction;
use crate::app_event_sender::HostReply;
use crate::host_client::HostCall;

impl ChatWidget {
    /// Sends `/compact` for a host session to the host; any other action,
    /// or any widget without a host session, is returned for the harness.
    pub(super) fn compact_action_on_host(&self, action: HarnessAction) -> Option<HarnessAction> {
        let Some(session_id) = self
            .managed_conversation_id
            .filter(|_| !self.cloud_mode)
            .map(|id| id.to_string())
        else {
            return Some(action);
        };
        match action {
            HarnessAction::Compact => {
                self.app_event_tx.send(AppEvent::HostRequest {
                    call: HostCall::Compact { session_id },
                    reply: HostReply::ReportError(self.app_event_tx.clone()),
                });
                None
            }
            other => Some(other),
        }
    }
}
