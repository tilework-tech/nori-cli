//! Hotkey picker: videogame-style keybinding configuration UI.
//!
//! Displays all configurable hotkey actions with their current bindings.
//! Users can select an action to rebind it by pressing a new key combination.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use nori_config::HotkeyAction;
use nori_config::HotkeyBinding;
use nori_config::HotkeyConfig;
use nori_tui_components::KeyHint;
use nori_tui_components::Picker;
use nori_tui_components::PickerColumn;
use nori_tui_components::PickerColumnWidth;
use nori_tui_components::PickerDensity;
use nori_tui_components::PickerItem;
use nori_tui_components::PickerState;
use nori_tui_components::SearchMode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::CancellationEvent;
use crate::render::renderable::Renderable;

use super::hotkey_match::key_event_to_binding;

/// State for the hotkey picker view.
pub(crate) struct HotkeyPickerView {
    /// All actions with their current bindings.
    entries: Vec<(HotkeyAction, HotkeyBinding)>,
    /// Currently highlighted row.
    selected_idx: usize,
    /// Whether we are in rebinding mode (waiting for a key press).
    rebinding: bool,
    /// The binding that was active before rebinding started (for cancel/restore).
    pre_rebind_binding: Option<HotkeyBinding>,
    /// Whether the view should be dismissed.
    complete: bool,
    /// Channel to send config change events.
    app_event_tx: AppEventSender,
}

impl HotkeyPickerView {
    pub fn new(config: &HotkeyConfig, app_event_tx: AppEventSender) -> Self {
        let entries: Vec<(HotkeyAction, HotkeyBinding)> = config
            .all_bindings()
            .into_iter()
            .map(|(action, binding)| (action, binding.clone()))
            .collect();

        Self {
            entries,
            selected_idx: 0,
            rebinding: false,
            pre_rebind_binding: None,
            complete: false,
            app_event_tx,
        }
    }

    fn move_up(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        if self.selected_idx == 0 {
            self.selected_idx = self.entries.len() - 1;
        } else {
            self.selected_idx -= 1;
        }
    }

    fn move_down(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.selected_idx = (self.selected_idx + 1) % self.entries.len();
    }

    fn start_rebind(&mut self) {
        if let Some((_, binding)) = self.entries.get(self.selected_idx) {
            self.pre_rebind_binding = Some(binding.clone());
            self.rebinding = true;
            // Clear the current binding to show "(press key...)"
            if let Some((_, binding)) = self.entries.get_mut(self.selected_idx) {
                *binding = HotkeyBinding::none();
            }
        }
    }

    fn cancel_rebind(&mut self) {
        if let Some(old_binding) = self.pre_rebind_binding.take()
            && let Some((_, binding)) = self.entries.get_mut(self.selected_idx)
        {
            *binding = old_binding;
        }
        self.rebinding = false;
    }

    fn apply_rebind(&mut self, new_binding: HotkeyBinding) {
        let selected_idx = self.selected_idx;

        // Check for conflicts: if another action has this binding, swap
        if !new_binding.is_none() {
            let old_binding = self
                .pre_rebind_binding
                .take()
                .unwrap_or_else(HotkeyBinding::none);
            for (idx, (_, binding)) in self.entries.iter_mut().enumerate() {
                if idx != selected_idx && *binding == new_binding {
                    // Swap: give the conflicting action our old binding
                    *binding = old_binding;
                    let conflict_action = self.entries[idx].0;
                    self.app_event_tx.send(AppEvent::SetConfigHotkey {
                        action: conflict_action,
                        binding: self.entries[idx].1.clone(),
                    });
                    break;
                }
            }
        } else {
            self.pre_rebind_binding = None;
        }

        // Set the new binding
        if let Some((action, binding)) = self.entries.get_mut(selected_idx) {
            *binding = new_binding.clone();
            self.app_event_tx.send(AppEvent::SetConfigHotkey {
                action: *action,
                binding: new_binding,
            });
        }

        self.rebinding = false;
    }

