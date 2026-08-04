use std::collections::BTreeMap;

use codex_tui_components::Picker;
use codex_tui_components::PickerAction;
use codex_tui_components::PickerDensity;
use codex_tui_components::PickerOutcome;
use codex_tui_components::PickerState;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::app_event_sender::AppEventSender;
use crate::render::renderable::Renderable;

use super::BottomPaneView;
use super::CancellationEvent;
use super::SelectionAction;

pub(crate) struct ComponentPickerParams {
    pub state: PickerState<String>,
    pub actions: BTreeMap<String, SelectionAction>,
    pub on_dismiss: Option<SelectionAction>,
    pub primary_column: String,
    pub detail_column: Option<String>,
    pub density: PickerDensity,
}

pub(crate) struct ComponentPickerView {
    state: PickerState<String>,
    actions: BTreeMap<String, SelectionAction>,
    on_dismiss: Option<SelectionAction>,
    app_event_tx: AppEventSender,
    complete: bool,
    primary_column: String,
    detail_column: Option<String>,
    density: PickerDensity,
}

impl ComponentPickerView {
    pub(crate) fn new(params: ComponentPickerParams, app_event_tx: AppEventSender) -> Self {
        Self {
            state: params.state,
            actions: params.actions,
            on_dismiss: params.on_dismiss,
            app_event_tx,
            complete: false,
            primary_column: params.primary_column,
            detail_column: params.detail_column,
            density: params.density,
        }
    }

    fn handle_action(&mut self, action: PickerAction) {
        match self.state.handle(action) {
            PickerOutcome::Selected(key) => {
                if let Some(action) = self.actions.get(&key) {
                    action(&self.app_event_tx);
                    self.complete = true;
                }
            }
            PickerOutcome::Cancelled => {
                if let Some(on_dismiss) = &self.on_dismiss {
                    on_dismiss(&self.app_event_tx);
                }
                self.complete = true;
            }
            PickerOutcome::Unchanged
            | PickerOutcome::SelectionChanged(_)
            | PickerOutcome::Toggled { .. }
            | PickerOutcome::Submitted(_)
            | PickerOutcome::QueryChanged(_)
            | PickerOutcome::CategoryChanged(_) => {}
        }
    }
}

impl BottomPaneView for ComponentPickerView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        let action = match key_event {
            KeyEvent {
                code: KeyCode::Char('c'),
                modifiers,
                ..
            } if modifiers.contains(KeyModifiers::CONTROL) => PickerAction::Cancel,
            KeyEvent {
                code: KeyCode::Esc, ..
            } => PickerAction::Cancel,
            KeyEvent {
                code: KeyCode::Up, ..
            } => PickerAction::MoveUp,
            KeyEvent {
                code: KeyCode::Down,
                ..
            } => PickerAction::MoveDown,
            KeyEvent {
                code: KeyCode::PageUp,
                ..
            } => PickerAction::PageUp,
            KeyEvent {
                code: KeyCode::PageDown,
                ..
            } => PickerAction::PageDown,
            KeyEvent {
                code: KeyCode::Home,
                ..
            } => PickerAction::First,
            KeyEvent {
                code: KeyCode::End, ..
            } => PickerAction::Last,
            KeyEvent {
                code: KeyCode::Enter,
                ..
            } => PickerAction::Submit,
            KeyEvent {
                code: KeyCode::Backspace,
                ..
            } => PickerAction::Backspace,
            KeyEvent {
                code: KeyCode::Tab, ..
            } => PickerAction::NextCategory,
            KeyEvent {
                code: KeyCode::BackTab,
                ..
            } => PickerAction::PreviousCategory,
            KeyEvent {
                code: KeyCode::Char(character),
                modifiers,
                ..
            } if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT => {
                PickerAction::AppendQuery(character)
            }
            _ => return,
        };
        self.handle_action(action);
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.handle_action(PickerAction::Cancel);
        CancellationEvent::Handled
    }

    fn handle_paste(&mut self, pasted: String) -> bool {
        for character in pasted.chars().filter(|character| !character.is_control()) {
            self.state.handle(PickerAction::AppendQuery(character));
        }
        true
    }

    fn update_selection_item(
        &mut self,
        stable_id: &str,
        name: String,
        description: Option<String>,
        search_value: String,
    ) -> bool {
        let Some(item) = self
            .state
            .items
            .iter_mut()
            .find(|item| item.key == stable_id)
        else {
            return false;
        };
        item.cells.insert(self.primary_column.clone(), name);
        if let Some(detail_column) = &self.detail_column {
            if let Some(description) = description {
                item.cells.insert(detail_column.clone(), description);
            } else {
                item.cells.remove(detail_column);
            }
        }
        item.search_text = search_value;
        true
    }

    fn remove_selection_item(&mut self, stable_id: &str) -> bool {
        let Some(index) = self
            .state
            .items
            .iter()
            .position(|item| item.key == stable_id)
        else {
            return false;
        };
        self.state.items.remove(index);
        if self.state.selected_index == Some(index) {
            self.state.selected_index = self.state.visible_indices().first().copied();
        } else if self
            .state
            .selected_index
            .is_some_and(|selected| selected > index)
        {
            self.state.selected_index = self.state.selected_index.map(|selected| selected - 1);
        }
        true
    }
}

impl Renderable for ComponentPickerView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        Picker::new(&self.state)
            .theme(crate::style::component_theme())
            .density(self.density)
            .render(area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        let rows = self.state.visible_indices().len().clamp(1, 10) as u16;
        let row_height = match self.density {
            PickerDensity::Compact => 1,
            PickerDensity::Normal => 2,
        };
        rows.saturating_mul(row_height)
            .saturating_add(7)
            .clamp(9, 18)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_tui_components::PickerColumn;
    use codex_tui_components::PickerItem;
    use pretty_assertions::assert_eq;

    fn params() -> ComponentPickerParams {
        ComponentPickerParams {
            state: PickerState::new(
                "Sessions",
                [PickerColumn::flexible("session", "Session")],
                [PickerItem::new("one".to_string(), "session", "First")],
            ),
            actions: BTreeMap::new(),
            on_dismiss: None,
            primary_column: "session".to_string(),
            detail_column: None,
            density: PickerDensity::Compact,
        }
    }

    #[test]
    fn dynamic_updates_are_applied_by_stable_key() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut view = ComponentPickerView::new(params(), AppEventSender::new(tx));

        assert!(view.update_selection_item(
            "one",
            "Updated".to_string(),
            None,
            "updated search".to_string(),
        ));
        assert_eq!(
            view.state.items[0].cells.get("session").map(String::as_str),
            Some("Updated")
        );
        assert_eq!(view.state.items[0].search_text, "updated search");
    }
}
