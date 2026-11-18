//! Taking a queued follow-up back into the composer.
//!
//! The harness owns the queue that is delivered. Up on an empty composer, or
//! Alt+Up at any time, hides the newest editable row and asks the harness to
//! remove that prompt by ID. The text returns to the composer only when the
//! harness confirms the removal; if the prompt already started, the user is
//! told and the composer is left alone.

use super::*;

impl ChatWidget {
    /// Whether Alt+Up would take a queued message back. Session hotkeys
    /// that also use Alt+Up yield to it when they have nothing to do.
    pub(crate) fn has_reclaimable_queued_message(&self) -> bool {
        !self.follow_only_attachment
            && !self.bottom_pane.has_active_overlay_or_popup()
            && self.bottom_pane.has_reclaimable_queued_message()
    }

    /// Returns `false` when no queued message can be taken back, so the key
    /// falls through to the composer.
    pub(super) fn start_queued_prompt_reclaim(&mut self) -> bool {
        if self.follow_only_attachment {
            return false;
        }
        let Some(event_id) = self.bottom_pane.start_queued_message_reclaim() else {
            return false;
        };
        self.app_event_tx.send(AppEvent::HarnessAction(
            crate::app_event::HarnessAction::ReclaimQueuedPrompt {
                event_id,
                action: crate::app_event::QueuedPromptAction::Edit,
            },
        ));
        true
    }

    /// Place a reclaimed message before any draft. Reclaims run newest
    /// first, so placing each one first keeps the original queue order. The
    /// row stays hidden after a successful reclaim until the next queue
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
