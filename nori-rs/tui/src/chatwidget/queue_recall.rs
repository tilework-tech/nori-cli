//! Taking a queued follow-up back into the composer.
//!
//! The harness owns the queue that is delivered. Editing a message selected
//! in the queue list hides its row and asks the harness to remove that prompt
//! by ID. The text returns to the composer only when the harness confirms the
//! removal; if the prompt already started, the user is told and the composer
//! is left alone.

use super::*;

impl ChatWidget {
    /// Place a reclaimed message before any draft typed while the reclaim
    /// was in flight. The row stays hidden after a successful reclaim until the next queue
    /// snapshot drops it, since that snapshot may arrive after this answer.
    /// A failed request only shows the row again; the error itself is
    /// reported as a failed harness action.
    pub(crate) fn on_queued_prompt_reclaimed(
        &mut self,
        event_id: String,
        action: crate::app_event::QueuedPromptAction,
        outcome: crate::app_event::ReclaimOutcome,
    ) {
        use crate::app_event::QueuedPromptAction;
        use crate::app_event::ReclaimOutcome;
        let text = match (outcome, action) {
            (ReclaimOutcome::Reclaimed(_), QueuedPromptAction::Remove) => {
                self.request_redraw();
                return;
            }
            (ReclaimOutcome::Reclaimed(text), QueuedPromptAction::Edit) => text,
            (ReclaimOutcome::AlreadyStarted, action) => {
                self.bottom_pane.finish_queued_message_reclaim(&event_id);
                let message = match action {
                    QueuedPromptAction::Edit => {
                        "That queued message already started, so it was sent as written."
                    }
                    QueuedPromptAction::Remove => {
                        "That queued message already started, so it was not removed."
                    }
                };
                self.add_info_message(message.to_string(), None);
                return;
            }
            (ReclaimOutcome::Failed, _) => {
                self.bottom_pane.finish_queued_message_reclaim(&event_id);
                return;
            }
        };
        if !self.bottom_pane.prepend_reclaimed_text(&text) {
            self.add_info_message(
                format!(
                    "Took this queued message back, but the prompt cannot hold it now:\n{text}"
                ),
                None,
            );
        }
        self.request_redraw();
    }
}
