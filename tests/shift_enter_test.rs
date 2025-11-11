use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nori_cli::app::{AppMode, Message};

#[test]
fn test_plain_enter_returns_submit_message() {
    // This test verifies that plain Enter (no modifiers) triggers submission
    let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let mode = AppMode::Input;
    let show_overlay = false;
    let show_install_prompt = false;

    let result =
        nori_cli::input::handle_key_simple(mode, show_overlay, show_install_prompt, None, key);
    assert_eq!(result, Some(Message::SubmitInput));
}

#[test]
fn test_alt_enter_returns_keypress_message() {
    // This test verifies that Alt+Enter passes through as KeyPress
    // so the textarea can insert a newline
    // Note: Using Alt instead of Shift because terminals don't reliably send SHIFT modifier for Enter
    let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
    let mode = AppMode::Input;
    let show_overlay = false;
    let show_install_prompt = false;

    let result =
        nori_cli::input::handle_key_simple(mode, show_overlay, show_install_prompt, None, key);

    match result {
        Some(Message::KeyPress(k)) => {
            assert_eq!(k.code, KeyCode::Enter);
            assert!(k.modifiers.contains(KeyModifiers::ALT));
        }
        _ => panic!(
            "Expected KeyPress message with Alt+Enter, got {:?}",
            result
        ),
    }
}

#[test]
fn test_alt_enter_during_streaming_is_ignored() {
    // During streaming, all input except Esc should be ignored
    let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
    let mode = AppMode::Streaming;
    let show_overlay = false;
    let show_install_prompt = false;

    let result =
        nori_cli::input::handle_key_simple(mode, show_overlay, show_install_prompt, None, key);
    assert_eq!(result, None);
}

#[test]
fn test_alt_enter_with_overlay_open_selects_item() {
    // When overlay is open, any Enter key (including Alt+Enter) selects an item
    // This is the expected behavior - overlay takes precedence
    let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
    let mode = AppMode::Input;
    let show_overlay = true;
    let show_install_prompt = false;

    let result =
        nori_cli::input::handle_key_simple(mode, show_overlay, show_install_prompt, None, key);
    assert_eq!(result, Some(Message::SelectItem));
}
