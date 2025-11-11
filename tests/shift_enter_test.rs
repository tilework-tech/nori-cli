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
fn test_shift_enter_returns_keypress_message() {
    // This test verifies that Shift+Enter passes through as KeyPress
    // so the textarea can insert a newline
    let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
    let mode = AppMode::Input;
    let show_overlay = false;
    let show_install_prompt = false;

    let result =
        nori_cli::input::handle_key_simple(mode, show_overlay, show_install_prompt, None, key);

    // Currently this will fail because Shift+Enter is treated as SubmitInput
    // After implementation, it should return KeyPress
    match result {
        Some(Message::KeyPress(k)) => {
            assert_eq!(k.code, KeyCode::Enter);
            assert!(k.modifiers.contains(KeyModifiers::SHIFT));
        }
        _ => panic!(
            "Expected KeyPress message with Shift+Enter, got {:?}",
            result
        ),
    }
}

#[test]
fn test_shift_enter_during_streaming_is_ignored() {
    // During streaming, all input except Esc should be ignored
    let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
    let mode = AppMode::Streaming;
    let show_overlay = false;
    let show_install_prompt = false;

    let result =
        nori_cli::input::handle_key_simple(mode, show_overlay, show_install_prompt, None, key);
    assert_eq!(result, None);
}

#[test]
fn test_shift_enter_with_overlay_open_selects_item() {
    // When overlay is open, any Enter key (including Shift+Enter) selects an item
    // This is the expected behavior - overlay takes precedence
    let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
    let mode = AppMode::Input;
    let show_overlay = true;
    let show_install_prompt = false;

    let result =
        nori_cli::input::handle_key_simple(mode, show_overlay, show_install_prompt, None, key);
    assert_eq!(result, Some(Message::SelectItem));
}
