use crate::app::{AppMode, Message};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn handle_key_simple(
    mode: AppMode,
    show_overlay: bool,
    show_install_prompt: bool,
    _last_ctrl_c_time: Option<std::time::Instant>,
    key: KeyEvent,
) -> Option<Message> {
    // Check for Ctrl-C FIRST (even with overlays/install prompt open)
    // This ensures double Ctrl-C always works to exit
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Message::ClearTextarea);
    }

    // Install prompt takes highest precedence
    if show_install_prompt {
        return match key.code {
            KeyCode::Up | KeyCode::Char('k') => Some(Message::NavigateInstallChoice),
            KeyCode::Down | KeyCode::Char('j') => Some(Message::NavigateInstallChoice),
            KeyCode::Enter => Some(Message::ConfirmInstall),
            KeyCode::Esc => Some(Message::CancelInstall),
            _ => None,
        };
    }

    // If overlay is open, handle navigation
    if show_overlay {
        return match key.code {
            KeyCode::Up | KeyCode::Char('k') => Some(Message::PreviousItem),
            KeyCode::Down | KeyCode::Char('j') => Some(Message::NextItem),
            KeyCode::Enter => Some(Message::SelectItem),
            KeyCode::Esc => Some(Message::ExitInputMode),
            _ => None,
        };
    }

    // If streaming, only allow Esc to cancel
    if mode == AppMode::Streaming {
        return match key.code {
            KeyCode::Esc => Some(Message::CancelStream),
            _ => None,
        };
    }

    // Otherwise, handle chat input
    // Shift+Enter inserts newline (pass through to textarea)
    if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::SHIFT) {
        return Some(Message::KeyPress(key));
    }

    // Plain Enter submits
    if key.code == KeyCode::Enter && key.modifiers.is_empty() {
        return Some(Message::SubmitInput);
    }

    // Send all other key events to textarea
    Some(Message::KeyPress(key))
}