    #[cfg(test)]
    pub(crate) fn entries(&self) -> &[(HotkeyAction, HotkeyBinding)] {
        &self.entries
    }

    #[cfg(test)]
    pub(crate) fn selected_idx(&self) -> usize {
        self.selected_idx
    }

    #[cfg(test)]
    pub(crate) fn is_rebinding(&self) -> bool {
        self.rebinding
    }

    fn picker_state(&self) -> PickerState<usize> {
        let items = self
            .entries
            .iter()
            .enumerate()
            .map(|(index, (action, binding))| {
                let binding = if self.rebinding && index == self.selected_idx {
                    "(press key...)".to_string()
                } else {
                    binding.display_name()
                };
                PickerItem::new(index, "action", action.display_name()).cell("binding", binding)
            })
            .collect::<Vec<_>>();
        let mut state = PickerState::new(
            "Hotkeys",
            [
                PickerColumn::flexible("action", "Action").width(PickerColumnWidth::Flexible {
                    min: 18,
                    max: 48,
                    weight: 1,
                }),
                PickerColumn::fixed("binding", "Binding", 20),
            ],
            items,
        )
        .subtitle("Configure keyboard shortcuts")
        .search_mode(SearchMode::None);
        state.selected_index = Some(self.selected_idx);
        state
    }
}

impl BottomPaneView for HotkeyPickerView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.kind != KeyEventKind::Press && key_event.kind != KeyEventKind::Repeat {
            return;
        }

        if self.rebinding {
            match key_event.code {
                KeyCode::Esc => {
                    self.cancel_rebind();
                }
                _ => {
                    // Capture this key as the new binding
                    if let Some(new_binding) = key_event_to_binding(&key_event) {
                        self.apply_rebind(new_binding);
                    }
                    // If key_event_to_binding returns None (unsupported key), ignore
                }
            }
            return;
        }

        match key_event {
            KeyEvent {
                code: KeyCode::Up, ..
            }
            | KeyEvent {
                code: KeyCode::Char('k'),
                modifiers: KeyModifiers::NONE,
                ..
            } => self.move_up(),

            KeyEvent {
                code: KeyCode::Down,
                ..
            }
            | KeyEvent {
                code: KeyCode::Char('j'),
                modifiers: KeyModifiers::NONE,
                ..
            } => self.move_down(),

            KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                ..
            } => self.start_rebind(),

            KeyEvent {
                code: KeyCode::Esc, ..
            } => {
                self.on_ctrl_c();
            }

            // Pressing 'r' on a selected action resets it to default
            KeyEvent {
                code: KeyCode::Char('r'),
                modifiers: KeyModifiers::NONE,
                ..
            } => {
                if let Some((action, binding)) = self.entries.get_mut(self.selected_idx) {
                    let default = HotkeyBinding::from_str(action.default_binding());
                    *binding = default.clone();
                    self.app_event_tx.send(AppEvent::SetConfigHotkey {
                        action: *action,
                        binding: default,
                    });
                }
            }

            _ => {}
        }
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.complete = true;
        CancellationEvent::Handled
    }
}

