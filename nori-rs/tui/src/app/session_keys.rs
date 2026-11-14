//! Keys that move between sessions, active with `[features] multi_session`.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use nori_config::HotkeyAction;

use super::App;
use super::session_deck::Direction;
use crate::nori::hotkey_match::matches_binding;
use crate::tui;

/// Whether Left on an empty prompt opens the session list. The list is also
/// on the `open_session_list` hotkey and `/sessions`, so turning this off is
/// a one-line change.
const LEFT_ON_EMPTY_PROMPT_OPENS_SESSION_LIST: bool = true;

impl App {
    /// Handle a session-move hotkey. Returns true when the key was used.
    /// Moves work over an open view, so the user can leave a session that is
    /// showing a request; opening the list needs the composer in front.
    pub(super) fn handle_session_hotkey(&mut self, tui: &mut tui::Tui, key: &KeyEvent) -> bool {
        if !self.config.multi_session {
            return false;
        }
        let pressed = |action| matches_binding(self.hotkey_config.binding_for(action), key);
        let composer_in_front = !self.chat_widget.has_active_popup();
        let left_on_empty_prompt = LEFT_ON_EMPTY_PROMPT_OPENS_SESSION_LIST
            && key.code == KeyCode::Left
            && key.modifiers == KeyModifiers::NONE
            && self.chat_widget.composer_is_empty();
        if composer_in_front && (pressed(HotkeyAction::OpenSessionList) || left_on_empty_prompt) {
            self.open_session_list();
            tui.frame_requester().schedule_frame();
            return true;
        }
        if pressed(HotkeyAction::NextWaitingSession) && self.chat_widget.composer_is_empty() {
            self.show_next_waiting_session();
            tui.frame_requester().schedule_frame();
            return true;
        }
        let direction = if pressed(HotkeyAction::PreviousSession) {
            Direction::Previous
        } else if pressed(HotkeyAction::NextSession) {
            Direction::Next
        } else {
            return false;
        };
        self.step_session(direction);
        tui.frame_requester().schedule_frame();
        true
    }
}
