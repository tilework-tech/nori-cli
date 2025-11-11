use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nori_cli::app::{Message, Model};

// NOTE: Automatic text wrapping on keystroke has been disabled because it resets
// cursor position, causing text to appear backwards. tui-textarea does not natively
// support visual line wrapping - it only supports horizontal scrolling.
//
// The text_wrapping module and infrastructure remain for potential future use if we:
// - Migrate to a different textarea library (e.g., edtui)
// - Implement custom rendering with proper cursor preservation
// - Use wrapping only on submit rather than on each keystroke

#[test]
fn test_textarea_does_not_autowrap_to_preserve_cursor() {
    let mut model = Model::default();

    // Set a narrow wrap width
    model.set_wrap_width(20);

    // Type a long line
    let long_text = "This is a very long line that exceeds width";
    for ch in long_text.chars() {
        let key_event = KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE);
        model.update(Message::KeyPress(key_event));
    }

    // The textarea should remain a single line (no automatic wrapping)
    // This is intentional - wrapping resets cursor position causing backwards text
    let lines = model.textarea.lines();
    assert_eq!(
        lines.len(),
        1,
        "Long lines are not automatically wrapped to preserve cursor"
    );

    // Verify text is in correct order
    assert_eq!(lines[0], long_text);
}

#[test]
fn test_textarea_preserves_user_newlines() {
    let mut model = Model::default();
    model.set_wrap_width(80);

    // Type "Line 1" + Enter + "Line 2"
    for ch in "Line 1".chars() {
        let key_event = KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE);
        model.update(Message::KeyPress(key_event));
    }

    let enter_event = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    model.update(Message::KeyPress(enter_event));

    for ch in "Line 2".chars() {
        let key_event = KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE);
        model.update(Message::KeyPress(key_event));
    }

    let lines = model.textarea.lines();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "Line 1");
    assert_eq!(lines[1], "Line 2");
}

#[test]
fn test_wrap_width_tracks_terminal_size() {
    let mut model = Model::default();

    // Wrap width starts at default
    assert_eq!(model.wrap_width, 80);

    // Change wrap width (simulating terminal resize)
    model.set_wrap_width(120);
    assert_eq!(model.wrap_width, 120);

    // Type some text - it should not be wrapped
    let text = "This is some text";
    for ch in text.chars() {
        let key_event = KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE);
        model.update(Message::KeyPress(key_event));
    }

    assert_eq!(model.textarea.lines().len(), 1);
}