impl Renderable for HotkeyPickerView {
    fn desired_height(&self, _width: u16) -> u16 {
        (self.entries.len() as u16).saturating_add(6)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let state = self.picker_state();
        let hints = if self.rebinding {
            vec![
                KeyHint::new("press", "new binding"),
                KeyHint::new("esc", "cancel"),
            ]
        } else {
            vec![
                KeyHint::new("↑↓/j/k", "move"),
                KeyHint::new("enter", "rebind"),
                KeyHint::new("r", "reset"),
                KeyHint::new("esc", "close"),
            ]
        };
        Picker::new(&state)
            .theme(crate::style::component_theme())
            .density(PickerDensity::Compact)
            .fullscreen_selection_rails(true)
            .footer_hints(hints)
            .render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    use pretty_assertions::assert_eq;
    use tokio::sync::mpsc::unbounded_channel;

    fn make_picker() -> (
        HotkeyPickerView,
        tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    ) {
        let (tx_raw, rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let config = HotkeyConfig::default();
        let picker = HotkeyPickerView::new(&config, tx);
        (picker, rx)
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn picker_starts_with_default_bindings() {
        let (picker, _rx) = make_picker();
        assert_eq!(picker.entries().len(), 16);
        assert_eq!(picker.entries()[0].0, HotkeyAction::OpenTranscript);
        assert_eq!(picker.entries()[0].1, HotkeyBinding::from_str("ctrl+t"));
        assert_eq!(picker.entries()[1].0, HotkeyAction::OpenEditor);
        assert_eq!(picker.entries()[1].1, HotkeyBinding::from_str("ctrl+g"));
    }

    #[test]
    fn picker_navigation_up_down() {
        let (mut picker, _rx) = make_picker();
        let entry_count = picker.entries().len();
        assert_eq!(picker.selected_idx(), 0);

        picker.handle_key_event(key(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(picker.selected_idx(), 1);

        // Navigate to the last entry and wrap around
        for _ in 1..entry_count {
            picker.handle_key_event(key(KeyCode::Down, KeyModifiers::NONE));
        }
        assert_eq!(picker.selected_idx(), 0); // wraps

        picker.handle_key_event(key(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(picker.selected_idx(), entry_count - 1); // wraps backward
    }

    #[test]
    fn picker_jk_navigation() {
        let (mut picker, _rx) = make_picker();
        assert_eq!(picker.selected_idx(), 0);

        picker.handle_key_event(key(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(picker.selected_idx(), 1);

        picker.handle_key_event(key(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(picker.selected_idx(), 0);
    }

    #[test]
    fn picker_render_uses_shared_symmetric_selection_rails() {
        let (picker, _rx) = make_picker();
        let area = Rect::new(0, 0, 80, picker.desired_height(80));
        let mut buffer = Buffer::empty(area);

        picker.render(area, &mut buffer);

        let label = picker.entries()[0].0.display_name();
        let selected_row = (area.y..area.bottom())
            .find(|&y| {
                (area.x..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .contains(label)
            })
            .expect("selected hotkey row");
        assert!(
            (area.x..area.right()).any(|x| buffer[(x, selected_row)].symbol() == "▏"),
            "selected row should have a left rail"
        );
        assert!(
            (area.x..area.right()).any(|x| buffer[(x, selected_row)].symbol() == "▕"),
            "selected row should have a right rail"
        );
    }

    #[test]
    fn picker_enter_starts_rebinding() {
        let (mut picker, _rx) = make_picker();
        assert!(!picker.is_rebinding());

        picker.handle_key_event(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(picker.is_rebinding());
        // The binding should be cleared during rebind
        assert!(picker.entries()[0].1.is_none());
    }

    #[test]
    fn picker_esc_cancels_rebinding() {
        let (mut picker, _rx) = make_picker();

        picker.handle_key_event(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(picker.is_rebinding());

        picker.handle_key_event(key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!picker.is_rebinding());
        // Original binding should be restored
        assert_eq!(picker.entries()[0].1, HotkeyBinding::from_str("ctrl+t"));
    }

    #[test]
    fn picker_rebind_applies_new_key() {
        let (mut picker, mut rx) = make_picker();

        // Start rebinding Open Transcript
        picker.handle_key_event(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(picker.is_rebinding());

        // Press F5 as the new binding (avoids conflicts with existing bindings)
        picker.handle_key_event(key(KeyCode::F(5), KeyModifiers::NONE));
        assert!(!picker.is_rebinding());
        assert_eq!(picker.entries()[0].1, HotkeyBinding::from_str("f5"));

        // Should have sent a SetConfigHotkey event
        let event = rx.try_recv().expect("should receive event");
        match event {
            AppEvent::SetConfigHotkey { action, binding } => {
                assert_eq!(action, HotkeyAction::OpenTranscript);
                assert_eq!(binding, HotkeyBinding::from_str("f5"));
            }
            _ => panic!("expected SetConfigHotkey event, got: {event:?}"),
        }
    }

    #[test]
    fn picker_rebind_conflict_swaps_bindings() {
        let (mut picker, mut rx) = make_picker();

        // Start rebinding Open Transcript (currently ctrl+t)
        picker.handle_key_event(key(KeyCode::Enter, KeyModifiers::NONE));

        // Press Ctrl+G which is currently bound to Open Editor
        picker.handle_key_event(key(KeyCode::Char('g'), KeyModifiers::CONTROL));

        // Open Transcript should now be ctrl+g
        assert_eq!(picker.entries()[0].1, HotkeyBinding::from_str("ctrl+g"));
        // Open Editor should have been swapped to ctrl+t (the old binding)
        assert_eq!(picker.entries()[1].1, HotkeyBinding::from_str("ctrl+t"));

        // Should have sent events for both the conflict swap and the new binding
        let event1 = rx.try_recv().expect("should receive first event");
        let event2 = rx.try_recv().expect("should receive second event");

        // The conflict swap event (Open Editor -> ctrl+t)
        match event1 {
            AppEvent::SetConfigHotkey { action, binding } => {
                assert_eq!(action, HotkeyAction::OpenEditor);
                assert_eq!(binding, HotkeyBinding::from_str("ctrl+t"));
            }
            _ => panic!("expected SetConfigHotkey for conflict, got: {event1:?}"),
        }

        // The new binding event (Open Transcript -> ctrl+g)
        match event2 {
            AppEvent::SetConfigHotkey { action, binding } => {
                assert_eq!(action, HotkeyAction::OpenTranscript);
                assert_eq!(binding, HotkeyBinding::from_str("ctrl+g"));
            }
            _ => panic!("expected SetConfigHotkey for new binding, got: {event2:?}"),
        }
    }

    #[test]
    fn picker_reset_to_default() {
        let (mut picker, mut rx) = make_picker();

        // First rebind Open Transcript to something else (F5 avoids conflicts)
        picker.handle_key_event(key(KeyCode::Enter, KeyModifiers::NONE));
        picker.handle_key_event(key(KeyCode::F(5), KeyModifiers::NONE));
        let _ = rx.try_recv(); // consume the rebind event

        // Now press 'r' to reset
        picker.handle_key_event(key(KeyCode::Char('r'), KeyModifiers::NONE));
        assert_eq!(picker.entries()[0].1, HotkeyBinding::from_str("ctrl+t"));

        let event = rx.try_recv().expect("should receive reset event");
        match event {
            AppEvent::SetConfigHotkey { action, binding } => {
                assert_eq!(action, HotkeyAction::OpenTranscript);
                assert_eq!(binding, HotkeyBinding::from_str("ctrl+t"));
            }
            _ => panic!("expected SetConfigHotkey for reset, got: {event:?}"),
        }
    }

    #[test]
    fn picker_esc_closes_when_not_rebinding() {
        let (mut picker, _rx) = make_picker();
        assert!(!picker.is_complete());

        picker.handle_key_event(key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(picker.is_complete());
    }

    #[test]
    fn picker_renders_without_panic() {
        let (picker, _rx) = make_picker();
        let width = 60;
        let height = picker.desired_height(width);
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        picker.render(area, &mut buf);

        // Verify the output contains expected text
        let text: String = (0..area.height)
            .map(|row| {
                (0..area.width)
                    .map(|col| {
                        let s = buf[(col, row)].symbol();
                        if s.is_empty() { " " } else { s }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("Hotkeys"), "should contain title");
        assert!(
            text.contains("Open Transcript"),
            "should contain action name"
        );
        assert!(
            text.contains("Open Editor"),
            "should contain second action name"
        );
    }
}
