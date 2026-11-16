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
