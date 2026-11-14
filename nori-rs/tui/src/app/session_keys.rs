//! Keys that move between sessions, active with `[features] multi_session`.

use crossterm::event::KeyEvent;
use nori_config::HotkeyAction;

use super::App;
use super::session_deck::Direction;
use crate::nori::hotkey_match::matches_binding;
use crate::tui;

impl App {
    /// Handle a session-move hotkey. Returns true when the key was used.
    pub(super) fn handle_session_hotkey(&mut self, tui: &mut tui::Tui, key: &KeyEvent) -> bool {
        if !self.config.multi_session {
            return false;
        }
        let pressed = |action| matches_binding(self.hotkey_config.binding_for(action), key);
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
