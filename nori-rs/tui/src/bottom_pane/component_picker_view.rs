use std::collections::BTreeMap;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use nori_tui_components::Picker;
use nori_tui_components::PickerAction;
use nori_tui_components::PickerDensity;
use nori_tui_components::PickerOutcome;
use nori_tui_components::PickerState;
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
            | PickerOutcome::SearchModeChanged(_)
            | PickerOutcome::QueryChanged(_)
            | PickerOutcome::CategoryChanged(_) => {}
        }
    }
}

impl BottomPaneView for ComponentPickerView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        let search_active = self.state.search_active;
        let action = match key_event {
            KeyEvent {
                code: KeyCode::Char('c'),
                modifiers,
                ..
            } if modifiers.contains(KeyModifiers::CONTROL) => PickerAction::Cancel,
            KeyEvent {
                code: KeyCode::Esc, ..
            } if search_active => PickerAction::DeactivateSearch,
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
            } if search_active => PickerAction::Backspace,
            KeyEvent {
                code: KeyCode::Tab, ..
            } => PickerAction::NextCategory,
            KeyEvent {
                code: KeyCode::BackTab,
                ..
            } => PickerAction::PreviousCategory,
            KeyEvent {
                code: KeyCode::Char('f'),
                modifiers,
                ..
            } if modifiers == KeyModifiers::CONTROL && !search_active => {
                PickerAction::ActivateSearch
            }
            KeyEvent {
                code: KeyCode::Char('f' | '/'),
                modifiers: KeyModifiers::NONE,
                ..
            } if !search_active => PickerAction::ActivateSearch,
            KeyEvent {
                code: KeyCode::Char('k'),
                modifiers: KeyModifiers::NONE,
                ..
            } if !search_active => PickerAction::MoveUp,
            KeyEvent {
                code: KeyCode::Char('j'),
                modifiers: KeyModifiers::NONE,
                ..
            } if !search_active => PickerAction::MoveDown,
            KeyEvent {
                code: KeyCode::Char(character),
                modifiers,
                ..
            } if search_active && (modifiers.is_empty() || modifiers == KeyModifiers::SHIFT) => {
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

    fn on_escape(&mut self) -> CancellationEvent {
        let action = if self.state.search_active {
            PickerAction::DeactivateSearch
        } else {
            PickerAction::Cancel
        };
        self.handle_action(action);
        CancellationEvent::Handled
    }

    fn handle_paste(&mut self, pasted: String) -> bool {
        if !self.state.search_active {
            return true;
        }
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
            .fullscreen_selection_rails(true)
            .render(area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        let rows = self.state.visible_indices().len().clamp(1, 10) as u16;
        let row_height = match self.density {
            PickerDensity::Compact => 1,
            PickerDensity::Normal => 2,
        };
        rows.saturating_mul(row_height)
            .saturating_add(6 + u16::from(self.state.search_active))
            .clamp(9, 18)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use nori_tui_components::PickerColumn;
    use nori_tui_components::PickerItem;
    use pretty_assertions::assert_eq;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn params() -> ComponentPickerParams {
        ComponentPickerParams {
            state: PickerState::new(
                "Sessions",
                [PickerColumn::flexible("session", "Session")],
                [
                    PickerItem::new("one".to_string(), "session", "First").search_text("alpha"),
                    PickerItem::new("two".to_string(), "session", "Second").search_text("beta"),
                    PickerItem::new("three".to_string(), "session", "Third").search_text("gamma"),
                ],
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

    #[test]
    fn cli_picker_renders_symmetric_selection_rails() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let view = ComponentPickerView::new(params(), AppEventSender::new(tx));
        let area = Rect::new(0, 0, 48, 12);
        let mut buffer = Buffer::empty(area);

        view.render(area, &mut buffer);

        let rendered_rows = (area.y..area.bottom())
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let selected_row = rendered_rows
            .iter()
            .find(|row| row.windows(5).any(|cells| cells.concat() == "First"))
            .expect("selected row");
        let left_rail = selected_row.iter().position(|symbol| *symbol == "▏");
        let right_rail = selected_row.iter().position(|symbol| *symbol == "▕");
        let label = selected_row
            .windows(5)
            .position(|cells| cells.concat() == "First")
            .expect("selected label");
        assert!(left_rail.is_some_and(|rail| rail < label));
        assert!(right_rail.is_some_and(|rail| rail > label));

        let unselected_row = rendered_rows
            .iter()
            .find(|row| row.windows(6).any(|cells| cells.concat() == "Second"))
            .expect("unselected row");
        assert!(!unselected_row.contains(&"▏"));
        assert!(!unselected_row.contains(&"▕"));
    }

    #[test]
    fn inactive_searchable_picker_uses_jk_for_navigation() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut view = ComponentPickerView::new(params(), AppEventSender::new(tx));

        view.handle_key_event(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(view.state.selected_index, Some(1));
        assert_eq!(view.state.query, "");

        view.handle_key_event(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(view.state.selected_index, Some(0));
        assert_eq!(view.state.query, "");
    }

    #[test]
    fn inactive_searchable_picker_ignores_other_printable_characters() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut view = ComponentPickerView::new(params(), AppEventSender::new(tx));

        view.handle_key_event(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));

        assert_eq!(view.state.query, "");
        assert_eq!(view.state.selected_index, Some(0));
    }

    #[test]
    fn picker_search_activation_keys_do_not_enter_the_query() {
        let activation_keys = [
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
        ];

        for activation_key in activation_keys {
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            let mut view = ComponentPickerView::new(params(), AppEventSender::new(tx));

            view.handle_key_event(activation_key);
            view.handle_key_event(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
            assert_eq!(view.state.query, "a", "activation key: {activation_key:?}");

            view.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            assert_eq!(view.state.query, "", "activation key: {activation_key:?}");
            assert!(!view.is_complete(), "activation key: {activation_key:?}");

            view.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            assert!(view.is_complete(), "activation key: {activation_key:?}");
        }
    }

    #[test]
    fn empty_active_picker_search_exits_before_the_picker_dismisses() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut view = ComponentPickerView::new(params(), AppEventSender::new(tx));

        view.handle_key_event(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        view.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!view.is_complete());

        view.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(view.is_complete());
    }

    #[test]
    fn active_picker_search_receives_reserved_and_general_printable_characters() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut view = ComponentPickerView::new(params(), AppEventSender::new(tx));

        view.handle_key_event(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for (character, modifiers) in [
            ('j', KeyModifiers::NONE),
            ('k', KeyModifiers::NONE),
            ('f', KeyModifiers::NONE),
            ('/', KeyModifiers::NONE),
            ('A', KeyModifiers::SHIFT),
            ('7', KeyModifiers::NONE),
            (' ', KeyModifiers::NONE),
            ('?', KeyModifiers::NONE),
            ('λ', KeyModifiers::NONE),
        ] {
            view.handle_key_event(KeyEvent::new(KeyCode::Char(character), modifiers));
        }

        assert_eq!(view.state.query, "jkf/A7 ?λ");
    }

    #[test]
    fn picker_search_filters_before_running_the_selected_action() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut params = params();
        params.actions.insert(
            "two".to_string(),
            Box::new(|tx| tx.send(AppEvent::BeginExit)),
        );
        let mut view = ComponentPickerView::new(params, AppEventSender::new(tx));

        view.handle_key_event(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        for character in "beta".chars() {
            view.handle_key_event(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        view.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(matches!(rx.try_recv(), Ok(AppEvent::BeginExit)));
    }
}
