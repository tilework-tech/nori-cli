//! Choosing one queued message from the list above the composer, and
//! resuming a queue held after an interrupted or failed turn.
//!
//! Up on an empty composer focuses the list on its newest editable message.
//! j/k (or Down/Up) move between editable messages; Up stops at the oldest,
//! and Down from the newest returns to the composer. Enter or e takes the
//! selected one back into the composer, and Delete or x discards it. Both go
//! through a reclaim by stable ID, so a message that already started
//! is reported instead of being sent twice. A session on the host is resumed
//! and reclaimed through the host (`queue_host`), so every terminal attached
//! to that host acts on the same queue; other widgets use their harness. Esc leaves the list; any other
//! key leaves it and reaches the composer.
//!
//! Enter on an empty composer resumes a paused queue. A non-empty composer
//! keeps its ordinary Enter and Tab behavior.

use super::*;
use crate::app_event::HarnessAction;
use crate::app_event::QueuedPromptAction;

impl ChatWidget {
    /// Returns `true` when the focused queue list consumed the key.
    pub(super) fn handle_queue_list_key(&mut self, key_event: KeyEvent) -> bool {
        if !self.bottom_pane.queue_list_is_focused()
            || !matches!(key_event.kind, KeyEventKind::Press | KeyEventKind::Repeat)
        {
            return false;
        }
        if self.follow_only_attachment || self.bottom_pane.has_active_overlay_or_popup() {
            self.bottom_pane.blur_queue_list();
            return false;
        }
        if key_event.modifiers != KeyModifiers::NONE {
            self.bottom_pane.blur_queue_list();
            return false;
        }
        match key_event.code {
            KeyCode::Char('j') | KeyCode::Down => self.bottom_pane.move_queue_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.bottom_pane.move_queue_selection(-1),
            KeyCode::Enter | KeyCode::Char('e') => {
                if self.take_selected_queued_message(QueuedPromptAction::Edit) {
                    self.bottom_pane.blur_queue_list();
                }
            }
            KeyCode::Delete | KeyCode::Char('x') => {
                self.take_selected_queued_message(QueuedPromptAction::Remove);
            }
            KeyCode::Esc => self.bottom_pane.blur_queue_list(),
            // Every other key belongs to the composer.
            _ => {
                self.bottom_pane.blur_queue_list();
                return false;
            }
        }
        true
    }

    /// Returns `false` when nothing is selected, so the key does nothing.
    fn take_selected_queued_message(&mut self, action: QueuedPromptAction) -> bool {
        let Some(event_id) = self.bottom_pane.start_selected_queued_message_reclaim() else {
            return false;
        };
        if let Some(session) = self.host_queue_session() {
            self.take_queued_on_host(session, event_id, action);
            return true;
        }
        self.app_event_tx.send(AppEvent::HarnessAction(
            HarnessAction::ReclaimQueuedPrompt { event_id, action },
        ));
        true
    }

    /// Returns `false` when there is no editable message to select, so Up
    /// keeps its composer meaning.
    pub(super) fn focus_queue_list(&mut self) -> bool {
        !self.follow_only_attachment && self.bottom_pane.focus_queue_list()
    }

    /// Returns `false` when the queue is not paused, so Enter keeps its
    /// composer meaning.
    pub(super) fn resume_paused_queue(&mut self) -> bool {
        if self.follow_only_attachment || !self.bottom_pane.queued_messages_paused() {
            return false;
        }
        match self.host_queue_session() {
            Some(session) => self.resume_queue_on_host(session),
            None => self
                .app_event_tx
                .send(AppEvent::HarnessAction(HarnessAction::ResumeQueue)),
        }
        true
    }
}
