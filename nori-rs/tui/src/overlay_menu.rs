use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use nori_tui_components::KeyHint;
use nori_tui_components::MenuAction;
use nori_tui_components::MenuItem;
use nori_tui_components::MenuShortcut;
use nori_tui_components::MenuState;

pub(crate) fn state_from_items<K: Eq>(
    items: impl IntoIterator<Item = MenuItem<K>>,
    context: &'static str,
) -> MenuState<K> {
    match MenuState::try_new(items) {
        Ok(state) => state,
        Err(error) => {
            tracing::error!(%error, context, "invalid overlay menu model");
            MenuState::empty()
        }
    }
}

pub(crate) fn action_from_key_event(event: KeyEvent) -> Option<MenuAction> {
    if event.kind == KeyEventKind::Release {
        return None;
    }
    let action = match event {
        KeyEvent {
            code: KeyCode::Char('c' | 'd'),
            modifiers,
            ..
        } if modifiers.contains(KeyModifiers::CONTROL) => MenuAction::Cancel,
        KeyEvent {
            code: KeyCode::Esc, ..
        } => MenuAction::Cancel,
        KeyEvent {
            code: KeyCode::Up | KeyCode::Char('k'),
            modifiers: KeyModifiers::NONE,
            ..
        } => MenuAction::MoveUp,
        KeyEvent {
            code: KeyCode::Down | KeyCode::Char('j'),
            modifiers: KeyModifiers::NONE,
            ..
        } => MenuAction::MoveDown,
        KeyEvent {
            code: KeyCode::PageUp,
            ..
        } => MenuAction::PageUp,
        KeyEvent {
            code: KeyCode::PageDown,
            ..
        } => MenuAction::PageDown,
        KeyEvent {
            code: KeyCode::Home,
            ..
        } => MenuAction::First,
        KeyEvent {
            code: KeyCode::End, ..
        } => MenuAction::Last,
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } => MenuAction::ActivateSelected,
        KeyEvent {
            code: KeyCode::Char(character @ '1'..='9'),
            modifiers: KeyModifiers::NONE,
            ..
        } => MenuAction::InvokeShortcut(MenuShortcut::Number(
            character.to_digit(10).unwrap_or_default() as i32,
        )),
        KeyEvent {
            code: KeyCode::Char(character),
            modifiers,
            ..
        } if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT => {
            MenuAction::InvokeShortcut(MenuShortcut::Character(character))
        }
        _ => return None,
    };
    Some(action)
}

pub(crate) fn default_hints() -> Vec<KeyHint<'static>> {
    vec![
        KeyHint::new("↑↓/j/k", "move"),
        KeyHint::new("1-9", "select"),
        KeyHint::new("enter", "select"),
        KeyHint::new("esc", "close"),
    ]
}
