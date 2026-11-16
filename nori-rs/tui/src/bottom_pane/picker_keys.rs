//! The keys every picker shares: arrows and j/k move, Enter picks, `/`, `f`
//! or Ctrl+F starts a search, typing filters only while searching, and Esc
//! leaves the search. Esc outside a search and keys a picker adds for itself
//! stay with that picker.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use nori_tui_components::picker::PickerAction;

/// The picker action `key` stands for, or `None` when it is not a shared
/// picker key.
pub(crate) fn navigation_action(key: &KeyEvent, search_active: bool) -> Option<PickerAction> {
    let plain = key.modifiers.is_empty();
    Some(match key.code {
        KeyCode::Esc if search_active => PickerAction::DeactivateSearch,
        KeyCode::Up => PickerAction::MoveUp,
        KeyCode::Down => PickerAction::MoveDown,
        KeyCode::PageUp => PickerAction::PageUp,
        KeyCode::PageDown => PickerAction::PageDown,
        KeyCode::Home => PickerAction::First,
        KeyCode::End => PickerAction::Last,
        KeyCode::Enter => PickerAction::Submit,
        KeyCode::Backspace if search_active => PickerAction::Backspace,
        KeyCode::Char('f') if key.modifiers == KeyModifiers::CONTROL && !search_active => {
            PickerAction::ActivateSearch
        }
        KeyCode::Char('f' | '/') if plain && !search_active => PickerAction::ActivateSearch,
        KeyCode::Char('k') if plain && !search_active => PickerAction::MoveUp,
        KeyCode::Char('j') if plain && !search_active => PickerAction::MoveDown,
        KeyCode::Char(character)
            if search_active && (plain || key.modifiers == KeyModifiers::SHIFT) =>
        {
            PickerAction::AppendQuery(character)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn action(code: KeyCode, modifiers: KeyModifiers, search_active: bool) -> Option<PickerAction> {
        navigation_action(&KeyEvent::new(code, modifiers), search_active)
    }

    #[test]
    fn letters_move_or_start_a_search_until_one_is_running() {
        let none = KeyModifiers::NONE;
        assert_eq!(
            [
                action(KeyCode::Char('j'), none, false),
                action(KeyCode::Char('k'), none, false),
                action(KeyCode::Char('/'), none, false),
                action(KeyCode::Char('f'), none, false),
                action(KeyCode::Char('f'), KeyModifiers::CONTROL, false),
                action(KeyCode::Char('x'), none, false),
                action(KeyCode::Esc, none, false),
            ],
            [
                Some(PickerAction::MoveDown),
                Some(PickerAction::MoveUp),
                Some(PickerAction::ActivateSearch),
                Some(PickerAction::ActivateSearch),
                Some(PickerAction::ActivateSearch),
                None,
                None,
            ]
        );
        assert_eq!(
            [
                action(KeyCode::Char('j'), none, true),
                action(KeyCode::Char('/'), none, true),
                action(KeyCode::Char('X'), KeyModifiers::SHIFT, true),
                action(KeyCode::Backspace, none, true),
                action(KeyCode::Esc, none, true),
                action(KeyCode::Down, none, true),
            ],
            [
                Some(PickerAction::AppendQuery('j')),
                Some(PickerAction::AppendQuery('/')),
                Some(PickerAction::AppendQuery('X')),
                Some(PickerAction::Backspace),
                Some(PickerAction::DeactivateSearch),
                Some(PickerAction::MoveDown),
            ]
        );
    }
}
