use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crossterm::event::KeyEvent;
use nori_tui_components::MenuDensity;
use nori_tui_components::MenuItem;
use nori_tui_components::MenuOutcome;
use nori_tui_components::MenuPlacement;
use nori_tui_components::MenuRowPattern;
use nori_tui_components::MenuState;
use nori_tui_components::OverlayMenu;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::StatefulWidget;

use crate::app_event_sender::AppEventSender;
use crate::render::renderable::Renderable;

use super::BottomPaneView;
use super::CancellationEvent;
use super::SelectionAction;
use super::SelectionViewParams;

pub(crate) struct ComponentOverlayMenuView {
    title: String,
    subtitle: Option<String>,
    state: RefCell<MenuState<String>>,
    actions: BTreeMap<String, SelectionAction>,
    keep_open: BTreeSet<String>,
    on_dismiss: Option<SelectionAction>,
    app_event_tx: AppEventSender,
    complete: bool,
    max_width: u16,
    density: MenuDensity,
    row_pattern: MenuRowPattern,
    placement: MenuPlacement,
}

impl ComponentOverlayMenuView {
    pub(crate) fn new(params: SelectionViewParams, app_event_tx: AppEventSender) -> Self {
        let mut actions = BTreeMap::new();
        let mut keep_open = BTreeSet::new();
        let mut shortcut = 0;
        let initial_selected_idx = params
            .initial_selected_idx
            .or_else(|| params.items.iter().position(|item| item.is_current));
        let items = params
            .items
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                let key = format!("menu-{index}");
                let disabled = item.is_header;
                let mut menu_item = MenuItem::new(key.clone(), item.name)
                    .disabled(disabled)
                    .current(item.is_current)
                    .tone(item.menu_tone);
                if let Some(description) = item.description {
                    menu_item = menu_item.description(description);
                }
                if !disabled {
                    shortcut += 1;
                }
                if shortcut <= 9 && !disabled {
                    menu_item = menu_item.number_shortcut(shortcut);
                }
                if !item.actions.is_empty() {
                    let callbacks = item.actions;
                    actions.insert(
                        key.clone(),
                        Box::new(move |tx: &AppEventSender| {
                            for callback in &callbacks {
                                callback(tx);
                            }
                        }) as SelectionAction,
                    );
                    if !item.dismiss_on_select {
                        keep_open.insert(key);
                    }
                }
                menu_item
            })
            .collect::<Vec<_>>();
        let mut state = crate::overlay_menu::state_from_items(items, "bottom pane selection");
        if let Some(index) = initial_selected_idx {
            state.select_key(&format!("menu-{index}"));
        }
        Self {
            title: params
                .title
                .unwrap_or_else(|| "Choose an action".to_string()),
            subtitle: params.subtitle,
            state: RefCell::new(state),
            actions,
            keep_open,
            on_dismiss: params.on_dismiss,
            app_event_tx,
            complete: false,
            max_width: params.menu_max_width,
            density: params.menu_density,
            row_pattern: params.menu_row_pattern,
            placement: params.menu_placement,
        }
    }

    fn handle_outcome(&mut self, outcome: MenuOutcome<String>) {
        match outcome {
            MenuOutcome::Activated(key) => {
                if let Some(action) = self.actions.get(&key) {
                    action(&self.app_event_tx);
                }
                self.complete = !self.keep_open.contains(&key);
            }
            MenuOutcome::Cancelled => {
                if let Some(on_dismiss) = &self.on_dismiss {
                    on_dismiss(&self.app_event_tx);
                }
                self.complete = true;
            }
            MenuOutcome::Unchanged | MenuOutcome::SelectionChanged(_) => {}
        }
    }

    fn cancel(&mut self) {
        let outcome = self
            .state
            .borrow_mut()
            .handle(nori_tui_components::MenuAction::Cancel);
        self.handle_outcome(outcome);
    }
}

impl BottomPaneView for ComponentOverlayMenuView {
    fn handle_key_event(&mut self, event: KeyEvent) {
        let Some(action) = crate::overlay_menu::action_from_key_event(event) else {
            return;
        };
        let outcome = self.state.borrow_mut().handle(action);
        self.handle_outcome(outcome);
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.cancel();
        CancellationEvent::Handled
    }

    fn on_escape(&mut self) -> CancellationEvent {
        self.cancel();
        CancellationEvent::Handled
    }
}

impl Renderable for ComponentOverlayMenuView {
    fn render(&self, area: Rect, buffer: &mut Buffer) {
        let mut menu = OverlayMenu::new(self.title.clone())
            .theme(crate::style::component_theme())
            .max_width(self.max_width)
            .density(self.density)
            .row_pattern(self.row_pattern)
            .placement(self.placement)
            .fullscreen_selection_rails(true)
            .key_hints(crate::overlay_menu::default_hints());
        if let Some(subtitle) = &self.subtitle {
            menu = menu.subtitle(subtitle.clone());
        }
        menu.render(area, buffer, &mut *self.state.borrow_mut());
    }

    fn desired_height(&self, _width: u16) -> u16 {
        let items = self.state.borrow().items().len() as u16;
        let minimum = if self.subtitle.is_some() { 14 } else { 9 };
        items.saturating_mul(2).saturating_add(5).clamp(minimum, 20)
    }
}
